use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

#[cfg(windows)]
use std::{collections::BTreeSet, io::Write, time::Duration};

#[cfg(any(windows, test))]
use crate::sha256_file;
use crate::{Error, Result, RuntimeManifest, verify_artifact};
#[cfg(windows)]
use crate::{InstalledRuntime, SimcIdentity, parse_identity, run_with_timeout};

const MAX_SIMC_BYTES: u64 = 256 * 1024 * 1024;
const MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 512;
const MAX_TOTAL_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;
const PE_HEADER_READ_BYTES: u64 = 64 * 1024;

pub fn validate_windows_pe_x64(executable: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(executable)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_SIMC_BYTES
    {
        return Err(Error::Contract(
            "simc.exe must be a bounded regular file".into(),
        ));
    }
    let mut header = Vec::new();
    File::open(executable)?
        .take(PE_HEADER_READ_BYTES)
        .read_to_end(&mut header)?;
    if header.get(..2) != Some(b"MZ") || header.len() < 0x40 {
        return Err(Error::Contract("simc.exe has no DOS header".into()));
    }
    let pe_offset = u32::from_le_bytes(
        header[0x3c..0x40]
            .try_into()
            .map_err(|_| Error::Contract("simc.exe PE offset is truncated".into()))?,
    ) as usize;
    let pe_end = pe_offset
        .checked_add(26)
        .ok_or_else(|| Error::Contract("simc.exe PE offset overflowed".into()))?;
    if pe_end > header.len() || header.get(pe_offset..pe_offset + 4) != Some(b"PE\0\0") {
        return Err(Error::Contract(
            "simc.exe has no bounded PE signature".into(),
        ));
    }
    let machine = u16::from_le_bytes([header[pe_offset + 4], header[pe_offset + 5]]);
    let optional_magic = u16::from_le_bytes([header[pe_offset + 24], header[pe_offset + 25]]);
    if machine != 0x8664 || optional_magic != 0x020b {
        return Err(Error::Contract(
            "simc.exe is not an x86-64 PE32+ executable".into(),
        ));
    }
    Ok(())
}

pub fn extract_windows_archive(
    manifest: &RuntimeManifest,
    archive: &Path,
    destination: &Path,
) -> Result<PathBuf> {
    if manifest.platform != "windows" || manifest.architecture != "x86_64" {
        return Err(Error::UnsafeArchive(
            "manifest is not for Windows x64".into(),
        ));
    }
    verify_artifact(manifest, archive)?;
    prepare_empty_destination(destination)?;
    let expected_root = format!(
        "simc-{}.{}-win64",
        manifest.simc_version.replace('-', "."),
        manifest.build
    );
    let mut entry_count = 0_usize;
    let mut total_unpacked = 0_u64;
    let mut extracted = HashSet::new();
    sevenz_rust2::decompress_file_with_extract_fn(
        archive,
        destination,
        |entry, reader, _validated_path| {
            entry_count += 1;
            if entry_count > MAX_ARCHIVE_ENTRIES {
                return Err(sevenz_rust2::Error::Other(
                    "archive contains too many entries".into(),
                ));
            }
            total_unpacked = total_unpacked
                .checked_add(entry.size())
                .ok_or_else(|| sevenz_rust2::Error::Other("archive size overflowed".into()))?;
            if total_unpacked > MAX_TOTAL_UNPACKED_BYTES {
                return Err(sevenz_rust2::Error::Other(
                    "archive unpacked size exceeds 1 GiB".into(),
                ));
            }
            let selected = selected_entry(entry.name(), &expected_root);
            if let Some(name) = selected {
                if entry.is_directory() || !entry.has_stream() || !extracted.insert(name.clone()) {
                    return Err(sevenz_rust2::Error::Other(
                        "required archive entry is duplicated or not a regular file".into(),
                    ));
                }
                let maximum = if name == "simc.exe" {
                    MAX_SIMC_BYTES
                } else {
                    MAX_DOCUMENT_BYTES
                };
                if entry.size() == 0 || entry.size() > maximum {
                    return Err(sevenz_rust2::Error::Other(
                        "selected archive entry size is out of range".into(),
                    ));
                }
                let path = destination.join(&name);
                let mut file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&path)?;
                let copied = std::io::copy(reader, &mut file)?;
                if copied != entry.size() {
                    return Err(sevenz_rust2::Error::Other(
                        "selected archive entry was truncated".into(),
                    ));
                }
                file.sync_all()?;
            } else {
                std::io::copy(reader, &mut std::io::sink())?;
            }
            Ok(true)
        },
    )
    .map_err(|error| Error::UnsafeArchive(error.to_string()))?;

    for required in ["simc.exe", "COPYING", "README.md"] {
        if !extracted.contains(required) {
            return Err(Error::UnsafeArchive(format!(
                "archive is missing required entry {required}"
            )));
        }
    }
    let executable = destination.join("simc.exe");
    validate_windows_pe_x64(&executable)?;
    Ok(executable)
}

fn prepare_empty_destination(destination: &Path) -> Result<()> {
    if destination.exists() {
        let metadata = fs::symlink_metadata(destination)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::UnsafeArchive(
                "archive destination must be a real directory".into(),
            ));
        }
        if fs::read_dir(destination)?.next().is_some() {
            return Err(Error::UnsafeArchive(
                "archive destination must be empty".into(),
            ));
        }
    } else {
        fs::create_dir_all(destination)?;
    }
    Ok(())
}

fn selected_entry(entry_name: &str, expected_root: &str) -> Option<String> {
    let normalized = entry_name.replace('\\', "/");
    let mut parts = normalized.split('/');
    if parts.next()? != expected_root {
        return None;
    }
    let name = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    match name {
        "simc.exe" | "COPYING" | "README.md" | "LICENSE" => Some(name.to_owned()),
        value
            if value.starts_with("LICENSE.")
                && value.len() <= 48
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-') =>
        {
            Some(value.to_owned())
        }
        _ => None,
    }
}

#[cfg(windows)]
pub fn discover_windows_executables(
    explicit_candidates: &[PathBuf],
    managed_root: &Path,
) -> Result<Vec<PathBuf>> {
    let mut candidates = BTreeSet::new();
    for candidate in explicit_candidates {
        if is_regular_windows_candidate(candidate)? {
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
            if entry.file_type()?.is_dir() {
                let candidate = entry.path().join("simc.exe");
                if is_regular_windows_candidate(&candidate)? {
                    candidates.insert(candidate);
                }
            }
        }
    }
    Ok(candidates.into_iter().collect())
}

#[cfg(windows)]
fn is_regular_windows_candidate(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() > 0
            && metadata.len() <= MAX_SIMC_BYTES),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(windows)]
pub fn validate_windows_binary(executable: &Path) -> Result<SimcIdentity> {
    validate_windows_pe_x64(executable)?;
    let parent = executable
        .parent()
        .ok_or_else(|| Error::Contract("simc.exe has no parent directory".into()))?;
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
    let identity = parse_identity(&format!("{}\n{}", output.stdout, output.stderr))?;
    if identity.channel != "live" {
        return Err(Error::Contract(format!(
            "unsupported game channel: {}",
            identity.channel
        )));
    }
    Ok(identity)
}

#[cfg(windows)]
pub fn install_windows_archive(
    manifest: &RuntimeManifest,
    archive: &Path,
    install_root: &Path,
) -> Result<InstalledRuntime> {
    fs::create_dir_all(install_root)?;
    let final_directory =
        install_root.join(format!("{}-{}", manifest.simc_version, manifest.build));
    if final_directory.exists() {
        return Err(Error::AlreadyInstalled(final_directory));
    }
    let staging = tempfile::Builder::new()
        .prefix(".simc-install-")
        .tempdir_in(install_root)?;
    let executable = extract_windows_archive(manifest, archive, staging.path())?;
    let identity = validate_windows_binary(&executable)?;
    if identity.simc_version != manifest.simc_version {
        return Err(Error::Contract(format!(
            "manifest version {} differs from executable {}",
            manifest.simc_version, identity.simc_version
        )));
    }
    let executable_sha256 = sha256_file(&executable)?;
    let mut metadata = serde_json::to_vec_pretty(&serde_json::json!({
        "manifest": manifest,
        "identity": identity,
        "executable_sha256": executable_sha256,
    }))?;
    metadata.push(b'\n');
    let mut metadata_file = File::create(staging.path().join("runtime.json"))?;
    metadata_file.write_all(&metadata)?;
    metadata_file.sync_all()?;
    drop(metadata_file);

    let staging_path = staging.keep();
    fs::rename(&staging_path, &final_directory)?;
    Ok(InstalledRuntime {
        executable: final_directory.join("simc.exe"),
        directory: final_directory,
        executable_sha256,
        identity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_flat_files_under_the_exact_official_root() {
        let root = "simc-1210.01.3487fce-win64";
        assert_eq!(
            selected_entry("simc-1210.01.3487fce-win64/simc.exe", root),
            Some("simc.exe".into())
        );
        assert_eq!(
            selected_entry("simc-1210.01.3487fce-win64/LICENSE.MIT", root),
            Some("LICENSE.MIT".into())
        );
        assert_eq!(selected_entry("other/simc.exe", root), None);
        assert_eq!(
            selected_entry("simc-1210.01.3487fce-win64/nested/simc.exe", root),
            None
        );
    }

    #[test]
    fn validates_an_x64_pe32_plus_header_without_executing_it() {
        let temporary = tempfile::NamedTempFile::new().unwrap();
        let mut bytes = vec![0_u8; 512];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0x8664_u16.to_le_bytes());
        bytes[0x98..0x9a].copy_from_slice(&0x020b_u16.to_le_bytes());
        fs::write(temporary.path(), bytes).unwrap();
        validate_windows_pe_x64(temporary.path()).unwrap();
    }

    #[test]
    #[ignore = "extracts the actual pinned 120 MB official Windows archive"]
    fn extracts_the_pinned_official_windows_archive() {
        let archive = std::env::var_os("SIMSHREDDER_WINDOWS_ARCHIVE")
            .map(PathBuf::from)
            .expect("SIMSHREDDER_WINDOWS_ARCHIVE must be set");
        let manifest: RuntimeManifest = serde_json::from_slice(include_bytes!(
            "../../../../test-data/fixtures/runtime/simc-1210-01-windows-3487fce.manifest.json"
        ))
        .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let executable = extract_windows_archive(&manifest, &archive, temporary.path()).unwrap();
        assert_eq!(
            sha256_file(&executable).unwrap(),
            "df3dee3c652ba5cf28032f42d0cc344e2c22fc9e4fd60f64c2f28d6c15b62745"
        );
        assert!(temporary.path().join("COPYING").is_file());
        assert!(temporary.path().join("README.md").is_file());
        assert!(!temporary.path().join("SimulationCraft.exe").exists());
    }
}
