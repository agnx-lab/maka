/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

//! Complete display preparation from the original TUI change projection.
use crate::markdown::{
    diff::{Kind, Row},
    syntax,
};
use maka_ui_runtime::diff::{DiffContent, DiffKind, DiffPatch};
use similar::{ChangeTag, TextDiff};
use std::time::Duration;

fn invalid(message: &str) -> maka_ui_runtime::Error {
    maka_ui_runtime::Error::Invalid(message.into())
}
fn language(hint: &Option<String>) -> Option<&'static str> {
    hint.as_deref().and_then(syntax::language)
}
fn bounded(source: &str) -> bool {
    source.len() <= 8192 && source.lines().count() <= 256 && !source.contains('\0')
}
fn append(
    text: &mut String,
    rows: &mut Vec<Row>,
    line: &str,
    kind: Kind,
    language: Option<&'static str>,
) {
    let start = text.len();
    text.push_str(line);
    if !line.ends_with('\n') {
        text.push('\n');
    }
    rows.push(Row {
        source: start..text.len(),
        kind,
        language,
    });
}
pub(super) fn prepare(content: &DiffContent) -> maka_ui_runtime::Result<(String, Vec<Row>)> {
    let mut text = String::new();
    let mut rows = Vec::new();
    match content {
        DiffContent::Annotated {
            source,
            rows: annotations,
        } => {
            let mut end = 0;
            for row in annotations {
                let range = row.start as usize..row.end as usize;
                if range.start < end
                    || range.end < range.start
                    || source.get(range.clone()).is_none()
                {
                    return Err(invalid("invalid Diff source range"));
                }
                end = range.end;
                rows.push(Row {
                    source: range,
                    kind: match row.kind {
                        DiffKind::Removed => Kind::Removed,
                        DiffKind::Added => Kind::Added,
                        DiffKind::Context => Kind::Context,
                        DiffKind::Content => Kind::Content,
                    },
                    language: language(&row.language),
                });
            }
            text.push_str(source);
        }
        DiffContent::Comparison {
            before,
            after,
            language: hint,
        } => {
            if before.contains('\0') || after.contains('\0') {
                return Err(invalid("Diff comparison requires text without NUL"));
            }
            let language = language(hint);
            // At the deadline similar may produce a coarser valid diff.
            let diff = TextDiff::configure()
                .timeout(Duration::from_millis(2))
                .diff_lines(before, after);
            for change in diff.iter_all_changes() {
                let kind = match change.tag() {
                    ChangeTag::Delete => Kind::Removed,
                    ChangeTag::Insert => Kind::Added,
                    ChangeTag::Equal => Kind::Context,
                };
                append(&mut text, &mut rows, change.value(), kind, language);
            }
        }
        DiffContent::Patch {
            source,
            operation,
            language: hint,
        } => {
            let language = language(hint);
            // Validate the whole operation before emitting any colored rows.
            // This never applies a patch or looks up a local file.
            match operation {
                DiffPatch::Create if bounded(source) => {
                    if let Ok(created) = apply_patch::parse_create(source) {
                        for line in created.split_inclusive('\n') {
                            append(&mut text, &mut rows, line, Kind::Added, language);
                        }
                        return Ok((text, rows));
                    }
                }
                DiffPatch::Update
                    if bounded(source) && apply_patch::parse_update(source).is_ok() =>
                {
                    for raw in source.lines() {
                        let (line, kind) = match raw.as_bytes().first() {
                            Some(b'+') => (&raw[1..], Kind::Added),
                            Some(b'-') => (&raw[1..], Kind::Removed),
                            Some(b' ') => (&raw[1..], Kind::Context),
                            _ => (raw, Kind::Content),
                        };
                        append(&mut text, &mut rows, line, kind, language);
                    }
                    return Ok((text, rows));
                }
                _ => {}
            }
            text.push_str(source);
        }
    }
    Ok((text, rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparisons_share_context_and_keep_independent_syntax_sides() {
        let (text, rows) = prepare(&DiffContent::Comparison {
            before: "same\r\n/* removed\nend".into(),
            after: "same\r\nlet name = \"中😀é\";\nend".into(),
            language: Some("rs".into()),
        })
        .unwrap();
        assert_eq!(text, "same\r\n/* removed\nlet name = \"中😀é\";\nend\n");
        assert_eq!(
            rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
            [Kind::Context, Kind::Removed, Kind::Added, Kind::Context]
        );
        assert!(rows.iter().all(|r| r.language == Some("Rust")));
        for width in [4, 80] {
            let layout = crate::markdown::diff::render(&text, &rows, width, false).unwrap();
            assert_eq!(
                layout.text,
                crate::markdown::plain(&text, width).unwrap().text
            );
            assert!(
                layout
                    .lines
                    .iter()
                    .all(|line| line.line.width() <= usize::from(width))
            );
        }
    }
    #[test]
    fn complete_comparisons_and_annotations_preserve_text_and_invalid_ranges() {
        for (before, after) in [
            ("x".repeat(4096), "y".repeat(4097)),
            ("中".repeat(1500), "😀".repeat(1000)),
            ("x\n".repeat(128), "y\n".repeat(129)),
        ] {
            let (text, rows) = prepare(&DiffContent::Comparison {
                before: before.clone(),
                after: after.clone(),
                language: Some("unknown-language".repeat(20)),
            })
            .unwrap();
            assert_eq!(
                text,
                format!(
                    "{}{}{}{}",
                    before,
                    if before.ends_with('\n') { "" } else { "\n" },
                    after,
                    if after.ends_with('\n') { "" } else { "\n" }
                )
            );
            assert!(rows.iter().all(|row| row.language.is_none()));
        }
        assert!(
            prepare(&DiffContent::Comparison {
                before: "binary\0".into(),
                after: "new".into(),
                language: None
            })
            .is_err()
        );
        let source = "x".repeat(70000);
        let rows = (0..8193)
            .map(|index| maka_ui_runtime::diff::DiffRow {
                start: index,
                end: index + 1,
                kind: DiffKind::Content,
                language: None,
            })
            .chain(std::iter::once(maka_ui_runtime::diff::DiffRow {
                start: 69999,
                end: 70000,
                kind: DiffKind::Content,
                language: None,
            }))
            .collect();
        let (text, rows) = prepare(&DiffContent::Annotated {
            source: source.clone(),
            rows,
        })
        .unwrap();
        assert_eq!(text, source);
        assert_eq!(rows.len(), 8194);
        assert!(
            prepare(&DiffContent::Annotated {
                source: "😀".into(),
                rows: vec![maka_ui_runtime::diff::DiffRow {
                    start: 1,
                    end: 2,
                    kind: DiffKind::Content,
                    language: None
                }]
            })
            .is_err()
        );
        let (text, rows) = prepare(&DiffContent::Comparison {
            before: String::new(),
            after: String::new(),
            language: None,
        })
        .unwrap();
        assert!(text.is_empty() && rows.is_empty());
    }
    #[test]
    fn native_patch_grammar_preserves_prefix_literals_and_empty_creation() {
        for (source, expected, count) in [
            ("+", "", 0),
            ("+one\n+two\n+", "one\ntwo\n", 2),
            ("++literal\n", "+literal\n", 1),
        ] {
            let (text, rows) = prepare(&DiffContent::Patch {
                source: source.into(),
                operation: DiffPatch::Create,
                language: None,
            })
            .unwrap();
            assert_eq!(text, expected);
            assert_eq!(rows.len(), count);
            assert!(rows.iter().all(|r| r.kind == Kind::Added));
        }
        let (text, rows) = prepare(&DiffContent::Patch {
            source: "@@\r\n same\r\n-old\r\n++literal\r\n*** End of File\r\n".into(),
            operation: DiffPatch::Update,
            language: Some("rs".into()),
        })
        .unwrap();
        assert_eq!(text, "@@\nsame\nold\n+literal\n*** End of File\n");
        assert_eq!(
            rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
            [
                Kind::Content,
                Kind::Context,
                Kind::Removed,
                Kind::Added,
                Kind::Content
            ]
        );
    }
    #[test]
    fn invalid_or_unbounded_patches_remain_wholly_literal() {
        for source in [
            "@@\n-old\n+new\n*** Delete File: injected".into(),
            "@@\n-old\n+new\ninvalid".into(),
            "bad\0data".into(),
            "+x\n".repeat(257),
            format!("+{}", "x".repeat(8192)),
        ] {
            for operation in [DiffPatch::Create, DiffPatch::Update] {
                let (text, rows) = prepare(&DiffContent::Patch {
                    source: source.clone(),
                    operation,
                    language: None,
                })
                .unwrap();
                assert_eq!(text, source);
                assert!(rows.is_empty());
            }
        }
        let source = "界".repeat(21846);
        let (text, rows) = prepare(&DiffContent::Patch {
            source: source.clone(),
            operation: DiffPatch::Create,
            language: None,
        })
        .unwrap();
        assert_eq!(text, source);
        assert!(rows.is_empty());
    }
}
