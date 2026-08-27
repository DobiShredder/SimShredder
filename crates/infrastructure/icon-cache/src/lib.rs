//! Provider-neutral, bounded WoW icon cache. Remote providers are intentionally absent.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use image::{GenericImageView, ImageFormat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const DEFAULT_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_RASTER_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RASTER_DIMENSION: u32 = 2048;
const NEGATIVE_CACHE_SECONDS: u64 = 15 * 60;
const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum Error {
    #[error("icon cache I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("icon cache index is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("icon cache layout is invalid: {0}")]
    InvalidCache(String),
    #[error("icon raster was rejected: {0}")]
    InvalidRaster(String),
    #[error("system clock is before the Unix epoch")]
    Clock,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IconKind {
    Item,
    Spell,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct IconSubject {
    pub game_build: u32,
    pub kind: IconKind,
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticIcon {
    Armor,
    Buff,
    Spell,
    Sword,
    Talent,
    Trinket,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IconResolution {
    CachedRaster { path: PathBuf, mime: String },
    Placeholder(SemanticIcon),
}

#[derive(Clone, Debug)]
pub struct ProviderImage {
    pub provider: String,
    pub icon_name: String,
    pub source_url: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub fn read_validated_raster(path: &Path, expected_hash: &str, extension: &str) -> Result<Vec<u8>> {
    if expected_hash.len() != 64
        || !expected_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(Error::InvalidRaster("content hash is malformed".into()));
    }
    let format = match extension {
        "png" => ImageFormat::Png,
        "jpg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::WebP,
        _ => return Err(Error::InvalidRaster("file extension is unsupported".into())),
    };
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_RASTER_BYTES as u64
    {
        return Err(Error::InvalidRaster(
            "blob is not a bounded regular file".into(),
        ));
    }
    let bytes = fs::read(path)?;
    if bytes.len() as u64 != metadata.len() || hex_digest(&bytes) != expected_hash {
        return Err(Error::InvalidRaster(
            "blob size or content digest changed".into(),
        ));
    }
    let guessed = image::guess_format(&bytes)
        .map_err(|error| Error::InvalidRaster(format!("unknown image encoding: {error}")))?;
    if guessed != format {
        return Err(Error::InvalidRaster(
            "extension and encoded format do not match".into(),
        ));
    }
    let decoded = image::load_from_memory_with_format(&bytes, format)
        .map_err(|error| Error::InvalidRaster(format!("decode failed: {error}")))?;
    let (width, height) = decoded.dimensions();
    if width == 0 || height == 0 || width > MAX_RASTER_DIMENSION || height > MAX_RASTER_DIMENSION {
        return Err(Error::InvalidRaster(
            "decoded dimensions exceed the safety limit".into(),
        ));
    }
    Ok(bytes)
}

pub trait IconProvider {
    fn fetch(&self, subject: &IconSubject) -> Result<Option<ProviderImage>>;
}

/// Safe MVP provider: never performs network I/O or transmits profile data.
pub struct OfflineProvider;

impl IconProvider for OfflineProvider {
    fn fetch(&self, _subject: &IconSubject) -> Result<Option<ProviderImage>> {
        Ok(None)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Mapping {
    content_hash: String,
    mime: String,
    provider: String,
    icon_name: String,
    source_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BlobRecord {
    extension: String,
    size: u64,
    width: u32,
    height: u32,
    last_access_unix_seconds: u64,
}

#[derive(Default, Deserialize, Serialize)]
struct Index {
    mappings: BTreeMap<String, Mapping>,
    blobs: BTreeMap<String, BlobRecord>,
    negative_until: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStatus {
    pub budget_bytes: u64,
    pub used_bytes: u64,
    pub icon_count: usize,
    pub mapping_count: usize,
    pub remote_provider_enabled: bool,
}

pub struct IconCache {
    root: PathBuf,
    budget_bytes: u64,
    index: Index,
}

impl IconCache {
    pub fn open(root: PathBuf) -> Result<Self> {
        Self::open_with_budget(root, DEFAULT_BUDGET_BYTES)
    }

    pub fn open_with_budget(root: PathBuf, budget_bytes: u64) -> Result<Self> {
        fs::create_dir_all(&root)?;
        ensure_real_directory(&root)?;
        protect_directory(&root)?;
        for child in [root.join("blobs"), root.join("quarantine")] {
            fs::create_dir(&child).or_else(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(error)
                }
            })?;
            ensure_real_directory(&child)?;
            protect_directory(&child)?;
        }
        let index_path = root.join("index.json");
        let index = match fs::symlink_metadata(&index_path) {
            Ok(metadata)
                if metadata.file_type().is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.len() <= MAX_INDEX_BYTES =>
            {
                serde_json::from_slice(&fs::read(index_path)?)?
            }
            Ok(_) => {
                return Err(Error::InvalidCache(
                    "index must be a bounded regular file".into(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Index::default(),
            Err(error) => return Err(error.into()),
        };
        let mut cache = Self {
            root,
            budget_bytes,
            index,
        };
        cache.audit()?;
        cache.trim()?;
        Ok(cache)
    }

    pub fn status(&self) -> CacheStatus {
        CacheStatus {
            budget_bytes: self.budget_bytes,
            used_bytes: self.index.blobs.values().map(|blob| blob.size).sum(),
            icon_count: self.index.blobs.len(),
            mapping_count: self.index.mappings.len(),
            remote_provider_enabled: false,
        }
    }

    pub fn resolve<P: IconProvider>(
        &mut self,
        subject: &IconSubject,
        semantic: SemanticIcon,
        provider: &P,
    ) -> Result<IconResolution> {
        self.ensure_layout()?;
        let key = subject_key(subject);
        let now = now_seconds()?;
        if let Some(mapping) = self.index.mappings.get(&key).cloned()
            && let Some(blob) = self.index.blobs.get_mut(&mapping.content_hash)
        {
            let path = self
                .root
                .join("blobs")
                .join(format!("{}.{}", mapping.content_hash, blob.extension));
            if read_validated_raster(&path, &mapping.content_hash, &blob.extension)
                .is_ok_and(|bytes| bytes.len() as u64 == blob.size)
            {
                blob.last_access_unix_seconds = now;
                self.persist()?;
                return Ok(IconResolution::CachedRaster {
                    path,
                    mime: mapping.mime,
                });
            }
        }
        if self
            .index
            .negative_until
            .get(&key)
            .is_some_and(|until| *until > now)
        {
            return Ok(IconResolution::Placeholder(semantic));
        }
        if let Some(image) = provider.fetch(subject)? {
            return self.store(subject, image);
        }
        self.index
            .negative_until
            .insert(key, now + NEGATIVE_CACHE_SECONDS);
        self.persist()?;
        Ok(IconResolution::Placeholder(semantic))
    }

    pub fn store(&mut self, subject: &IconSubject, image: ProviderImage) -> Result<IconResolution> {
        self.ensure_layout()?;
        let (format, extension) = expected_format(&image.mime)?;
        if image.bytes.is_empty() || image.bytes.len() > MAX_RASTER_BYTES {
            return Err(Error::InvalidRaster(
                "body size is outside the allowed range".into(),
            ));
        }
        let guessed = image::guess_format(&image.bytes)
            .map_err(|error| Error::InvalidRaster(format!("unknown image encoding: {error}")))?;
        if guessed != format {
            return Err(Error::InvalidRaster(
                "MIME and encoded format do not match".into(),
            ));
        }
        let decoded = image::load_from_memory_with_format(&image.bytes, format)
            .map_err(|error| Error::InvalidRaster(format!("decode failed: {error}")))?;
        let (width, height) = decoded.dimensions();
        if width == 0
            || height == 0
            || width > MAX_RASTER_DIMENSION
            || height > MAX_RASTER_DIMENSION
        {
            return Err(Error::InvalidRaster(
                "decoded dimensions exceed the safety limit".into(),
            ));
        }
        let hash = hex_digest(&image.bytes);
        let path = self.root.join("blobs").join(format!("{hash}.{extension}"));
        if !path.exists() {
            atomic_write(&path, &image.bytes)?;
        }
        let now = now_seconds()?;
        self.index.blobs.insert(
            hash.clone(),
            BlobRecord {
                extension: extension.into(),
                size: image.bytes.len() as u64,
                width,
                height,
                last_access_unix_seconds: now,
            },
        );
        self.index.mappings.insert(
            subject_key(subject),
            Mapping {
                content_hash: hash.clone(),
                mime: image.mime.clone(),
                provider: image.provider,
                icon_name: image.icon_name,
                source_url: image.source_url,
            },
        );
        self.index.negative_until.remove(&subject_key(subject));
        self.trim()?;
        self.persist()?;
        Ok(IconResolution::CachedRaster {
            path,
            mime: image.mime,
        })
    }

    pub fn clear(&mut self) -> Result<()> {
        self.ensure_layout()?;
        for entry in fs::read_dir(self.root.join("blobs"))? {
            let path = entry?.path();
            if path.is_file() {
                fs::remove_file(path)?;
            }
        }
        self.index = Index::default();
        self.persist()
    }

    fn audit(&mut self) -> Result<()> {
        self.ensure_layout()?;
        let blobs = self.root.join("blobs");
        self.index.blobs.retain(|hash, record| {
            read_validated_raster(
                &blobs.join(format!("{hash}.{}", record.extension)),
                hash,
                &record.extension,
            )
            .is_ok_and(|bytes| bytes.len() as u64 == record.size)
        });
        self.index
            .mappings
            .retain(|_, mapping| self.index.blobs.contains_key(&mapping.content_hash));
        self.persist()
    }

    fn trim(&mut self) -> Result<()> {
        while self.status().used_bytes > self.budget_bytes {
            let Some((hash, record)) = self
                .index
                .blobs
                .iter()
                .min_by_key(|(_, blob)| blob.last_access_unix_seconds)
                .map(|(hash, blob)| (hash.clone(), blob.clone()))
            else {
                break;
            };
            let path = self
                .root
                .join("blobs")
                .join(format!("{hash}.{}", record.extension));
            if path.exists() {
                fs::remove_file(path)?;
            }
            self.index.blobs.remove(&hash);
            self.index
                .mappings
                .retain(|_, mapping| mapping.content_hash != hash);
        }
        Ok(())
    }

    fn persist(&self) -> Result<()> {
        self.ensure_layout()?;
        atomic_write(
            &self.root.join("index.json"),
            &serde_json::to_vec_pretty(&self.index)?,
        )
    }

    fn ensure_layout(&self) -> Result<()> {
        for path in [
            self.root.as_path(),
            self.root.join("blobs").as_path(),
            self.root.join("quarantine").as_path(),
        ] {
            ensure_real_directory(path)?;
        }
        Ok(())
    }
}

fn ensure_real_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::InvalidCache(format!(
            "{} must be a real directory",
            path.display()
        )));
    }
    Ok(())
}

fn expected_format(mime: &str) -> Result<(ImageFormat, &'static str)> {
    match mime {
        "image/png" => Ok((ImageFormat::Png, "png")),
        "image/jpeg" => Ok((ImageFormat::Jpeg, "jpg")),
        "image/webp" => Ok((ImageFormat::WebP, "webp")),
        _ => Err(Error::InvalidRaster(
            "MIME is not PNG, JPEG, or WebP".into(),
        )),
    }
}

fn subject_key(subject: &IconSubject) -> String {
    format!("{}:{:?}:{}", subject.game_build, subject.kind, subject.id).to_ascii_lowercase()
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn now_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .map_err(|_| Error::Clock)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::InvalidRaster("cache path has no parent".into()))?;
    let mut temporary = tempfile_path(
        parent,
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("icon"),
    );
    let mut suffix = 0_u32;
    while temporary.exists() {
        suffix += 1;
        temporary = tempfile_path(parent, &format!("icon-{suffix}"));
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn tempfile_path(parent: &Path, name: &str) -> PathBuf {
    parent.join(format!(".{name}.staging-{}", std::process::id()))
}

#[cfg(unix)]
fn protect_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn protect_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn png(color: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba(color));
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encode PNG");
        bytes.into_inner()
    }

    fn subject(id: u32) -> IconSubject {
        IconSubject {
            game_build: 69465,
            kind: IconKind::Item,
            id,
        }
    }

    fn provider_image(bytes: Vec<u8>) -> ProviderImage {
        ProviderImage {
            provider: "fixture".into(),
            icon_name: "armor".into(),
            source_url: "https://example.invalid/icon.png".into(),
            mime: "image/png".into(),
            bytes,
        }
    }

    #[test]
    fn offline_resolution_is_a_semantic_placeholder_and_negative_cached() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut cache = IconCache::open(directory.path().to_owned()).expect("cache");
        assert_eq!(
            cache
                .resolve(&subject(1), SemanticIcon::Armor, &OfflineProvider)
                .expect("resolve"),
            IconResolution::Placeholder(SemanticIcon::Armor)
        );
        assert_eq!(cache.status().icon_count, 0);
    }

    #[test]
    fn validates_and_deduplicates_raster_content() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut cache = IconCache::open(directory.path().to_owned()).expect("cache");
        let bytes = png([20, 30, 40, 255]);
        cache
            .store(&subject(1), provider_image(bytes.clone()))
            .expect("first");
        cache
            .store(&subject(2), provider_image(bytes))
            .expect("second");
        assert_eq!(cache.status().icon_count, 1);
        assert_eq!(cache.status().mapping_count, 2);
    }

    #[test]
    fn rejects_mime_mismatch_and_evicts_to_budget() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut cache = IconCache::open_with_budget(directory.path().to_owned(), 1).expect("cache");
        let mut wrong = provider_image(png([1, 2, 3, 255]));
        wrong.mime = "image/jpeg".into();
        assert!(matches!(
            cache.store(&subject(1), wrong),
            Err(Error::InvalidRaster(_))
        ));
        cache
            .store(&subject(2), provider_image(png([4, 5, 6, 255])))
            .expect("store");
        assert_eq!(cache.status().used_bytes, 0);
        assert_eq!(cache.status().mapping_count, 0);
    }

    #[test]
    fn clear_removes_cached_rasters_and_mappings_and_survives_reopen() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("icons");
        let mut cache = IconCache::open(root.clone()).expect("cache");
        cache
            .store(&subject(1), provider_image(png([8, 9, 10, 255])))
            .expect("store");
        assert_eq!(cache.status().icon_count, 1);

        cache.clear().expect("clear");
        assert_eq!(cache.status().icon_count, 0);
        assert_eq!(cache.status().mapping_count, 0);
        assert!(fs::read_dir(root.join("blobs")).unwrap().next().is_none());
        drop(cache);
        assert_eq!(IconCache::open(root).unwrap().status().icon_count, 0);
    }

    #[test]
    fn reopen_drops_a_blob_whose_content_no_longer_matches_its_digest() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().join("icons");
        let mut cache = IconCache::open(root.clone()).expect("cache");
        let path = match cache
            .store(&subject(1), provider_image(png([20, 30, 40, 255])))
            .expect("store")
        {
            IconResolution::CachedRaster { path, .. } => path,
            IconResolution::Placeholder(_) => panic!("stored raster became a placeholder"),
        };
        let size = fs::metadata(&path).unwrap().len() as usize;
        fs::write(&path, vec![0_u8; size]).unwrap();
        drop(cache);

        let reopened = IconCache::open(root).unwrap();
        assert_eq!(reopened.status().icon_count, 0);
        assert_eq!(reopened.status().mapping_count, 0);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_layout_and_index_are_rejected_without_touching_targets() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("icons");
        let outside = temporary.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        symlink(&outside, root.join("blobs")).unwrap();
        let error = match IconCache::open(root.clone()) {
            Ok(_) => panic!("symlinked blob directory was accepted"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::InvalidCache(_)));
        assert!(fs::read_dir(&outside).unwrap().next().is_none());

        fs::remove_file(root.join("blobs")).unwrap();
        fs::create_dir(root.join("blobs")).unwrap();
        fs::create_dir(root.join("quarantine")).unwrap();
        let outside_index = temporary.path().join("outside-index.json");
        fs::write(&outside_index, b"{}").unwrap();
        symlink(&outside_index, root.join("index.json")).unwrap();
        let error = match IconCache::open(root) {
            Ok(_) => panic!("symlinked index was accepted"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::InvalidCache(_)));
        assert_eq!(fs::read(&outside_index).unwrap(), b"{}");
    }
}
