use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::PathBuf,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use atomic_write_file::AtomicWriteFile;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
#[cfg(feature = "signing")]
use ed25519_dalek::{Signer, SigningKey, pkcs8::DecodePrivateKey};
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use simc_adapter::RuntimeManifest;

use crate::{Error, Result};

const CATALOG_SCHEMA_VERSION: u32 = 1;
const TRUST_SCHEMA_VERSION: u32 = 1;
const SIGNATURE_DOMAIN: &[u8] = b"SimShredder runtime catalog v1\0";
const MAX_CATALOG_BYTES: usize = 1024 * 1024;
const MAX_CATALOG_LIFETIME_SECONDS: u64 = 366 * 24 * 60 * 60;
const MAX_CLOCK_SKEW_SECONDS: u64 = 5 * 60;
const MAX_KEYS: usize = 16;
const MAX_MANIFESTS: usize = 16;
const MAX_TRUST_CHAIN: usize = 64;
pub const PRODUCTION_CATALOG_URL: &str = "https://github.com/DobiShredder/SimShredder/releases/download/simc-runtime-catalog/runtime-catalog.json";
const GITHUB_RELEASE_ASSET_HOST: &str = "release-assets.githubusercontent.com";
const GITHUB_RELEASE_ASSET_PATH_PREFIX: &str = "/github-production-release-asset/";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedCatalogKey {
    pub key_id: String,
    pub algorithm: String,
    pub public_key_base64: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPayload {
    pub schema_version: u32,
    pub sequence: u64,
    pub generated_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
    pub manifests: Vec<RuntimeManifest>,
    #[serde(default)]
    pub next_keys: Vec<TrustedCatalogKey>,
    #[serde(default)]
    pub revoked_key_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSignature {
    pub key_id: String,
    pub algorithm: String,
    pub signature_base64: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCatalog {
    pub payload: CatalogPayload,
    pub signatures: Vec<CatalogSignature>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogTrustState {
    pub schema_version: u32,
    pub highest_sequence: u64,
    pub accepted_payload_sha256: String,
    pub trusted_keys: Vec<TrustedCatalogKey>,
    pub accepted_catalogs: Vec<SignedCatalog>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedCatalog {
    pub payload: CatalogPayload,
    pub verified_by: Vec<String>,
}

impl CatalogPayload {
    pub fn validate_at(&self, now_unix_seconds: u64) -> Result<()> {
        validate_payload(self, now_unix_seconds)
    }
}

impl super::RuntimeManager {
    pub fn verify_and_accept_catalog(
        &self,
        catalog_bytes: &[u8],
        bundled_root_keys: &[TrustedCatalogKey],
        now_unix_seconds: u64,
    ) -> Result<VerifiedCatalog> {
        self.verify_and_accept_catalog_inner(
            catalog_bytes,
            bundled_root_keys,
            now_unix_seconds,
            None,
        )
    }

    pub fn verify_and_accept_catalog_for_target(
        &self,
        catalog_bytes: &[u8],
        bundled_root_keys: &[TrustedCatalogKey],
        now_unix_seconds: u64,
        platform: &str,
        architecture: &str,
    ) -> Result<VerifiedCatalog> {
        self.verify_and_accept_catalog_inner(
            catalog_bytes,
            bundled_root_keys,
            now_unix_seconds,
            Some((platform, architecture)),
        )
    }

    fn verify_and_accept_catalog_inner(
        &self,
        catalog_bytes: &[u8],
        bundled_root_keys: &[TrustedCatalogKey],
        now_unix_seconds: u64,
        required_target: Option<(&str, &str)>,
    ) -> Result<VerifiedCatalog> {
        if catalog_bytes.is_empty() || catalog_bytes.len() > MAX_CATALOG_BYTES {
            return Err(Error::InvalidCatalog(
                "catalog must be non-empty and no larger than 1 MiB".into(),
            ));
        }
        let catalog: SignedCatalog = serde_json::from_slice(catalog_bytes)?;
        validate_payload(&catalog.payload, now_unix_seconds)?;
        validate_signatures(&catalog.signatures)?;
        if let Some((platform, architecture)) = required_target
            && !catalog.payload.manifests.iter().any(|manifest| {
                manifest.platform == platform && manifest.architecture == architecture
            })
        {
            return Err(Error::InvalidCatalog(format!(
                "catalog has no {platform} {architecture} runtime"
            )));
        }

        let payload_bytes = canonical_payload_bytes(&catalog.payload)?;
        let payload_sha256 = format!("{:x}", Sha256::digest(&payload_bytes));
        let existing = self.load_catalog_trust_state()?;
        let (trusted_keys, mut accepted_catalogs) = if let Some(state) = &existing {
            let replayed = replay_trust_chain(&state.accepted_catalogs, bundled_root_keys)?;
            if replayed.highest_sequence != state.highest_sequence
                || replayed.accepted_payload_sha256 != state.accepted_payload_sha256
                || replayed.trusted_keys != state.trusted_keys
            {
                return Err(Error::InvalidCatalog(
                    "catalog trust summary differs from its signed chain".into(),
                ));
            }
            if catalog.payload.sequence < state.highest_sequence {
                return Err(Error::CatalogRollback {
                    accepted: state.highest_sequence,
                    offered: catalog.payload.sequence,
                });
            }
            if catalog.payload.sequence == state.highest_sequence {
                if payload_sha256 != state.accepted_payload_sha256 {
                    return Err(Error::InvalidCatalog(
                        "catalog reuses an accepted sequence with different content".into(),
                    ));
                }
                return Ok(VerifiedCatalog {
                    payload: catalog.payload,
                    verified_by: replayed.latest_verified_by,
                });
            }
            (replayed.trusted_keys, state.accepted_catalogs.clone())
        } else {
            (validated_root_keys(bundled_root_keys)?, Vec::new())
        };

        let verified_by = verify_signatures(&catalog.signatures, &trusted_keys, &payload_bytes)?;

        let next_trusted_keys = rotated_keys(
            trusted_keys,
            &catalog.payload.next_keys,
            &catalog.payload.revoked_key_ids,
        )?;
        if accepted_catalogs.len() >= MAX_TRUST_CHAIN {
            return Err(Error::InvalidCatalog(
                "catalog trust chain reached its safety limit".into(),
            ));
        }
        accepted_catalogs.push(catalog.clone());
        self.write_catalog_trust_state(&CatalogTrustState {
            schema_version: TRUST_SCHEMA_VERSION,
            highest_sequence: catalog.payload.sequence,
            accepted_payload_sha256: payload_sha256,
            trusted_keys: next_trusted_keys,
            accepted_catalogs,
        })?;
        Ok(VerifiedCatalog {
            payload: catalog.payload,
            verified_by,
        })
    }

    pub fn catalog_trust_state(&self) -> Result<Option<CatalogTrustState>> {
        self.load_catalog_trust_state()
    }

    pub fn accepted_catalog(
        &self,
        bundled_root_keys: &[TrustedCatalogKey],
        now_unix_seconds: u64,
    ) -> Result<Option<VerifiedCatalog>> {
        let Some(state) = self.load_catalog_trust_state()? else {
            return Ok(None);
        };
        let replayed = replay_trust_chain(&state.accepted_catalogs, bundled_root_keys)?;
        if replayed.highest_sequence != state.highest_sequence
            || replayed.accepted_payload_sha256 != state.accepted_payload_sha256
            || replayed.trusted_keys != state.trusted_keys
        {
            return Err(Error::InvalidCatalog(
                "catalog trust summary differs from its signed chain".into(),
            ));
        }
        let latest = state
            .accepted_catalogs
            .last()
            .ok_or_else(|| Error::InvalidCatalog("catalog trust chain is empty".into()))?;
        latest.payload.validate_at(now_unix_seconds)?;
        Ok(Some(VerifiedCatalog {
            payload: latest.payload.clone(),
            verified_by: replayed.latest_verified_by,
        }))
    }

    fn catalog_trust_path(&self) -> PathBuf {
        self.root().join("runtime-catalog-trust.json")
    }

    fn load_catalog_trust_state(&self) -> Result<Option<CatalogTrustState>> {
        let path = self.catalog_trust_path();
        if !path.exists() {
            return Ok(None);
        }
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() as usize > MAX_CATALOG_BYTES
        {
            return Err(Error::InvalidCatalog(
                "catalog trust state must be a bounded regular file".into(),
            ));
        }
        let state: CatalogTrustState = serde_json::from_slice(&fs::read(path)?)?;
        validate_trust_state(&state)?;
        Ok(Some(state))
    }

    fn write_catalog_trust_state(&self, state: &CatalogTrustState) -> Result<()> {
        validate_trust_state(state)?;
        let mut bytes = serde_json::to_vec_pretty(state)?;
        bytes.push(b'\n');
        let mut destination = AtomicWriteFile::open(self.catalog_trust_path())?;
        #[cfg(unix)]
        destination
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        destination.write_all(&bytes)?;
        destination.commit()?;
        Ok(())
    }
}

pub fn download_production_catalog() -> Result<Vec<u8>> {
    let client = Client::builder()
        .redirect(Policy::custom(|attempt| {
            if allowed_catalog_redirect(attempt.previous(), attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| Error::CatalogDownload(error.to_string()))?;
    let response = client
        .get(PRODUCTION_CATALOG_URL)
        .send()
        .map_err(|error| Error::CatalogDownload(error.to_string()))?;
    if response.status().is_redirection() {
        return Err(Error::CatalogDownload(
            "production catalog returned a disallowed redirect".into(),
        ));
    }
    if !valid_catalog_response_url(response.url()) {
        return Err(Error::CatalogDownload(
            "production catalog response came from an untrusted URL".into(),
        ));
    }
    let mut response = response
        .error_for_status()
        .map_err(|error| Error::CatalogDownload(error.to_string()))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CATALOG_BYTES as u64)
    {
        return Err(Error::CatalogDownload(
            "remote catalog exceeds the 1 MiB limit".into(),
        ));
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_CATALOG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::CatalogDownload(error.to_string()))?;
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(Error::CatalogDownload(
            "remote catalog exceeds the 1 MiB limit".into(),
        ));
    }
    Ok(bytes)
}

fn allowed_catalog_redirect(previous: &[Url], target: &Url) -> bool {
    previous.len() == 1
        && previous[0].as_str() == PRODUCTION_CATALOG_URL
        && valid_github_release_asset_url(target)
}

fn valid_catalog_response_url(url: &Url) -> bool {
    url.as_str() == PRODUCTION_CATALOG_URL || valid_github_release_asset_url(url)
}

fn valid_github_release_asset_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some(GITHUB_RELEASE_ASSET_HOST)
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none()
        && url.path().starts_with(GITHUB_RELEASE_ASSET_PATH_PREFIX)
}

fn canonical_payload_bytes(payload: &CatalogPayload) -> Result<Vec<u8>> {
    let payload = serde_json::to_vec(payload)?;
    let mut message = Vec::with_capacity(SIGNATURE_DOMAIN.len() + payload.len());
    message.extend_from_slice(SIGNATURE_DOMAIN);
    message.extend_from_slice(&payload);
    Ok(message)
}

fn validate_payload(payload: &CatalogPayload, now: u64) -> Result<()> {
    if payload.schema_version != CATALOG_SCHEMA_VERSION || payload.sequence == 0 {
        return Err(Error::InvalidCatalog(
            "unsupported catalog schema or zero sequence".into(),
        ));
    }
    if payload.generated_at_unix_seconds > now.saturating_add(MAX_CLOCK_SKEW_SECONDS) {
        return Err(Error::InvalidCatalog(
            "catalog generation time is in the future".into(),
        ));
    }
    if payload.expires_at_unix_seconds <= now
        || payload.expires_at_unix_seconds <= payload.generated_at_unix_seconds
        || payload
            .expires_at_unix_seconds
            .saturating_sub(payload.generated_at_unix_seconds)
            > MAX_CATALOG_LIFETIME_SECONDS
    {
        return Err(Error::InvalidCatalog(
            "catalog is expired or has an excessive validity interval".into(),
        ));
    }
    if payload.manifests.is_empty() || payload.manifests.len() > MAX_MANIFESTS {
        return Err(Error::InvalidCatalog(
            "catalog manifest count is out of range".into(),
        ));
    }
    let mut previous_target: Option<(String, String)> = None;
    for manifest in &payload.manifests {
        manifest.validate()?;
        let target = (manifest.platform.clone(), manifest.architecture.clone());
        if previous_target
            .as_ref()
            .is_some_and(|previous| previous >= &target)
        {
            return Err(Error::InvalidCatalog(
                "catalog targets must be unique and sorted".into(),
            ));
        }
        previous_target = Some(target);
    }
    validate_sorted_keys(&payload.next_keys, true)?;
    validate_sorted_ids(&payload.revoked_key_ids)?;
    let next_ids: BTreeSet<&str> = payload
        .next_keys
        .iter()
        .map(|key| key.key_id.as_str())
        .collect();
    if payload
        .revoked_key_ids
        .iter()
        .any(|key_id| next_ids.contains(key_id.as_str()))
    {
        return Err(Error::InvalidCatalog(
            "the same key cannot be introduced and revoked".into(),
        ));
    }
    Ok(())
}

fn validate_signatures(signatures: &[CatalogSignature]) -> Result<()> {
    if signatures.is_empty() || signatures.len() > MAX_KEYS {
        return Err(Error::InvalidCatalog(
            "catalog signature count is out of range".into(),
        ));
    }
    let mut previous = None;
    for signature in signatures {
        validate_key_id(&signature.key_id)?;
        if signature.algorithm != "ed25519" {
            return Err(Error::InvalidCatalog(
                "catalog signature algorithm is unsupported".into(),
            ));
        }
        if previous.is_some_and(|value: &str| value >= signature.key_id.as_str()) {
            return Err(Error::InvalidCatalog(
                "catalog signatures must be unique and sorted".into(),
            ));
        }
        previous = Some(signature.key_id.as_str());
    }
    Ok(())
}

fn validated_root_keys(keys: &[TrustedCatalogKey]) -> Result<Vec<TrustedCatalogKey>> {
    if keys.is_empty() {
        return Err(Error::InvalidCatalog(
            "no bundled trust root is configured".into(),
        ));
    }
    validate_sorted_keys(keys, false)?;
    Ok(keys.to_vec())
}

fn validate_sorted_keys(keys: &[TrustedCatalogKey], allow_empty: bool) -> Result<()> {
    if (!allow_empty && keys.is_empty()) || keys.len() > MAX_KEYS {
        return Err(Error::InvalidCatalog(
            "trusted key count is out of range".into(),
        ));
    }
    let mut previous = None;
    for key in keys {
        validate_key(key)?;
        if previous.is_some_and(|value: &str| value >= key.key_id.as_str()) {
            return Err(Error::InvalidCatalog(
                "trusted keys must be unique and sorted".into(),
            ));
        }
        previous = Some(key.key_id.as_str());
    }
    Ok(())
}

fn validate_sorted_ids(ids: &[String]) -> Result<()> {
    if ids.len() > MAX_KEYS {
        return Err(Error::InvalidCatalog(
            "revoked key count is out of range".into(),
        ));
    }
    let mut previous = None;
    for id in ids {
        validate_key_id(id)?;
        if previous.is_some_and(|value: &str| value >= id.as_str()) {
            return Err(Error::InvalidCatalog(
                "revoked key IDs must be unique and sorted".into(),
            ));
        }
        previous = Some(id.as_str());
    }
    Ok(())
}

fn validate_key_id(key_id: &str) -> Result<()> {
    if key_id.is_empty()
        || key_id.len() > 64
        || !key_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(Error::InvalidCatalog("catalog key ID is unsafe".into()));
    }
    Ok(())
}

fn validate_key(key: &TrustedCatalogKey) -> Result<VerifyingKey> {
    validate_key_id(&key.key_id)?;
    if key.algorithm != "ed25519" {
        return Err(Error::InvalidCatalog(
            "catalog key algorithm is unsupported".into(),
        ));
    }
    let bytes = BASE64
        .decode(&key.public_key_base64)
        .map_err(|_| Error::InvalidCatalog("catalog public key is malformed".into()))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Error::InvalidCatalog("Ed25519 public key must be 32 bytes".into()))?;
    VerifyingKey::from_bytes(&bytes)
        .map_err(|_| Error::InvalidCatalog("Ed25519 public key is invalid".into()))
}

fn verify_signatures(
    signatures: &[CatalogSignature],
    trusted_keys: &[TrustedCatalogKey],
    message: &[u8],
) -> Result<Vec<String>> {
    let trusted: BTreeMap<&str, VerifyingKey> = trusted_keys
        .iter()
        .map(|key| validate_key(key).map(|value| (key.key_id.as_str(), value)))
        .collect::<Result<_>>()?;
    let mut verified_by = Vec::new();
    for catalog_signature in signatures {
        let Some(key) = trusted.get(catalog_signature.key_id.as_str()) else {
            continue;
        };
        let bytes = BASE64
            .decode(&catalog_signature.signature_base64)
            .map_err(|_| Error::InvalidCatalog("catalog signature is malformed".into()))?;
        let signature = Signature::from_slice(&bytes)
            .map_err(|_| Error::InvalidCatalog("Ed25519 signature must be 64 bytes".into()))?;
        if key.verify(message, &signature).is_ok() {
            verified_by.push(catalog_signature.key_id.clone());
        }
    }
    if verified_by.is_empty() {
        return Err(Error::CatalogSignature);
    }
    Ok(verified_by)
}

fn rotated_keys(
    current: Vec<TrustedCatalogKey>,
    additions: &[TrustedCatalogKey],
    revoked: &[String],
) -> Result<Vec<TrustedCatalogKey>> {
    let mut keys: BTreeMap<String, TrustedCatalogKey> = current
        .into_iter()
        .map(|key| (key.key_id.clone(), key))
        .collect();
    for key in additions {
        validate_key(key)?;
        if let Some(existing) = keys.get(&key.key_id)
            && existing != key
        {
            return Err(Error::InvalidCatalog(
                "catalog key ID collides with different key material".into(),
            ));
        }
        keys.insert(key.key_id.clone(), key.clone());
    }
    for key_id in revoked {
        keys.remove(key_id);
    }
    if keys.is_empty() || keys.len() > MAX_KEYS {
        return Err(Error::InvalidCatalog(
            "catalog rotation must leave at least one trusted key".into(),
        ));
    }
    Ok(keys.into_values().collect())
}

fn validate_trust_state(state: &CatalogTrustState) -> Result<()> {
    if state.schema_version != TRUST_SCHEMA_VERSION
        || state.highest_sequence == 0
        || state.accepted_payload_sha256.len() != 64
        || !state
            .accepted_payload_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(Error::InvalidCatalog(
            "catalog trust state is invalid".into(),
        ));
    }
    validate_sorted_keys(&state.trusted_keys, false)?;
    if state.accepted_catalogs.is_empty() || state.accepted_catalogs.len() > MAX_TRUST_CHAIN {
        return Err(Error::InvalidCatalog(
            "catalog trust chain length is out of range".into(),
        ));
    }
    Ok(())
}

struct ReplayedTrust {
    highest_sequence: u64,
    accepted_payload_sha256: String,
    trusted_keys: Vec<TrustedCatalogKey>,
    latest_verified_by: Vec<String>,
}

fn replay_trust_chain(
    catalogs: &[SignedCatalog],
    bundled_root_keys: &[TrustedCatalogKey],
) -> Result<ReplayedTrust> {
    if catalogs.is_empty() || catalogs.len() > MAX_TRUST_CHAIN {
        return Err(Error::InvalidCatalog(
            "catalog trust chain length is out of range".into(),
        ));
    }
    let mut trusted_keys = validated_root_keys(bundled_root_keys)?;
    let mut previous_sequence = 0;
    let mut accepted_payload_sha256 = String::new();
    let mut latest_verified_by = Vec::new();
    for catalog in catalogs {
        catalog
            .payload
            .validate_at(catalog.payload.generated_at_unix_seconds)?;
        validate_signatures(&catalog.signatures)?;
        if catalog.payload.sequence <= previous_sequence {
            return Err(Error::InvalidCatalog(
                "catalog trust chain sequence is not strictly increasing".into(),
            ));
        }
        let message = canonical_payload_bytes(&catalog.payload)?;
        latest_verified_by = verify_signatures(&catalog.signatures, &trusted_keys, &message)?;
        trusted_keys = rotated_keys(
            trusted_keys,
            &catalog.payload.next_keys,
            &catalog.payload.revoked_key_ids,
        )?;
        previous_sequence = catalog.payload.sequence;
        accepted_payload_sha256 = format!("{:x}", Sha256::digest(&message));
    }
    Ok(ReplayedTrust {
        highest_sequence: previous_sequence,
        accepted_payload_sha256,
        trusted_keys,
        latest_verified_by,
    })
}

#[cfg(feature = "signing")]
pub fn signing_key_from_pkcs8_pem(pem: &str) -> Result<SigningKey> {
    SigningKey::from_pkcs8_pem(pem)
        .map_err(|_| Error::InvalidCatalog("Ed25519 PKCS#8 private key is invalid".into()))
}

#[cfg(feature = "signing")]
pub fn sign_catalog_payload(
    payload: &CatalogPayload,
    key_id: &str,
    key: &SigningKey,
) -> Result<CatalogSignature> {
    validate_key_id(key_id)?;
    payload.validate_at(payload.generated_at_unix_seconds)?;
    let signature = key.sign(&canonical_payload_bytes(payload)?);
    Ok(CatalogSignature {
        key_id: key_id.to_owned(),
        algorithm: "ed25519".into(),
        signature_base64: BASE64.encode(signature.to_bytes()),
    })
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn manifest() -> RuntimeManifest {
        RuntimeManifest {
            schema_version: 1,
            simc_version: "1210-01".into(),
            game_channel: "live".into(),
            platform: "macos".into(),
            architecture: "aarch64".into(),
            build: "3487fce".into(),
            filename: "simc-1210-01-macos-3487fce.dmg".into(),
            url: "https://downloads.simulationcraft.org/nightly/simc-1210-01-macos-3487fce.dmg"
                .into(),
            size: 226_929_563,
            sha256: "2b0908e1a720a43b78c1e727692436fe707d317787143c5a61c3d46c830caf48".into(),
        }
    }

    fn key(seed: u8, id: &str) -> (SigningKey, TrustedCatalogKey) {
        let signing = SigningKey::from_bytes(&[seed; 32]);
        let trusted = TrustedCatalogKey {
            key_id: id.into(),
            algorithm: "ed25519".into(),
            public_key_base64: BASE64.encode(signing.verifying_key().to_bytes()),
        };
        (signing, trusted)
    }

    fn payload(sequence: u64) -> CatalogPayload {
        CatalogPayload {
            schema_version: 1,
            sequence,
            generated_at_unix_seconds: NOW - 60,
            expires_at_unix_seconds: NOW + 86_400,
            manifests: vec![manifest()],
            next_keys: Vec::new(),
            revoked_key_ids: Vec::new(),
        }
    }

    fn signed(payload: CatalogPayload, key_id: &str, key: &SigningKey) -> Vec<u8> {
        let signature = key.sign(&canonical_payload_bytes(&payload).unwrap());
        serde_json::to_vec(&SignedCatalog {
            payload,
            signatures: vec![CatalogSignature {
                key_id: key_id.into(),
                algorithm: "ed25519".into(),
                signature_base64: BASE64.encode(signature.to_bytes()),
            }],
        })
        .unwrap()
    }

    #[test]
    fn accepts_valid_catalog_idempotently_and_rejects_tamper_and_rollback() {
        let temporary = tempfile::tempdir().unwrap();
        let manager = super::super::RuntimeManager::open(temporary.path()).unwrap();
        let (root_signing, root) = key(7, "release-2026-a");
        let catalog = signed(payload(2), &root.key_id, &root_signing);
        let accepted = manager
            .verify_and_accept_catalog(&catalog, std::slice::from_ref(&root), NOW)
            .unwrap();
        assert_eq!(accepted.verified_by, vec![root.key_id.clone()]);
        manager
            .verify_and_accept_catalog(&catalog, std::slice::from_ref(&root), NOW)
            .unwrap();

        let mut tampered: SignedCatalog = serde_json::from_slice(&catalog).unwrap();
        tampered.payload.manifests[0].size += 1;
        assert!(matches!(
            manager.verify_and_accept_catalog(
                &serde_json::to_vec(&tampered).unwrap(),
                std::slice::from_ref(&root),
                NOW
            ),
            Err(Error::InvalidCatalog(_)) | Err(Error::CatalogSignature)
        ));

        let rollback = signed(payload(1), &root.key_id, &root_signing);
        assert!(matches!(
            manager.verify_and_accept_catalog(&rollback, &[root], NOW),
            Err(Error::CatalogRollback { .. })
        ));
    }

    #[test]
    fn rotates_to_a_new_key_and_can_revoke_the_old_key() {
        let temporary = tempfile::tempdir().unwrap();
        let manager = super::super::RuntimeManager::open(temporary.path()).unwrap();
        let (old_signing, old) = key(1, "release-2026-a");
        let (new_signing, new) = key(2, "release-2027-a");

        let mut introduce = payload(1);
        introduce.next_keys.push(new.clone());
        manager
            .verify_and_accept_catalog(
                &signed(introduce, &old.key_id, &old_signing),
                std::slice::from_ref(&old),
                NOW,
            )
            .unwrap();

        let mut revoke = payload(2);
        revoke.revoked_key_ids.push(old.key_id.clone());
        manager
            .verify_and_accept_catalog(
                &signed(revoke, &new.key_id, &new_signing),
                std::slice::from_ref(&old),
                NOW,
            )
            .unwrap();
        let state = manager.catalog_trust_state().unwrap().unwrap();
        assert_eq!(state.trusted_keys, vec![new.clone()]);

        let old_again = signed(payload(3), &old.key_id, &old_signing);
        assert!(matches!(
            manager.verify_and_accept_catalog(&old_again, &[old], NOW),
            Err(Error::CatalogSignature)
        ));
    }

    #[test]
    fn rejects_expired_unknown_and_same_sequence_substitution() {
        let temporary = tempfile::tempdir().unwrap();
        let manager = super::super::RuntimeManager::open(temporary.path()).unwrap();
        let (root_signing, root) = key(3, "release-2026-a");
        let (unknown_signing, unknown) = key(4, "attacker-2026-a");

        let mut expired = payload(1);
        expired.expires_at_unix_seconds = NOW;
        assert!(matches!(
            manager.verify_and_accept_catalog(
                &signed(expired, &root.key_id, &root_signing),
                std::slice::from_ref(&root),
                NOW
            ),
            Err(Error::InvalidCatalog(_))
        ));
        assert!(matches!(
            manager.verify_and_accept_catalog(
                &signed(payload(1), &unknown.key_id, &unknown_signing),
                std::slice::from_ref(&root),
                NOW
            ),
            Err(Error::CatalogSignature)
        ));

        manager
            .verify_and_accept_catalog(
                &signed(payload(1), &root.key_id, &root_signing),
                std::slice::from_ref(&root),
                NOW,
            )
            .unwrap();
        let mut substitute = payload(1);
        substitute.generated_at_unix_seconds -= 1;
        assert!(matches!(
            manager.verify_and_accept_catalog(
                &signed(substitute, &root.key_id, &root_signing),
                &[root],
                NOW
            ),
            Err(Error::InvalidCatalog(_))
        ));
    }

    #[test]
    fn requires_the_requested_target_before_persisting_and_replays_cached_catalog() {
        let temporary = tempfile::tempdir().unwrap();
        let manager = super::super::RuntimeManager::open(temporary.path()).unwrap();
        let (root_signing, root) = key(8, "release-2026-a");
        let catalog = signed(payload(1), &root.key_id, &root_signing);

        assert!(matches!(
            manager.verify_and_accept_catalog_for_target(
                &catalog,
                std::slice::from_ref(&root),
                NOW,
                "windows",
                "x86_64"
            ),
            Err(Error::InvalidCatalog(_))
        ));
        assert!(manager.catalog_trust_state().unwrap().is_none());

        manager
            .verify_and_accept_catalog_for_target(
                &catalog,
                std::slice::from_ref(&root),
                NOW,
                "macos",
                "aarch64",
            )
            .unwrap();
        let cached = manager
            .accepted_catalog(std::slice::from_ref(&root), NOW)
            .unwrap()
            .unwrap();
        assert_eq!(cached.payload.sequence, 1);
        assert_eq!(cached.verified_by, vec![root.key_id]);
    }

    #[test]
    fn follows_only_the_single_https_github_release_asset_redirect() {
        let source = Url::parse(PRODUCTION_CATALOG_URL).unwrap();
        let asset = Url::parse(
            "https://release-assets.githubusercontent.com/github-production-release-asset/123/abc?sp=r&sig=opaque",
        )
        .unwrap();
        assert!(allowed_catalog_redirect(
            std::slice::from_ref(&source),
            &asset
        ));
        assert!(valid_catalog_response_url(&source));
        assert!(valid_catalog_response_url(&asset));

        for rejected in [
            "http://release-assets.githubusercontent.com/github-production-release-asset/123/abc",
            "https://release-assets.githubusercontent.com:444/github-production-release-asset/123/abc",
            "https://user@release-assets.githubusercontent.com/github-production-release-asset/123/abc",
            "https://release-assets.githubusercontent.com/not-a-release/123/abc",
            "https://objects.githubusercontent.com/github-production-release-asset/123/abc",
            "https://example.com/github-production-release-asset/123/abc",
        ] {
            let rejected = Url::parse(rejected).unwrap();
            assert!(!allowed_catalog_redirect(
                std::slice::from_ref(&source),
                &rejected
            ));
            assert!(!valid_catalog_response_url(&rejected));
        }

        assert!(!allowed_catalog_redirect(
            &[source.clone(), asset.clone()],
            &asset
        ));
        let wrong_source = Url::parse(
            "https://github.com/attacker/repository/releases/download/simc-runtime-catalog/runtime-catalog.json",
        )
        .unwrap();
        assert!(!allowed_catalog_redirect(&[wrong_source], &asset));
    }
}
