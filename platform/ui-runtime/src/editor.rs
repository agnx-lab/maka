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

//! Shared native editing protocol; platform adapters own all mutable text.
mod component;
pub use component::{Props, reduce};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorCommand {
    /// Correlation with the original native editor, never a resource grant.
    #[schemars(length(min = 1, max = 128))]
    pub owner: String,
    #[schemars(range(min = 1, max = 9007199254740991u64))]
    pub sequence: u64,
    #[schemars(range(max = 9007199254740991u64))]
    pub based_on: u64,
    pub edit: EditorEdit,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditorEdit {
    /// Replace a UTF-16 range in the current revision, preserving its suffix.
    ReplaceRange {
        range: TextRange,
        text: String,
    },
    Replace {
        text: String,
    },
    Reset {
        text: String,
    },
    /// Continue initialization after Reset, without creating undo history.
    /// The same native owner retains the complete buffer.
    Append {
        text: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Ordered UTF-16 code-unit offsets, shared by selections and range edits.
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}
impl TextRange {
    /// Convert protocol UTF-16 offsets to UTF-8, rejecting split graphemes.
    pub fn utf8_range(&self, text: &str) -> Option<std::ops::Range<usize>> {
        use unicode_segmentation::UnicodeSegmentation;
        if self.start > self.end {
            return None;
        }
        let mut offset = 0;
        let mut start = None;
        let mut end = None;
        for (byte, grapheme) in text.grapheme_indices(true) {
            if offset == self.start {
                start = Some(byte);
            }
            if offset == self.end {
                end = Some(byte);
            }
            offset += grapheme.encode_utf16().count();
        }
        if offset == self.start {
            start = Some(text.len());
        }
        if offset == self.end {
            end = Some(text.len());
        }
        Some(start?..end?)
    }
}
impl EditorEdit {
    /// Prospective native buffer, checked before mutation.
    pub fn replacement(&self, current: &str) -> Option<String> {
        match self {
            Self::ReplaceRange { range, text } => {
                let range = range.utf8_range(current)?;
                let mut value = current.to_owned();
                value.replace_range(range, text);
                Some(value)
            }
            Self::Append { text } => Some(format!("{current}{text}")),
            Self::Replace { text } | Self::Reset { text } => Some(text.clone()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditorTextRef {
    pub owner: String,
    pub revision: u64,
    pub bytes: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditorSnapshot {
    pub text: EditorTextRef,
    pub selection: TextRange,
    pub reversed: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(schema_with = "nullable_schema::<TextRange>")]
    pub marked: Option<TextRange>,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditorConflict {
    Persistence,
    Owner,
    Representation,
    Revision,
    Composition,
    Sequence,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditorOutcome {
    Applied,
    Conflict { reason: EditorConflict },
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditorReceipt {
    pub owner: String,
    pub sequence: u64,
    pub revision: u64,
    pub outcome: EditorOutcome,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditorObservation {
    pub revision: u64,
    pub snapshot: EditorSnapshot,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(schema_with = "nullable_schema::<EditorReceipt>")]
    pub receipt: Option<EditorReceipt>,
}
pub fn observation_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(EditorObservation))
        .expect("editor observation schema")
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditorCapture {
    Current,
    AtRevision {
        #[schemars(length(min = 1, max = 128))]
        owner: String,
        revision: u64,
    },
}
pub use super::text::text_resource_type;

fn nullable_schema<T: JsonSchema>(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    generator.subschema_for::<Option<T>>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_replacement_uses_utf16_and_rejects_split_or_invalid_graphemes() {
        let current = "🙂e\u{301}/old suffix";
        assert_eq!(
            TextRange { start: 5, end: 8 }.utf8_range(current),
            Some(8..11)
        );
        let edit = EditorEdit::ReplaceRange {
            range: TextRange { start: 5, end: 8 },
            text: "new".into(),
        };
        assert_eq!(
            edit.replacement(current).as_deref(),
            Some("🙂e\u{301}/new suffix")
        );
        for range in [
            TextRange { start: 1, end: 2 },
            TextRange { start: 2, end: 3 },
            TextRange { start: 8, end: 5 },
            TextRange { start: 99, end: 99 },
        ] {
            assert!(range.utf8_range(current).is_none());
        }
        assert_eq!(TextRange { start: 0, end: 0 }.utf8_range(""), Some(0..0));
        let text = "中".repeat(30_000);
        assert!(
            EditorEdit::ReplaceRange {
                range: TextRange { start: 0, end: 0 },
                text: text.clone(),
            }
            .replacement(current)
            .as_deref()
                == Some(format!("{text}{current}").as_str())
        );
    }
}
