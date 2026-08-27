use simshredder_runtime_manager::{RuntimeManager, TrustedCatalogKey};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn bundled_catalog_has_a_valid_production_signature() {
    let roots: Vec<TrustedCatalogKey> = serde_json::from_slice(include_bytes!(
        "../../../../apps/desktop/src-tauri/resources/runtime-catalog-trust-roots.json"
    ))
    .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let manager = RuntimeManager::open(temporary.path()).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let verified = manager
        .verify_and_accept_catalog(
            include_bytes!("../../../../apps/desktop/src-tauri/resources/runtime-catalog.json"),
            &roots,
            now,
        )
        .unwrap();
    assert_eq!(verified.payload.sequence, 6);
    assert_eq!(verified.verified_by, vec!["runtime-release-2026-a"]);
    assert_eq!(verified.payload.manifests.len(), 2);
    assert!(verified.payload.manifests.iter().all(|manifest| {
        manifest
            .url
            .starts_with("http://downloads.simulationcraft.org/nightly/")
    }));
    assert_eq!(
        verified.payload.manifests[0].sha256,
        "2e248c6da7dda4807d22a309600c166143cc08d0b5c719e77bfef89e9eb3a17f"
    );
}
