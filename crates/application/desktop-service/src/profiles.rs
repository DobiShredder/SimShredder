use std::{
    fs,
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{DesktopService, Error, QuickSimRequest, Result, SourceFormat, prepare_request};

const PROFILE_CATALOG_SCHEMA: u32 = 1;
const MAX_CHARACTER_PROFILES: usize = 64;
const MAX_PROFILE_CATALOG_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProfileInputSource {
    AddonExport,
    SimcFile,
    Armory,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CharacterIdentity {
    pub region: Option<String>,
    pub realm: Option<String>,
    pub character_name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ProfileInputSnapshot {
    source: ProfileInputSource,
    request: QuickSimRequest,
    captured_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CharacterProfileRecord {
    id: String,
    identity: CharacterIdentity,
    display_name: String,
    class: String,
    specialization: String,
    favorite: bool,
    active_input: ProfileInputSnapshot,
    previous_input: Option<ProfileInputSnapshot>,
    created_at_unix_seconds: u64,
    updated_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CharacterProfileCatalog {
    schema_version: u32,
    profiles: Vec<CharacterProfileRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArmoryRefreshStatus {
    pub available: bool,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterProfileView {
    pub id: String,
    pub identity: CharacterIdentity,
    pub display_name: String,
    pub class: String,
    pub specialization: String,
    pub favorite: bool,
    pub input_source: ProfileInputSource,
    pub request: QuickSimRequest,
    pub captured_at_unix_seconds: u64,
    pub previous_input_available: bool,
    pub armory_refresh: ArmoryRefreshStatus,
}

pub trait ArmoryProfileProvider {
    fn status(&self) -> ArmoryRefreshStatus;
    fn reload(&self, identity: &CharacterIdentity) -> std::result::Result<QuickSimRequest, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DisabledArmoryProvider;

impl ArmoryProfileProvider for DisabledArmoryProvider {
    fn status(&self) -> ArmoryRefreshStatus {
        ArmoryRefreshStatus {
            available: false,
            reason: "Blizzard API authentication and the credential broker are scheduled for 1.0"
                .into(),
        }
    }

    fn reload(
        &self,
        _identity: &CharacterIdentity,
    ) -> std::result::Result<QuickSimRequest, String> {
        Err(self.status().reason)
    }
}

impl DesktopService {
    pub fn character_profiles(&self) -> Result<Vec<CharacterProfileView>> {
        let provider = DisabledArmoryProvider;
        let mut profiles = self.read_character_profile_catalog()?.profiles;
        profiles.sort_by(|left, right| {
            right
                .favorite
                .cmp(&left.favorite)
                .then_with(|| {
                    right
                        .updated_at_unix_seconds
                        .cmp(&left.updated_at_unix_seconds)
                })
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(profiles
            .into_iter()
            .map(|profile| profile_view(profile, provider.status()))
            .collect())
    }

    pub fn save_character_profile_import(
        &self,
        request: &QuickSimRequest,
    ) -> Result<CharacterProfileView> {
        let (prepared, _) = prepare_request(request)?;
        let profile = prepared.profile;
        let identity = CharacterIdentity {
            region: profile.region.clone(),
            realm: profile.server.clone(),
            character_name: profile.name.clone(),
        };
        validate_identity(&identity)?;
        let now = now_seconds()?;
        let input = ProfileInputSnapshot {
            source: match request.format {
                SourceFormat::AddonExport => ProfileInputSource::AddonExport,
                SourceFormat::SimcFile => ProfileInputSource::SimcFile,
            },
            request: request.clone(),
            captured_at_unix_seconds: now,
        };
        let mut catalog = self.read_character_profile_catalog()?;
        let saved_identity_key = identity_key(&identity);
        let index = catalog
            .profiles
            .iter()
            .position(|existing| identity_key(&existing.identity) == saved_identity_key);
        let record = if let Some(index) = index {
            let existing = &mut catalog.profiles[index];
            if existing.active_input.request != input.request {
                existing.previous_input = Some(existing.active_input.clone());
            }
            existing.active_input = input;
            existing.display_name.clone_from(&profile.name);
            existing.class = profile.class.simc_token().into();
            existing.specialization.clone_from(&profile.specialization);
            existing.identity = identity;
            existing.updated_at_unix_seconds = now;
            existing.clone()
        } else {
            if catalog.profiles.len() >= MAX_CHARACTER_PROFILES {
                return Err(Error::CharacterProfile(format!(
                    "at most {MAX_CHARACTER_PROFILES} character profiles can be stored"
                )));
            }
            let record = CharacterProfileRecord {
                id: profile_id(&identity),
                identity,
                display_name: profile.name,
                class: profile.class.simc_token().into(),
                specialization: profile.specialization,
                favorite: false,
                active_input: input,
                previous_input: None,
                created_at_unix_seconds: now,
                updated_at_unix_seconds: now,
            };
            catalog.profiles.push(record.clone());
            record
        };
        self.write_character_profile_catalog(&catalog)?;
        Ok(profile_view(record, DisabledArmoryProvider.status()))
    }

    pub fn set_character_profile_favorite(
        &self,
        profile_id: &str,
        favorite: bool,
    ) -> Result<CharacterProfileView> {
        validate_profile_id(profile_id)?;
        let mut catalog = self.read_character_profile_catalog()?;
        let profile = catalog
            .profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| Error::CharacterProfile("character profile was not found".into()))?;
        profile.favorite = favorite;
        let record = profile.clone();
        self.write_character_profile_catalog(&catalog)?;
        Ok(profile_view(record, DisabledArmoryProvider.status()))
    }

    pub fn reload_character_profile_from_armory(
        &self,
        profile_id: &str,
    ) -> Result<CharacterProfileView> {
        self.reload_character_profile_with(profile_id, &DisabledArmoryProvider)
    }

    fn reload_character_profile_with(
        &self,
        profile_id: &str,
        provider: &dyn ArmoryProfileProvider,
    ) -> Result<CharacterProfileView> {
        validate_profile_id(profile_id)?;
        let status = provider.status();
        if !status.available {
            return Err(Error::ArmoryUnavailable(status.reason));
        }
        let mut catalog = self.read_character_profile_catalog()?;
        let profile = catalog
            .profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| Error::CharacterProfile("character profile was not found".into()))?;
        if profile.identity.region.is_none() || profile.identity.realm.is_none() {
            return Err(Error::CharacterProfile(
                "region and realm are required for Armory reload".into(),
            ));
        }
        let request = provider
            .reload(&profile.identity)
            .map_err(Error::ArmoryUnavailable)?;
        let (prepared, _) = prepare_request(&request)?;
        if !prepared
            .profile
            .name
            .eq_ignore_ascii_case(&profile.identity.character_name)
        {
            return Err(Error::CharacterProfile(
                "Armory response character does not match the saved profile".into(),
            ));
        }
        let now = now_seconds()?;
        profile.previous_input = Some(profile.active_input.clone());
        profile.active_input = ProfileInputSnapshot {
            source: ProfileInputSource::Armory,
            request,
            captured_at_unix_seconds: now,
        };
        profile.class = prepared.profile.class.simc_token().into();
        profile.specialization = prepared.profile.specialization;
        profile.updated_at_unix_seconds = now;
        let record = profile.clone();
        self.write_character_profile_catalog(&catalog)?;
        Ok(profile_view(record, status))
    }

    pub fn restore_previous_character_profile_input(
        &self,
        profile_id: &str,
    ) -> Result<CharacterProfileView> {
        validate_profile_id(profile_id)?;
        let mut catalog = self.read_character_profile_catalog()?;
        let profile = catalog
            .profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| Error::CharacterProfile("character profile was not found".into()))?;
        let previous = profile.previous_input.take().ok_or_else(|| {
            Error::CharacterProfile("there is no previous profile input to restore".into())
        })?;
        profile.active_input = previous;
        profile.updated_at_unix_seconds = now_seconds()?;
        let record = profile.clone();
        self.write_character_profile_catalog(&catalog)?;
        Ok(profile_view(record, DisabledArmoryProvider.status()))
    }

    fn read_character_profile_catalog(&self) -> Result<CharacterProfileCatalog> {
        let path = &self.profile_catalog_path;
        if !path.exists() {
            return Ok(CharacterProfileCatalog {
                schema_version: PROFILE_CATALOG_SCHEMA,
                profiles: Vec::new(),
            });
        }
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_PROFILE_CATALOG_BYTES
        {
            return Err(Error::CharacterProfile(
                "profile catalog must be a bounded regular file".into(),
            ));
        }
        let catalog: CharacterProfileCatalog = serde_json::from_slice(&fs::read(path)?)?;
        validate_catalog(&catalog)?;
        Ok(catalog)
    }

    fn write_character_profile_catalog(&self, catalog: &CharacterProfileCatalog) -> Result<()> {
        validate_catalog(catalog)?;
        let parent = self
            .profile_catalog_path
            .parent()
            .ok_or_else(|| Error::CharacterProfile("profile catalog has no parent".into()))?;
        ensure_real_directory(parent)?;
        let mut bytes = serde_json::to_vec_pretty(catalog)?;
        bytes.push(b'\n');
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_PROFILE_CATALOG_BYTES {
            return Err(Error::CharacterProfile(
                "serialized profile catalog exceeds 256 MiB".into(),
            ));
        }
        let mut destination = AtomicWriteFile::open(&self.profile_catalog_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            destination
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        destination.write_all(&bytes)?;
        destination.commit()?;
        Ok(())
    }
}

fn profile_view(
    profile: CharacterProfileRecord,
    armory_refresh: ArmoryRefreshStatus,
) -> CharacterProfileView {
    CharacterProfileView {
        id: profile.id,
        identity: profile.identity,
        display_name: profile.display_name,
        class: profile.class,
        specialization: profile.specialization,
        favorite: profile.favorite,
        input_source: profile.active_input.source,
        request: profile.active_input.request,
        captured_at_unix_seconds: profile.active_input.captured_at_unix_seconds,
        previous_input_available: profile.previous_input.is_some(),
        armory_refresh,
    }
}

fn validate_catalog(catalog: &CharacterProfileCatalog) -> Result<()> {
    if catalog.schema_version != PROFILE_CATALOG_SCHEMA {
        return Err(Error::CharacterProfile(format!(
            "unsupported profile catalog schema {}",
            catalog.schema_version
        )));
    }
    if catalog.profiles.len() > MAX_CHARACTER_PROFILES {
        return Err(Error::CharacterProfile(
            "too many character profiles".into(),
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    for profile in &catalog.profiles {
        validate_profile_id(&profile.id)?;
        validate_identity(&profile.identity)?;
        if !ids.insert(profile.id.as_str()) {
            return Err(Error::CharacterProfile(
                "profile catalog contains duplicate IDs".into(),
            ));
        }
        for input in std::iter::once(&profile.active_input).chain(profile.previous_input.as_ref()) {
            if input.request.source.is_empty() || input.request.source.len() > 2 * 1024 * 1024 {
                return Err(Error::CharacterProfile(
                    "profile input must be between 1 byte and 2 MiB".into(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_identity(identity: &CharacterIdentity) -> Result<()> {
    if identity.character_name.trim().is_empty() || identity.character_name.len() > 64 {
        return Err(Error::CharacterProfile(
            "character name must be between 1 and 64 bytes".into(),
        ));
    }
    for (label, value) in [("region", &identity.region), ("realm", &identity.realm)] {
        if value
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 128)
        {
            return Err(Error::CharacterProfile(format!(
                "{label} must be absent or between 1 and 128 bytes"
            )));
        }
    }
    Ok(())
}

fn validate_profile_id(value: &str) -> Result<()> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::CharacterProfile("profile ID is invalid".into()));
    }
    Ok(())
}

fn identity_key(identity: &CharacterIdentity) -> String {
    format!(
        "{}\0{}\0{}",
        identity
            .region
            .as_deref()
            .unwrap_or_default()
            .to_lowercase(),
        identity.realm.as_deref().unwrap_or_default().to_lowercase(),
        identity.character_name.to_lowercase()
    )
}

fn profile_id(identity: &CharacterIdentity) -> String {
    let digest = Sha256::digest(identity_key(identity).as_bytes());
    format!("{digest:x}")[..32].to_owned()
}

fn ensure_real_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::CharacterProfile(
            "profile catalog parent must be a real directory".into(),
        ));
    }
    Ok(())
}

fn now_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| Error::CharacterProfile("system clock is before the Unix epoch".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpuChoice, SourceFormat};

    struct FixtureArmoryProvider;

    impl ArmoryProfileProvider for FixtureArmoryProvider {
        fn status(&self) -> ArmoryRefreshStatus {
            ArmoryRefreshStatus {
                available: true,
                reason: "fixture".into(),
            }
        }

        fn reload(
            &self,
            identity: &CharacterIdentity,
        ) -> std::result::Result<QuickSimRequest, String> {
            Ok(request(&format!(
                "warrior={}\nregion=kr\nserver=azshara\nlevel=90\nrace=orc\nrole=attack\nspec=fury\nhead=,id=2\n",
                identity.character_name
            )))
        }
    }

    fn request(source: &str) -> QuickSimRequest {
        QuickSimRequest {
            source: source.into(),
            format: SourceFormat::SimcFile,
            iterations: 10_000,
            fixed_time: false,
            max_time_seconds: 300,
            vary_combat_length: 0.2,
            desired_targets: 1,
            fight_style: "Patchwerk".into(),
            cpu_preset: CpuChoice::Balanced,
        }
    }

    #[test]
    fn imported_inputs_are_upserted_and_previous_input_is_restorable() {
        let temporary = tempfile::tempdir().unwrap();
        let service = DesktopService::open(temporary.path()).unwrap();
        let first = service
            .save_character_profile_import(&request(
                "warrior=Core\nregion=kr\nserver=azshara\nlevel=90\nrace=orc\nrole=attack\nspec=fury\nhead=,id=1\n",
            ))
            .unwrap();
        assert_eq!(service.character_profiles().unwrap().len(), 1);

        let second = service
            .save_character_profile_import(&request(
                "warrior=Core\nregion=kr\nserver=azshara\nlevel=90\nrace=orc\nrole=attack\nspec=fury\nhead=,id=2\n",
            ))
            .unwrap();
        assert_eq!(first.id, second.id);
        assert!(second.previous_input_available);
        assert!(second.request.source.contains("id=2"));

        let restored = service
            .restore_previous_character_profile_input(&first.id)
            .unwrap();
        assert!(restored.request.source.contains("id=1"));
        assert!(!restored.previous_input_available);
    }

    #[test]
    fn armory_reload_is_fail_closed_until_provider_exists_and_replaces_the_input() {
        let temporary = tempfile::tempdir().unwrap();
        let service = DesktopService::open(temporary.path()).unwrap();
        let saved = service
            .save_character_profile_import(&request(
                "warrior=Core\nregion=kr\nserver=azshara\nlevel=90\nrace=orc\nrole=attack\nspec=fury\nhead=,id=1\n",
            ))
            .unwrap();
        assert!(matches!(
            service.reload_character_profile_from_armory(&saved.id),
            Err(Error::ArmoryUnavailable(_))
        ));

        let refreshed = service
            .reload_character_profile_with(&saved.id, &FixtureArmoryProvider)
            .unwrap();
        assert_eq!(refreshed.input_source, ProfileInputSource::Armory);
        assert!(refreshed.request.source.contains("id=2"));
        assert!(refreshed.previous_input_available);
        assert_eq!(service.character_profiles().unwrap().len(), 1);
    }

    #[test]
    fn profile_catalog_rejects_symlink_substitution() {
        #[cfg(unix)]
        {
            let temporary = tempfile::tempdir().unwrap();
            let service = DesktopService::open(temporary.path()).unwrap();
            let outside = temporary.path().join("outside.json");
            fs::write(&outside, b"{}\n").unwrap();
            std::os::unix::fs::symlink(&outside, &service.profile_catalog_path).unwrap();
            assert!(service.character_profiles().is_err());
        }
    }
}
