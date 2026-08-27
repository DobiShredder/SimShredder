use std::{fs, path::PathBuf};

use input_builder::build_input;
use profile_parser::{parse_addon_export, parse_simc_file};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .expect("crate must be in workspace")
        .to_owned()
}

#[test]
fn addon_and_simc_inputs_match_byte_golden() {
    let root = root();
    let cases = [("addon-warrior", true), ("file-warrior", false)];
    for (name, addon) in cases {
        let source =
            fs::read_to_string(root.join(format!("test-data/fixtures/profiles/{name}.simc")))
                .unwrap();
        let profile = if addon {
            parse_addon_export(&source).unwrap()
        } else {
            parse_simc_file(&source).unwrap()
        };
        let actual = build_input(&profile).unwrap();
        let expected = fs::read(root.join(format!(
            "test-data/fixtures/generated/{name}.generated.simc"
        )))
        .unwrap();
        assert_eq!(actual, expected, "{name} generated input changed");
    }
}
