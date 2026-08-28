use serde::{Deserialize, Serialize};

use crate::{MAX_INPUT_BYTES, MAX_LINE_BYTES, ParseError, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectiveOperator {
    Set,
    Append,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimcDirective {
    pub key: String,
    pub operator: DirectiveOperator,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum SimcLineKind {
    Blank,
    Comment(String),
    Directive(SimcDirective),
    BareInput(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimcLine {
    pub number: usize,
    /// Exact line content, excluding its line ending.
    pub raw: String,
    /// Exact `\n`, `\r\n`, `\r`, or empty final line ending.
    pub ending: String,
    pub kind: SimcLineKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimcDocument {
    source: String,
    pub lines: Vec<SimcLine>,
}

impl SimcDocument {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.source.as_bytes()
    }

    pub fn into_source(self) -> String {
        self.source
    }
}

pub fn parse_document(input: &str) -> Result<SimcDocument> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(ParseError::InputTooLarge);
    }
    if input.as_bytes().contains(&0) {
        return Err(ParseError::Invalid {
            line: 1,
            message: "NUL bytes are not allowed".into(),
        });
    }

    let bytes = input.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut number = 1;
    while start < bytes.len() {
        let mut end = start;
        while end < bytes.len() && bytes[end] != b'\n' && bytes[end] != b'\r' {
            end += 1;
        }
        if end - start > MAX_LINE_BYTES {
            return Err(ParseError::Invalid {
                line: number,
                message: "line exceeds the 16 KiB limit".into(),
            });
        }
        let ending_end = if end < bytes.len() && bytes[end] == b'\r' {
            if end + 1 < bytes.len() && bytes[end + 1] == b'\n' {
                end + 2
            } else {
                end + 1
            }
        } else if end < bytes.len() {
            end + 1
        } else {
            end
        };
        let raw = input[start..end].to_owned();
        let ending = input[end..ending_end].to_owned();
        lines.push(SimcLine {
            number,
            kind: classify_line(&raw),
            raw,
            ending,
        });
        start = ending_end;
        number += 1;
    }

    Ok(SimcDocument {
        source: input.to_owned(),
        lines,
    })
}

fn classify_line(raw: &str) -> SimcLineKind {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return SimcLineKind::Blank;
    }
    if let Some(comment) = trimmed.strip_prefix('#') {
        return SimcLineKind::Comment(comment.to_owned());
    }
    if let Some((key, operator, value)) = split_directive(trimmed) {
        return SimcLineKind::Directive(SimcDirective {
            key: key.trim().to_owned(),
            operator,
            value: value.trim().to_owned(),
        });
    }
    SimcLineKind::BareInput(trimmed.to_owned())
}

fn split_directive(line: &str) -> Option<(&str, DirectiveOperator, &str)> {
    let equals = line.find('=')?;
    let (key_end, operator) = if equals > 0 && line.as_bytes()[equals - 1] == b'+' {
        (equals - 1, DirectiveOperator::Append)
    } else {
        (equals, DirectiveOperator::Set)
    };
    Some((&line[..key_end], operator, &line[equals + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_mixed_endings_unicode_comments_and_duplicates() {
        let source = "# 한글\r\niterations=1\nactions+=/spell,if=foo==bar\runknown = \"a=b\"";
        let document = parse_document(source).unwrap();
        assert_eq!(document.as_bytes(), source.as_bytes());
        assert_eq!(document.lines[0].ending, "\r\n");
        assert_eq!(document.lines[1].ending, "\n");
        assert_eq!(document.lines[2].ending, "\r");
        assert_eq!(document.lines[3].ending, "");
        assert!(matches!(
            &document.lines[2].kind,
            SimcLineKind::Directive(SimcDirective {
                operator: DirectiveOperator::Append,
                ..
            })
        ));
    }

    #[test]
    fn preserves_templates_and_bare_include_lines_without_interpreting_them() {
        let source = "$(name)=value\nother.simc\n";
        let document = parse_document(source).unwrap();
        assert!(matches!(document.lines[0].kind, SimcLineKind::Directive(_)));
        assert!(matches!(document.lines[1].kind, SimcLineKind::BareInput(_)));
        assert_eq!(document.source(), source);
    }
}
