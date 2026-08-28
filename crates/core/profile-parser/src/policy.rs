use serde::{Deserialize, Serialize};

use crate::{SimcDocument, SimcLineKind, parse_class};

const MAX_DIAGNOSTICS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityCategory {
    SupportedEditable,
    PreservedNotEditable,
    ExecutionBlocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompatibilityDiagnostic {
    pub line: usize,
    pub key: Option<String>,
    pub category: CompatibilityCategory,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompatibilityReport {
    pub supported_editable: usize,
    pub preserved_not_editable: usize,
    pub execution_blocked: usize,
    pub diagnostics: Vec<CompatibilityDiagnostic>,
}

impl CompatibilityReport {
    pub fn first_blocker(&self) -> Option<&CompatibilityDiagnostic> {
        self.diagnostics
            .iter()
            .find(|entry| entry.category == CompatibilityCategory::ExecutionBlocked)
    }
}

pub fn analyze_compatibility(document: &SimcDocument) -> CompatibilityReport {
    let mut report = CompatibilityReport::default();
    let mut actor_lines = Vec::new();

    for line in &document.lines {
        match &line.kind {
            SimcLineKind::Blank => {}
            SimcLineKind::Comment(comment) => {
                let compact = comment.trim().to_ascii_lowercase().replace(' ', "");
                if matches!(
                    compact.as_str(),
                    "ptr=1" | "beta=1" | "classic=1" | "wowptr" | "wowbeta" | "wowclassic"
                ) {
                    push(
                        &mut report,
                        line.number,
                        None,
                        CompatibilityCategory::ExecutionBlocked,
                        "comment metadata marks a Classic, PTR, or Beta profile".into(),
                    );
                }
            }
            SimcLineKind::BareInput(value) => push(
                &mut report,
                line.number,
                None,
                CompatibilityCategory::ExecutionBlocked,
                format!("file include is not allowed in local product mode: {value}"),
            ),
            SimcLineKind::Directive(directive) => {
                let key = directive.key.as_str();
                if parse_class(key).is_some() {
                    actor_lines.push(line.number);
                    report.supported_editable += 1;
                    continue;
                }
                if let Some(reason) = blocked_reason(key, &directive.value) {
                    push(
                        &mut report,
                        line.number,
                        Some(key.to_owned()),
                        CompatibilityCategory::ExecutionBlocked,
                        reason,
                    );
                } else if is_typed_key(key) {
                    report.supported_editable += 1;
                } else {
                    push(
                        &mut report,
                        line.number,
                        Some(key.to_owned()),
                        CompatibilityCategory::PreservedNotEditable,
                        "preserved exactly and delegated to SimulationCraft".into(),
                    );
                }
            }
        }
    }

    for line in actor_lines.into_iter().skip(1) {
        push(
            &mut report,
            line,
            None,
            CompatibilityCategory::ExecutionBlocked,
            "multiple player actors are not supported in local product mode".into(),
        );
    }
    report
}

fn push(
    report: &mut CompatibilityReport,
    line: usize,
    key: Option<String>,
    category: CompatibilityCategory,
    reason: String,
) {
    match category {
        CompatibilityCategory::SupportedEditable => report.supported_editable += 1,
        CompatibilityCategory::PreservedNotEditable => report.preserved_not_editable += 1,
        CompatibilityCategory::ExecutionBlocked => report.execution_blocked += 1,
    }
    if report.diagnostics.len() < MAX_DIAGNOSTICS {
        report.diagnostics.push(CompatibilityDiagnostic {
            line,
            key,
            category,
            reason,
        });
    } else if category == CompatibilityCategory::ExecutionBlocked {
        report.diagnostics.pop();
        report.diagnostics.push(CompatibilityDiagnostic {
            line,
            key,
            category,
            reason,
        });
    }
}

fn blocked_reason(key: &str, value: &str) -> Option<String> {
    let lower_key = key.to_ascii_lowercase();
    let lower_value = value.to_ascii_lowercase();
    if matches!(lower_key.as_str(), "ptr" | "beta" | "classic")
        && !matches!(lower_value.as_str(), "0" | "false" | "disabled")
    {
        return Some("Classic, PTR and Beta execution is unsupported".into());
    }
    if lower_key == "game_channel" && matches!(lower_value.as_str(), "classic" | "ptr" | "beta") {
        return Some("Classic, PTR and Beta execution is unsupported".into());
    }
    if lower_key == "input" || lower_key == "path" {
        return Some("file include and search paths are not allowed".into());
    }
    if matches!(
        lower_key.as_str(),
        "json" | "json2" | "html" | "output" | "save"
    ) || lower_key.starts_with("save_")
    {
        return Some("output paths are controlled by SimShredder".into());
    }
    if matches!(
        lower_key.as_str(),
        "armory" | "local_json" | "proxy" | "apikey" | "spell_query" | "item_query"
    ) {
        return Some("network, credential, and query directives are not allowed".into());
    }
    if lower_key == "copy" || lower_key == "enemy" || lower_key.starts_with("profileset") {
        return Some(
            "multi-actor and profileset inputs are not executable in this workflow".into(),
        );
    }
    None
}

fn is_typed_key(key: &str) -> bool {
    parse_class(key).is_some()
        || crate::parse_gear_slot(key).is_some()
        || matches!(
            key,
            "level"
                | "race"
                | "region"
                | "server"
                | "role"
                | "spec"
                | "iterations"
                | "fixed_time"
                | "max_time"
                | "vary_combat_length"
                | "desired_targets"
                | "fight_style"
                | "threads"
                | "seed"
                | "report_details"
        )
        || crate::is_talent_key(key)
        || crate::is_scalar_option(key)
        || key.starts_with("actions")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_document;

    #[test]
    fn distinguishes_preserved_unknowns_from_blocked_execution_features() {
        let document = parse_document(
            "warrior=One\nunknown_future_option=1\nraid_events+=/adds,count=2\ninput=other.simc\n",
        )
        .unwrap();
        let report = analyze_compatibility(&document);
        assert_eq!(report.execution_blocked, 1);
        assert_eq!(report.preserved_not_editable, 2);
        assert_eq!(report.first_blocker().unwrap().line, 4);
    }

    #[test]
    fn blocks_every_external_io_boundary_with_line_and_key() {
        for (source, key) in [
            ("input=other.simc\n", Some("input")),
            ("path=../profiles\n", Some("path")),
            ("json2=../../result.json\n", Some("json2")),
            ("armory=kr,realm,name\n", Some("armory")),
            ("proxy=http://127.0.0.1\n", Some("proxy")),
            ("other.simc\n", None),
            ("C:\\profiles\\other.simc\n", None),
        ] {
            let report = analyze_compatibility(&parse_document(source).unwrap());
            let blocker = report.first_blocker().unwrap();
            assert_eq!(blocker.line, 1);
            assert_eq!(blocker.key.as_deref(), key);
        }
    }

    #[test]
    fn blocks_multi_actor_and_profileset_but_allows_raid_events() {
        let source = "warrior=One\nraid_events+=/movement,duration=1\nprofileset.foo=talents=abc\n";
        let report = analyze_compatibility(&parse_document(source).unwrap());
        assert_eq!(report.execution_blocked, 1);
        assert_eq!(report.preserved_not_editable, 1);

        let actors = analyze_compatibility(&parse_document("warrior=One\nmage=Two\n").unwrap());
        assert_eq!(actors.execution_blocked, 1);
        assert_eq!(actors.first_blocker().unwrap().line, 2);
    }
}
