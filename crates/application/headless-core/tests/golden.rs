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
        assert_eq!(
            prepared.generated_bytes,
            fs::read(root.join(format!(
                "test-data/fixtures/generated/{name}.generated.simc"
            )))
            .unwrap()
        );
    }
}
