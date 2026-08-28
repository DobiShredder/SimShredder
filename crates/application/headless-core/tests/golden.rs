use std::{fs, path::PathBuf};

use simshredder_core::{InputFormat, prepare};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .expect("crate must be in workspace")
        .to_owned()
}

#[test]
fn both_supported_inputs_prepare_to_their_golden_bytes() {
    let root = root();
    for (name, format) in [
        ("addon-warrior", InputFormat::AddonExport),
        ("file-warrior", InputFormat::SimcFile),
    ] {
        let source =
            fs::read_to_string(root.join(format!("test-data/fixtures/profiles/{name}.simc")))
                .unwrap();
        let prepared = prepare(&source, format).unwrap();
        assert!(prepared.generated_bytes.starts_with(source.as_bytes()));
        let overlay = prepared
            .generated_bytes
            .strip_prefix(source.as_bytes())
            .expect("the imported document must be an exact generated-input prefix");
        assert_eq!(
            overlay,
            b"\n# SimShredder deterministic run overlay\niterations=100\nfixed_time=1\nmax_time=20\nvary_combat_length=0\ndesired_targets=1\nfight_style=Patchwerk\nthreads=1\nseed=12345\nreport_details=1\n"
        );
    }
}

#[test]
fn advanced_standard_tci_is_preserved_and_classified() {
    let source =
        fs::read_to_string(root().join("test-data/fixtures/profiles/advanced-warrior.simc"))
            .unwrap();
    let prepared = prepare(&source, InputFormat::SimcFile).unwrap();
    assert!(prepared.generated_bytes.starts_with(source.as_bytes()));
    assert_eq!(prepared.compatibility.execution_blocked, 0);
    assert!(prepared.compatibility.preserved_not_editable >= 3);
    assert_eq!(prepared.profile.name, "Advanced Warrior");
    assert_eq!(prepared.profile.actions.len(), 2);
}
