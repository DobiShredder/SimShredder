//! Per-user SimulationCraft installation registry with atomic activation and rollback.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};
use simc_adapter::{
    AvailableRuntime, InstalledRuntime, RuntimeManifest, SimcIdentity, download_available,
    install_supported_artifact, sha256_file, supported_executable_name, validate_supported_binary,
};

const STATE_SCHEMA_VERSION: u32 = 1;
const MAX_STATE_BYTES: u64 = 1024 * 1024;
const MAX_RUNTIME_METADATA_BYTES: u64 = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("SimulationCraft runtime error: {0}")]
    Adapter(#[from] simc_adapter::Error),
    #[error("runtime registry is invalid: {0}")]
    InvalidRegistry(String),
    #[error("runtime {0} is not installed")]
    NotInstalled(String),
    #[error("no previous runtime is available for rollback")]
    RollbackUnavailable,
    #[error("runtime integrity check failed: {0}")]
    Integrity(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeRecord {
    pub id: String,
    pub simc_version: String,
    pub build: String,
    pub game_version: String,
    pub channel: String,
    pub executable_sha256: String,
    pub installed_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeState {
    pub schema_version: u32,
    pub active_id: Option<String>,
    pub previous_id: Option<String>,
    pub runtimes: Vec<RuntimeRecord>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            active_id: None,
            previous_id: None,
            runtimes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDoctor {
    pub record: RuntimeRecord,
    pub executable: PathBuf,
    pub identity: SimcIdentity,
    pub healthy: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledRuntimeMetadata {
    manifest: RuntimeManifest,
    identity: SimcIdentity,
    executable_sha256: String,
}

#[derive(Clone, Debug)]
pub struct RuntimeManager {
    root: PathBuf,
}

impl RuntimeManager {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let manager = Self { root: root.into() };
        manager.ensure_layout()?;
        let _ = manager.state()?;
        Ok(manager)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn state(&self) -> Result<RuntimeState> {
        self.ensure_layout()?;
        let path = self.state_path();
        if !path.exists() {
            return Ok(RuntimeState::default());
        }
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_STATE_BYTES
        {
            return Err(Error::InvalidRegistry(
                "state must be a bounded regular file".into(),
            ));
        }
        let state: RuntimeState = serde_json::from_slice(&fs::read(path)?)?;
        self.validate_state(&state)?;
        Ok(state)
    }

    pub fn active_executable(&self) -> Result<Option<PathBuf>> {
        let state = self.state()?;
        let Some(active_id) = state.active_id.as_deref() else {
            return Ok(None);
        };
        let record = record_by_id(&state, active_id)?;
        let executable = self.executable_path(&record.id);
        self.verify_record(record, &executable)?;
        Ok(Some(executable))
    }

    pub fn install_available_and_activate(
        &self,
        available: &AvailableRuntime,
    ) -> Result<RuntimeDoctor> {
        let (manifest, download) = download_available(available, &self.download_root())?;
        self.install_verified_artifact_and_activate(&manifest, &download)
    }

    pub fn install_verified_artifact_and_activate(
        &self,
        manifest: &RuntimeManifest,
        artifact: &Path,
    ) -> Result<RuntimeDoctor> {
        manifest.validate()?;
        let installed = match install_supported_artifact(manifest, artifact, &self.runtime_root()) {
            Ok(installed) => installed,
            Err(simc_adapter::Error::AlreadyInstalled(directory)) => {
                self.recover_complete_install(manifest, &directory)?
            }
            Err(error) => return Err(error.into()),
        };
        let id = runtime_id(&manifest.simc_version, &manifest.build)?;
        self.register_installed(id.clone(), manifest.build.clone(), installed)?;
        self.activate(&id)?;
        self.doctor(&id)
    }

    fn recover_complete_install(
        &self,
        manifest: &RuntimeManifest,
        directory: &Path,
    ) -> Result<InstalledRuntime> {
        let expected = self
            .runtime_root()
            .join(runtime_id(&manifest.simc_version, &manifest.build)?);
        if directory != expected {
            return Err(Error::Integrity(
                "existing runtime directory escaped the managed root".into(),
            ));
        }
        let directory_metadata = fs::symlink_metadata(directory)?;
        if !directory_metadata.file_type().is_dir() || directory_metadata.file_type().is_symlink() {
            return Err(Error::Integrity(
                "existing runtime directory must be a real directory".into(),
            ));
        }
        let metadata_path = directory.join("runtime.json");
        let file_metadata = fs::symlink_metadata(&metadata_path)?;
        if !file_metadata.file_type().is_file()
            || file_metadata.file_type().is_symlink()
            || file_metadata.len() == 0
            || file_metadata.len() > MAX_RUNTIME_METADATA_BYTES
        {
            return Err(Error::Integrity(
                "existing runtime metadata is not a bounded regular file".into(),
            ));
        }
        let metadata: InstalledRuntimeMetadata =
            serde_json::from_slice(&fs::read(&metadata_path)?)?;
        if metadata.manifest != *manifest {
            return Err(Error::Integrity(
                "existing runtime manifest differs from the requested release".into(),
            ));
        }
        let executable = directory.join(supported_executable_name());
        self.verify_hash(&executable, &metadata.executable_sha256)?;
        let identity = validate_supported_binary(&executable)?;
        if identity != metadata.identity
            || identity.simc_version != manifest.simc_version
            || identity.channel != manifest.game_channel
        {
            return Err(Error::Integrity(
                "existing runtime identity differs from its verified metadata".into(),
            ));
        }
        Ok(InstalledRuntime {
            directory: directory.to_owned(),
            executable,
            executable_sha256: metadata.executable_sha256,
            identity,
        })
    }

    pub fn activate(&self, id: &str) -> Result<()> {
        self.activate_inner(id, true)
    }

    fn activate_inner(&self, id: &str, validate_identity: bool) -> Result<()> {
        validate_id(id)?;
        let mut state = self.state()?;
        let record = record_by_id(&state, id)?;
        self.verify_record(record, &self.executable_path(id))?;
        if validate_identity {
            let _ = self.validated_identity(record, &self.executable_path(id))?;
        }
        if state.active_id.as_deref() == Some(id) {
            return Ok(());
        }
        state.previous_id = state.active_id.take();
        state.active_id = Some(id.to_owned());
        self.write_state(&state)
    }

    pub fn rollback(&self) -> Result<RuntimeDoctor> {
        let previous = self.rollback_inner(true)?;
        self.doctor(&previous)
    }

    fn rollback_inner(&self, validate_identity: bool) -> Result<String> {
        let mut state = self.state()?;
        let previous = state
            .previous_id
            .clone()
            .ok_or(Error::RollbackUnavailable)?;
        let record = record_by_id(&state, &previous)?;
        self.verify_record(record, &self.executable_path(&previous))?;
        if validate_identity {
            let _ = self.validated_identity(record, &self.executable_path(&previous))?;
        }
        let active = state.active_id.replace(previous.clone());
        state.previous_id = active;
        self.write_state(&state)?;
        Ok(previous)
    }

    pub fn doctor_active(&self) -> Result<Option<RuntimeDoctor>> {
        let state = self.state()?;
        state
            .active_id
            .as_deref()
            .map(|id| self.doctor(id))
            .transpose()
    }

    pub fn doctor(&self, id: &str) -> Result<RuntimeDoctor> {
        validate_id(id)?;
        let state = self.state()?;
        let record = record_by_id(&state, id)?.clone();
        let executable = self.executable_path(id);
        self.verify_record(&record, &executable)?;
        let identity = self.validated_identity(&record, &executable)?;
        Ok(RuntimeDoctor {
            record,
            executable,
            identity,
            healthy: true,
        })
    }

    fn register_installed(
        &self,
        id: String,
        build: String,
        installed: InstalledRuntime,
    ) -> Result<()> {
        validate_id(&id)?;
        if installed.directory != self.runtime_root().join(&id)
            || installed.executable != installed.directory.join(supported_executable_name())
        {
            return Err(Error::Integrity(
                "installed runtime escaped the managed root".into(),
            ));
        }
        self.verify_hash(&installed.executable, &installed.executable_sha256)?;
        let record = RuntimeRecord {
            id,
            simc_version: installed.identity.simc_version.clone(),
            build,
            game_version: installed.identity.game_version.clone(),
            channel: installed.identity.channel.clone(),
            executable_sha256: installed.executable_sha256,
            installed_at_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        if record.channel != "live" {
            return Err(Error::Integrity("runtime is not Retail Live".into()));
        }
        let mut state = self.state()?;
        if let Some(existing) = state.runtimes.iter().find(|item| item.id == record.id) {
            if !same_runtime_identity(existing, &record) {
                return Err(Error::InvalidRegistry(
                    "runtime identity collides with an existing record".into(),
                ));
            }
            return Ok(());
        }
        state.runtimes.push(record);
        state.runtimes.sort_by(|left, right| left.id.cmp(&right.id));
        self.write_state(&state)
    }

    fn verify_record(&self, record: &RuntimeRecord, executable: &Path) -> Result<()> {
        if record.channel != "live" {
            return Err(Error::Integrity(
                "registry contains a non-Live runtime".into(),
            ));
        }
        self.verify_hash(executable, &record.executable_sha256)
    }

    fn validated_identity(
        &self,
        record: &RuntimeRecord,
        executable: &Path,
    ) -> Result<SimcIdentity> {
        let identity = validate_supported_binary(executable)?;
        if identity.simc_version != record.simc_version
            || identity.game_version != record.game_version
            || identity.channel != record.channel
        {
            return Err(Error::Integrity(
                "executable identity differs from registry".into(),
            ));
        }
        Ok(identity)
    }

    fn verify_hash(&self, executable: &Path, expected: &str) -> Result<()> {
        let metadata = fs::symlink_metadata(executable)?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(Error::Integrity(
                "runtime executable must be a regular file".into(),
            ));
        }
        let actual = sha256_file(executable)?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(Error::Integrity(format!(
                "executable SHA-256 mismatch: expected {expected}, got {actual}"
            )));
        }
        Ok(())
    }

    fn validate_state(&self, state: &RuntimeState) -> Result<()> {
        if state.schema_version != STATE_SCHEMA_VERSION {
            return Err(Error::InvalidRegistry(
                "unsupported state schema version".into(),
            ));
        }
        let mut previous = None;
        for runtime in &state.runtimes {
            validate_id(&runtime.id)?;
            if runtime.channel != "live" || runtime.executable_sha256.len() != 64 {
                return Err(Error::InvalidRegistry(
                    "runtime record has an invalid channel or digest".into(),
                ));
            }
            if previous.is_some_and(|value: &str| value >= runtime.id.as_str()) {
                return Err(Error::InvalidRegistry(
                    "runtime records must be unique and sorted".into(),
                ));
            }
            previous = Some(runtime.id.as_str());
        }
        for id in [state.active_id.as_deref(), state.previous_id.as_deref()]
            .into_iter()
            .flatten()
        {
            validate_id(id)?;
            let _ = record_by_id(state, id)?;
        }
        Ok(())
    }

    fn write_state(&self, state: &RuntimeState) -> Result<()> {
        self.validate_state(state)?;
        let mut bytes = serde_json::to_vec_pretty(state)?;
        bytes.push(b'\n');
        let mut destination = AtomicWriteFile::open(self.state_path())?;
        #[cfg(unix)]
        destination
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        destination.write_all(&bytes)?;
        destination.commit()?;
        Ok(())
    }

    fn ensure_layout(&self) -> Result<()> {
        for path in [&self.root, &self.runtime_root(), &self.download_root()] {
            fs::create_dir_all(path)?;
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
                return Err(Error::InvalidRegistry(
                    "managed runtime paths must be real directories".into(),
                ));
            }
            #[cfg(unix)]
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("runtime-state.json")
    }

    fn runtime_root(&self) -> PathBuf {
        self.root.join("runtimes")
    }

    fn download_root(&self) -> PathBuf {
        self.root.join("downloads")
    }

    fn executable_path(&self, id: &str) -> PathBuf {
        self.runtime_root()
            .join(id)
            .join(supported_executable_name())
    }
}

fn same_runtime_identity(left: &RuntimeRecord, right: &RuntimeRecord) -> bool {
    left.id == right.id
        && left.simc_version == right.simc_version
        && left.build == right.build
        && left.game_version == right.game_version
        && left.channel == right.channel
        && left.executable_sha256 == right.executable_sha256
}

fn runtime_id(version: &str, build: &str) -> Result<String> {
    let id = format!("{version}-{build}");
    validate_id(&id)?;
    Ok(id)
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(Error::InvalidRegistry("runtime ID is unsafe".into()));
    }
    Ok(())
}

fn record_by_id<'a>(state: &'a RuntimeState, id: &str) -> Result<&'a RuntimeRecord> {
    state
        .runtimes
        .iter()
        .find(|runtime| runtime.id == id)
        .ok_or_else(|| Error::NotInstalled(id.to_owned()))
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read};

    use sha2::{Digest, Sha256};

    use super::*;

    fn fake_install(manager: &RuntimeManager, id: &str, marker: &[u8]) {
        let directory = manager.runtime_root().join(id);
        fs::create_dir_all(&directory).unwrap();
        let executable = directory.join(supported_executable_name());
        fs::write(&executable, marker).unwrap();
        let digest = format!("{:x}", Sha256::digest(marker));
        let (version, build) = id.rsplit_once('-').unwrap();
        manager
            .register_installed(
                id.into(),
                build.into(),
                InstalledRuntime {
                    directory,
                    executable,
                    executable_sha256: digest,
                    identity: SimcIdentity {
                        simc_version: version.into(),
                        game_version: "12.1.0.69465".into(),
                        channel: "live".into(),
                        hotfix: None,
                    },
                },
            )
            .unwrap();
    }

    #[test]
    fn activation_and_rollback_are_atomic_and_integrity_checked() {
        let temporary = tempfile::tempdir().unwrap();
        let manager = RuntimeManager::open(temporary.path().join("managed")).unwrap();
        fake_install(&manager, "1210-01-old", b"old runtime");
        fake_install(&manager, "1210-01-new", b"new runtime");

        manager.activate_inner("1210-01-old", false).unwrap();
        manager.activate_inner("1210-01-new", false).unwrap();
        assert_eq!(
            manager.active_executable().unwrap().unwrap(),
            manager.executable_path("1210-01-new")
        );

        assert_eq!(manager.rollback_inner(false).unwrap(), "1210-01-old");
        let state = manager.state().unwrap();
        assert_eq!(state.active_id.as_deref(), Some("1210-01-old"));
        assert_eq!(state.previous_id.as_deref(), Some("1210-01-new"));

        fs::write(manager.executable_path("1210-01-new"), b"corrupt").unwrap();
        let before = manager.state().unwrap();
        assert!(matches!(
            manager.activate_inner("1210-01-new", false),
            Err(Error::Integrity(_))
        ));
        assert_eq!(manager.state().unwrap(), before);
    }

    #[test]
    fn runtime_identity_ignores_only_the_original_install_timestamp() {
        let first = RuntimeRecord {
            id: "1210-01-build".into(),
            simc_version: "1210-01".into(),
            build: "build".into(),
            game_version: "12.1.0.69465".into(),
            channel: "live".into(),
            executable_sha256: "a".repeat(64),
            installed_at_unix_seconds: 1,
        };
        let mut retried = first.clone();
        retried.installed_at_unix_seconds = 2;
        assert!(same_runtime_identity(&first, &retried));
        retried.executable_sha256 = "b".repeat(64);
        assert!(!same_runtime_identity(&first, &retried));
    }

    #[cfg(unix)]
    #[test]
    fn complete_install_recovery_rejects_a_symlinked_runtime_directory_without_state_change() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let manager = RuntimeManager::open(temporary.path().join("managed")).unwrap();
        fake_install(&manager, "1210-01-old", b"old runtime");
        manager.activate_inner("1210-01-old", false).unwrap();
        let before = manager.state().unwrap();

        let outside = temporary.path().join("outside-runtime");
        fs::create_dir(&outside).unwrap();
        let requested = RuntimeManifest {
            schema_version: 1,
            simc_version: "1210-01".into(),
            game_channel: "live".into(),
            platform: "macos".into(),
            architecture: "aarch64".into(),
            build: "new".into(),
            filename: "simc-1210-01-macos-new.dmg".into(),
            url: "https://downloads.simulationcraft.org/nightly/simc-1210-01-macos-new.dmg".into(),
            size: 1,
            sha256: "0".repeat(64),
        };
        let managed_link = manager.runtime_root().join("1210-01-new");
        symlink(&outside, &managed_link).unwrap();

        assert!(matches!(
            manager.recover_complete_install(&requested, &managed_link),
            Err(Error::Integrity(message)) if message.contains("real directory")
        ));
        assert_eq!(manager.state().unwrap(), before);
        assert!(
            fs::symlink_metadata(&managed_link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn managed_layout_replacement_with_a_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let manager = RuntimeManager::open(temporary.path().join("managed")).unwrap();
        let outside = temporary.path().join("outside-downloads");
        fs::create_dir(&outside).unwrap();
        fs::remove_dir(manager.download_root()).unwrap();
        symlink(&outside, manager.download_root()).unwrap();

        let error = manager.state().unwrap_err();
        assert!(matches!(error, Error::InvalidRegistry(_)));
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
    }

    #[test]
    fn rejects_unsafe_registry_and_bounds_permissions() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("managed");
        let manager = RuntimeManager::open(&root).unwrap();
        assert!(matches!(
            manager.activate("../escape"),
            Err(Error::InvalidRegistry(_))
        ));

        fs::write(
            manager.state_path(),
            br#"{"schema_version":1,"active_id":"missing","previous_id":null,"runtimes":[]}"#,
        )
        .unwrap();
        assert!(matches!(manager.state(), Err(Error::NotInstalled(_))));

        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn state_file_is_complete_json_and_mode_0600() {
        let temporary = tempfile::tempdir().unwrap();
        let manager = RuntimeManager::open(temporary.path().join("managed")).unwrap();
        fake_install(&manager, "1210-01-build", b"runtime");
        manager.activate_inner("1210-01-build", false).unwrap();

        let mut bytes = Vec::new();
        File::open(manager.state_path())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let parsed: RuntimeState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.active_id.as_deref(), Some("1210-01-build"));
        assert!(!manager.root.join(".runtime-state.tmp").exists());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(manager.state_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
