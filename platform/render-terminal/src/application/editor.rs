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

use super::NativeInput;
use crate::editor::{Editor, Geometry};
use maka_ui_runtime::editor::*;
use serde_json::{Value, json};
use std::{ops::Range, sync::Arc};
pub(super) struct Owner {
    pub editor: Editor,
    pub redraw: bool,
    pub clipboard: Option<super::textarea::paste::Paste>,
    pub presented: Option<Geometry>,
    pub focus: maka_ui_runtime::focus::FocusOnReveal,
    id: String,
    revision: u64,
    last_edit: u64,
    last_selection: (Range<usize>, bool),
    pub last_request: Option<EditorCommand>,
    sequence: u64,
    receipt: Option<EditorReceipt>,
    observed_receipt: Option<EditorReceipt>,
    pub announced: bool,
}
impl Owner {
    pub fn invalidate_geometry(&mut self) {
        self.cancel_paste();
        self.editor.invalidate_geometry();
        self.presented = None;
    }
    pub fn clipboard(&mut self, context: super::NativeContext) {
        self.clipboard = Some(super::textarea::paste::Paste::new(context));
    }
    pub fn paste(&mut self, spaces: bool) {
        if let Some(paste) = &mut self.clipboard {
            paste.text(&self.editor, spaces);
        }
    }
    pub fn cancel_paste(&mut self) {
        if let Some(paste) = &mut self.clipboard {
            paste.cancel_text();
        }
    }
    pub fn poll_paste(&mut self, enabled: bool) {
        let before = self.editor.error;
        if let Some(paste) = &mut self.clipboard {
            paste.poll_text(&mut self.editor, enabled);
        }
        self.redraw |= before != self.editor.error;
    }
    pub fn new(mut editor: Editor, initial_text: &str) -> Self {
        editor.insert(initial_text);
        editor.clear_history();
        let last_edit = editor.revision();
        let last_selection = (editor.selection_range(), editor.selection_reversed());
        Self {
            editor,
            clipboard: None,
            redraw: false,
            presented: None,
            focus: Default::default(),
            id: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            last_edit,
            last_selection,
            last_request: None,
            sequence: 0,
            receipt: None,
            observed_receipt: None,
            announced: false,
        }
    }
    pub fn refresh(&mut self) -> bool {
        let edit = self.editor.revision();
        let selection = (
            self.editor.selection_range(),
            self.editor.selection_reversed(),
        );
        if edit != self.last_edit || selection != self.last_selection {
            self.last_edit = edit;
            self.last_selection = selection;
            self.revision += 1;
            return true;
        }
        false
    }
    pub fn snapshot(&self) -> EditorSnapshot {
        let range = self.editor.selection_range();
        EditorSnapshot {
            text: EditorTextRef {
                owner: self.id.clone(),
                revision: self.revision,
                bytes: self.editor.text().len(),
            },
            selection: TextRange {
                start: self.editor.text()[..range.start].encode_utf16().count(),
                end: self.editor.text()[..range.end].encode_utf16().count(),
            },
            reversed: self.editor.selection_reversed(),
            marked: None,
        }
    }
    pub fn current_observation(&self) -> EditorObservation {
        EditorObservation {
            revision: self.revision,
            snapshot: self.snapshot(),
            receipt: self.receipt.clone(),
        }
    }
    pub fn observation(&mut self) -> NativeInput {
        let observation = self.current_observation();
        let replaceable = self.receipt == self.observed_receipt;
        self.observed_receipt.clone_from(&self.receipt);
        NativeInput {
            replaceable,
            key: "observe".into(),
            value: serde_json::to_value(observation).expect("native editor observation"),
        }
    }
    pub fn reject(&mut self, command: &EditorCommand, reason: EditorConflict) {
        self.last_request = Some(command.clone());
        self.receipt = Some(EditorReceipt {
            owner: command.owner.clone(),
            sequence: command.sequence,
            revision: self.revision,
            outcome: EditorOutcome::Conflict { reason },
        });
    }
    pub fn apply(&mut self, command: &EditorCommand) {
        self.last_request = Some(command.clone());
        let outcome = if command.owner != self.id {
            EditorOutcome::Conflict {
                reason: EditorConflict::Owner,
            }
        } else if command.sequence <= self.sequence {
            EditorOutcome::Conflict {
                reason: EditorConflict::Sequence,
            }
        } else {
            self.sequence = command.sequence;
            if command.based_on != self.revision {
                EditorOutcome::Conflict {
                    reason: EditorConflict::Revision,
                }
            } else {
                let value = command.edit.replacement(self.editor.text());
                if value
                    .as_ref()
                    .is_none_or(|value| !self.editor.accepts_text(value))
                {
                    EditorOutcome::Conflict {
                        reason: EditorConflict::Representation,
                    }
                } else {
                    let value = value.expect("validated editor replacement");
                    match &command.edit {
                        EditorEdit::ReplaceRange { range, text } => {
                            let range = range
                                .utf8_range(self.editor.text())
                                .expect("validated range");
                            self.editor.replace_range(range, text);
                        }
                        EditorEdit::Replace { text } => {
                            self.editor.select_all();
                            self.editor.insert(text);
                        }
                        EditorEdit::Reset { .. } | EditorEdit::Append { .. } => {
                            self.editor.clear();
                            self.editor.insert(&value);
                            self.editor.clear_history();
                        }
                    }
                    self.refresh();
                    EditorOutcome::Applied
                }
            }
        };
        self.receipt = Some(EditorReceipt {
            owner: command.owner.clone(),
            sequence: command.sequence,
            revision: self.revision,
            outcome,
        });
    }
}
impl Owner {
    pub fn capture(
        &self,
        target: &maka_ui_runtime::actions::CaptureTarget,
    ) -> maka_ui_runtime::Result<Arc<dyn maka_capabilities::call::ReadResource>> {
        if target.method() != "snapshot" {
            return Err(maka_ui_runtime::Error::Invalid(
                "unknown terminal editor capture".into(),
            ));
        }
        let owner = self;
        let capture: EditorCapture = serde_json::from_value(target.input().clone())
            .map_err(|e| maka_ui_runtime::Error::Invalid(e.to_string()))?;
        if matches!(capture,EditorCapture::AtRevision {owner:ref expected,revision} if expected!=&owner.id || revision!=owner.revision)
        {
            return Err(maka_ui_runtime::Error::Invalid(
                "editor revision changed".into(),
            ));
        }
        Ok(Arc::new(Snapshot {
            owner: owner.id.clone(),
            text: owner.editor.text().to_owned(),
            revision: owner.revision,
        }))
    }
}

pub(super) fn error_message(error: &str) -> &str {
    match error {
        "input-too-large" | "composer-too-large" => "Text exceeds the input limit",
        "input-control" => "This input requires a single line of text",
        "composer-control" => "Text contains unsupported control characters",
        other => other,
    }
}

pub(super) struct Snapshot {
    pub owner: String,
    pub text: String,
    pub revision: u64,
}
impl maka_capabilities::call::ReadResource for Snapshot {
    fn encoding(&self) -> maka_capabilities::call::ResourceEncoding {
        maka_capabilities::call::ResourceEncoding::Utf8
    }
    fn resource_type(&self) -> maka_plugin_core::contract::ResourceType {
        text_resource_type()
    }
    fn bytes(&self) -> usize {
        self.text.len()
    }
    fn metadata(&self) -> Value {
        json!({"owner":self.owner,"revision":self.revision,"bytes":self.text.len()})
    }
    fn read(&self, range: Range<usize>) -> Result<Vec<u8>, String> {
        self.text
            .get(range)
            .map(|text| text.as_bytes().to_vec())
            .ok_or_else(|| "invalid editor UTF-8 range".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_command_preserves_unicode_suffix_caret_and_one_undo_step() {
        let original = "🙂e\u{301}/old suffix";
        let mut owner = Owner::new(Editor::bounded(256, "too-large"), original);
        let command = EditorCommand {
            owner: owner.id.clone(),
            sequence: 1,
            based_on: owner.revision,
            edit: EditorEdit::ReplaceRange {
                range: TextRange { start: 5, end: 8 },
                text: "new技能".into(),
            },
        };
        owner.apply(&command);
        assert_eq!(
            owner.receipt.as_ref().unwrap().outcome,
            EditorOutcome::Applied
        );
        assert_eq!(owner.editor.text(), "🙂e\u{301}/new技能 suffix");
        assert_eq!(owner.snapshot().selection, TextRange { start: 10, end: 10 });
        let mut rejected = command.clone();
        rejected.sequence = 2;
        owner.apply(&rejected);
        assert_eq!(
            owner.receipt.as_ref().unwrap().outcome,
            EditorOutcome::Conflict {
                reason: EditorConflict::Revision,
            }
        );
        rejected.sequence = 3;
        rejected.based_on = owner.revision;
        rejected.edit = EditorEdit::ReplaceRange {
            range: TextRange { start: 2, end: 3 },
            text: "bad".into(),
        };
        owner.apply(&rejected);
        assert_eq!(
            owner.receipt.as_ref().unwrap().outcome,
            EditorOutcome::Conflict {
                reason: EditorConflict::Representation,
            }
        );
        owner.editor.key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('z'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(owner.editor.text(), original);
    }

    #[test]
    fn initialization_keeps_one_native_owner_and_rejects_stale_append() {
        let mut owner = Owner::new(Editor::default(), "");
        let identity = owner.id.clone();
        let chunk = "中🌟".repeat(9000);
        for sequence in 1..=8 {
            owner.apply(&EditorCommand {
                owner: identity.clone(),
                sequence,
                based_on: owner.revision,
                edit: if sequence == 1 {
                    EditorEdit::Reset {
                        text: chunk.clone(),
                    }
                } else {
                    EditorEdit::Append {
                        text: chunk.clone(),
                    }
                },
            });
            assert_eq!(
                owner.receipt.as_ref().unwrap().outcome,
                EditorOutcome::Applied
            );
            assert_eq!(owner.editor.text(), chunk.repeat(sequence as usize));
        }
        let before = owner.editor.text().to_owned();
        owner.editor.key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('z'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(
            owner.editor.text(),
            before,
            "initialization does not create partial-document undo steps"
        );
        owner.apply(&EditorCommand {
            owner: identity.clone(),
            sequence: 9,
            based_on: 0,
            edit: EditorEdit::Append {
                text: "stale".into(),
            },
        });
        assert_eq!(
            owner.receipt.as_ref().unwrap().outcome,
            EditorOutcome::Conflict {
                reason: EditorConflict::Revision
            }
        );
        owner.apply(&EditorCommand {
            owner: identity,
            sequence: 10,
            based_on: owner.revision,
            edit: EditorEdit::Append {
                text: "x".repeat(65537),
            },
        });
        assert_eq!(
            owner.receipt.as_ref().unwrap().outcome,
            EditorOutcome::Applied
        );
        assert!(owner.editor.text() == format!("{before}{}", "x".repeat(65537)));
    }
}
