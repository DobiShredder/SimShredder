use std::{fs, path::Path};

use profile_parser::{parse_document, parse_simc_file};

#[test]
#[ignore = "reads an explicitly supplied checkout of the official simc-profile corpus"]
fn official_simc_profile_corpus_is_lossless_and_projects_live_profiles() {
    let root = std::env::var_os("SIMSHREDDER_SIMC_PROFILE_ROOT")
        .expect("SIMSHREDDER_SIMC_PROFILE_ROOT must point to an official checkout");
    let mut paths = Vec::new();
    collect_simc_files(Path::new(&root), &mut paths);
    paths.sort();
    assert!(!paths.is_empty(), "official corpus contains no .simc files");

    let mut projected = 0;
    for path in paths {
        let source = fs::read_to_string(&path).unwrap();
        let document = parse_document(&source)
            .unwrap_or_else(|error| panic!("{} did not parse losslessly: {error}", path.display()));
        assert_eq!(document.as_bytes(), source.as_bytes(), "{}", path.display());

        let marks_non_live = source.lines().any(|line| {
            let compact = line.trim().to_ascii_lowercase().replace(' ', "");
            matches!(compact.as_str(), "#ptr=1" | "#beta=1" | "#classic=1")
        });
        if !marks_non_live {
            parse_simc_file(&source)
                .unwrap_or_else(|error| panic!("{} failed projection: {error}", path.display()));
            projected += 1;
        }
    }
    assert!(projected > 0, "no Retail Live profile was projected");
}

fn collect_simc_files(root: &Path, target: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_simc_files(&path, target);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "simc")
        {
            target.push(path);
        }
    }
}
