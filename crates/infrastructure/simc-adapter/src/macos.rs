use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use crate::{
    Error, InstalledRuntime, Result, RuntimeManifest, SimcIdentity, parse_identity,
    run_with_timeout, verify_artifact,
};

const MAX_SIMC_BYTES: u64 = 256 * 1024 * 1024;

/// Discovers only caller-selected executables and immediate managed runtime children.
/// It never scans the whole filesystem or trusts PATH.
pub fn discover_macos_executables(
    explicit_candidates: &[PathBuf],
    managed_root: &Path,
) -> Result<Vec<PathBuf>> {
    let mut candidates = BTreeSet::new();
    for candidate in explicit_candidates {
        if is_regular_executable_candidate(candidate)? {
            candidates.insert(candidate.to_owned());
        }
    }
    if managed_root.exists() {
        let metadata = fs::symlink_metadata(managed_root)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Contract(
                "managed runtime root must be a real directory".into(),
            ));
        }
        for entry in fs::read_dir(managed_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let candidate = entry.path().join("simc");
            if is_regular_executable_candidate(&candidate)? {
                candidates.insert(candidate);
            }
        }
    }
    Ok(candidates.into_iter().collect())
}

fn is_regular_executable_candidate(path: &Path) -> Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    Ok(metadata.file_type().is_file()
        && !metadata.file_type().is_symlink()
        && metadata.len() > 0
        && metadata.len() <= MAX_SIMC_BYTES
        && metadata.permissions().mode() & 0o111 != 0)
}

struct MountedDmg {
    directory: tempfile::TempDir,
}

impl MountedDmg {
    fn attach(dmg: &Path) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("simshredder-dmg-")
            .tempdir_in("/private/tmp")?;
        let output = Command::new("/usr/bin/hdiutil")
            .args([
                "attach",
                "-readonly",
                "-nobrowse",
                "-noautoopen",
                "-mountpoint",
            ])
            .arg(directory.path())
            .arg(dmg)
            .output()?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                program: "hdiutil attach".into(),
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(Self { directory })
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }
}

impl Drop for MountedDmg {
    fn drop(&mut self) {
        let _ = Command::new("/usr/bin/hdiutil")
            .arg("detach")
            .arg(self.directory.path())
            .output();
    }
}

pub fn validate_macos_binary(executable: &Path) -> Result<SimcIdentity> {
    let metadata = fs::symlink_metadata(executable)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_SIMC_BYTES
    {
        return Err(Error::Contract(
            "simc must be a bounded regular file".into(),
        ));
    }
    let architecture = Command::new("/usr/bin/lipo")
        .arg(executable)
        .args(["-verify_arch", "arm64"])
        .output()?;
    if !architecture.status.success() {
        return Err(Error::Contract("simc has no ARM64 Mach-O slice".into()));
    }
    let parent = executable
        .parent()
        .ok_or_else(|| Error::Contract("simc has no parent directory".into()))?;
    let output = run_with_timeout(
        executable,
        std::iter::empty::<&str>(),
        parent,
        Duration::from_secs(10),
    )?;
    if !output.status.success() {
        return Err(Error::Contract(format!(
            "identity probe exited with {}: {}",
            output.status,
            output.stderr.trim()
        )));
    }
    let combined = format!("{}\n{}", output.stdout, output.stderr);
    let identity = parse_identity(&combined)?;
    if identity.channel != "live" {
        return Err(Error::Contract(format!(
            "unsupported game channel: {}",
            identity.channel
        )));
    }
    Ok(identity)
}

pub fn install_macos_dmg(
    manifest: &RuntimeManifest,
    dmg: &Path,
    install_root: &Path,
) -> Result<InstalledRuntime> {
    verify_artifact(manifest, dmg)?;
    fs::create_dir_all(install_root)?;
    let final_directory =
        install_root.join(format!("{}-{}", manifest.simc_version, manifest.build));
    if final_directory.exists() {
        return Err(Error::AlreadyInstalled(final_directory));
    }

    let mounted = MountedDmg::attach(dmg)?;
    let source = mounted.path().join("simc");
    let source_metadata = fs::symlink_metadata(&source)?;
    if !source_metadata.file_type().is_file()
        || source_metadata.file_type().is_symlink()
        || source_metadata.len() == 0
        || source_metadata.len() > MAX_SIMC_BYTES
    {
        return Err(Error::UnsafeDmg(source));
    }

    let staging = tempfile::Builder::new()
        .prefix(".simc-install-")
        .tempdir_in(install_root)?;
    let executable = staging.path().join("simc");
    fs::copy(&source, &executable)?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;

    // The official standalone CLI is ad-hoc signed. macOS propagates the browser quarantine
    // from the verified DMG and otherwise kills it before main(). Remove only this attribute,
    // and only after the DMG's pinned size and SHA-256 have been verified above.
    let _ = Command::new("/usr/bin/xattr")
        .args(["-d", "com.apple.quarantine"])
        .arg(&executable)
        .output();

    for name in ["COPYING", "README.md"] {
        let candidate = mounted.path().join(name);
        if candidate.is_file() {
            fs::copy(candidate, staging.path().join(name))?;
        }
    }
    for entry in fs::read_dir(mounted.path())? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("LICENSE.") && entry.file_type()?.is_file() {
            fs::copy(entry.path(), staging.path().join(name))?;
        }
    }

    let identity = validate_macos_binary(&executable)?;
    if identity.simc_version != manifest.simc_version {
        return Err(Error::Contract(format!(
            "manifest version {} differs from executable {}",
            manifest.simc_version, identity.simc_version
        )));
    }
    let executable_sha256 = crate::sha256_file(&executable)?;
    let metadata = serde_json::to_vec_pretty(&serde_json::json!({
        "manifest": manifest,
        "identity": identity,
        "executable_sha256": executable_sha256,
    }))?;
    let mut metadata_file = File::create(staging.path().join("runtime.json"))?;
    metadata_file.write_all(&metadata)?;
    metadata_file.write_all(b"\n")?;
    metadata_file.sync_all()?;
    drop(metadata_file);

    let staging_path = staging.keep();
    fs::rename(&staging_path, &final_directory)?;
    let executable = final_directory.join("simc");
    Ok(InstalledRuntime {
        directory: final_directory,
        executable,
        executable_sha256,
        identity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_is_sorted_bounded_and_does_not_follow_symlinks() {
        let temporary = tempfile::tempdir().unwrap();
        let managed = temporary.path().join("managed");
        fs::create_dir_all(managed.join("b")).unwrap();
        fs::create_dir_all(managed.join("a")).unwrap();
        for path in [managed.join("b/simc"), managed.join("a/simc")] {
            fs::write(&path, b"binary").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::os::unix::fs::symlink(managed.join("a/simc"), temporary.path().join("linked-simc"))
            .unwrap();
        let missing = temporary.path().join("missing");
        let found =
            discover_macos_executables(&[missing, temporary.path().join("linked-simc")], &managed)
                .unwrap();
        assert_eq!(found, vec![managed.join("a/simc"), managed.join("b/simc")]);
    }
}
