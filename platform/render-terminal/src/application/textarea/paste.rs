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

//! One physically admitted batch, read serially through the original file custody.
use super::super::{NativeContext, NativeInput, file::paths};
use super::clipboard::{self, Content, Paths};
use crate::editor::{Editor, PasteTarget};
use maka_ui_runtime::{
    budget::{Kind, Permit},
    file::{FileCapture, FileError, FilePhase, FileSelection, FileSnapshot, read_selected},
    textarea::{FilePasteAction, FilePasteCommand, FilePasteObservation, FilePastePolicy},
};
use std::sync::{Arc, mpsc};

enum Source {
    Paths(Paths),
    Clipboard {
        allow_files: bool,
        target: PasteTarget,
    },
    Snapshot,
    Failed,
    Empty,
}
impl Source {
    fn remaining(&self) -> usize {
        match self {
            Self::Paths(paths) => paths.items.len(),
            Self::Empty => 0,
            _ => 1,
        }
    }
}
struct Batch {
    context: String,
    source: Source,
    staged: bool,
    _memory: Permit,
}
enum Output {
    File(Arc<FileSnapshot>),
    Clipboard(Content),
}
struct Read {
    revision: u64,
    receiver: mpsc::Receiver<Result<Output, FileError>>,
}
pub(in crate::application) struct Paste {
    context: NativeContext,
    selection: FileSelection,
    batch: Option<Batch>,
    read: Option<Read>,
    dirty: bool,
    text_target: Option<(PasteTarget, bool)>,
    text_read: bool,
    text_error: Option<FileError>,
    #[cfg(test)]
    clipboard: Option<String>,
}
impl Paste {
    pub fn will_accept(&self, request: Option<&str>) -> bool {
        self.selection.will_accept(request)
    }
    pub fn pending(&self) -> bool {
        self.batch.is_some() || self.read.is_some()
    }
    pub fn new(context: NativeContext) -> Self {
        Self {
            context,
            selection: FileSelection::default(),
            batch: None,
            read: None,
            dirty: false,
            text_target: None,
            text_read: false,
            text_error: None,
            #[cfg(test)]
            clipboard: None,
        }
    }
    #[cfg(test)]
    pub fn set_clipboard_text(&mut self, text: String) {
        self.clipboard = Some(text);
    }
    pub fn text(&mut self, editor: &Editor, spaces: bool) {
        if self.pending() {
            return;
        }
        self.text_target = Some((editor.paste_target(), spaces));
        self.text_read = true;
        self.text_error = None;
        let budget = self.context.budget.clone();
        #[cfg(test)]
        if let Some(value) = self.clipboard.take() {
            return self.spawn(Box::new(move || {
                let memory = budget.reserve(Kind::Encoded, value.capacity())?;
                Ok(Output::Clipboard(Content::Text(clipboard::Text {
                    value,
                    _memory: memory,
                })))
            }));
        }
        self.spawn(Box::new(move || {
            clipboard::read_text(&budget).map(Output::Clipboard)
        }));
    }
    pub fn cancel_text(&mut self) {
        self.text_target = None;
    }
    pub fn poll_text(&mut self, editor: &mut Editor, enabled: bool) {
        if !enabled {
            self.cancel_text();
        }
        if let Some(error) = self.text_error.take() {
            editor.error = Some(match error {
                FileError::BudgetExhausted | FileError::LimitExceeded => {
                    "Clipboard text exceeds the input limit"
                }
                _ => "Clipboard unavailable",
            });
        }
        if !self.text_read {
            return;
        }
        let Some(read) = &self.read else {
            return;
        };
        let result = match read.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => Err(FileError::WorkerFailed),
        };
        self.read = None;
        self.text_read = false;
        let Some((target, spaces)) = self.text_target.take() else {
            return;
        };
        match result {
            Ok(Output::Clipboard(Content::Text(text))) => {
                let value = if spaces {
                    text.value
                        .replace("\r\n", " ")
                        .replace(['\r', '\n', '\t'], " ")
                } else {
                    text.value.clone()
                };
                if !editor.paste_at(target, &value) {
                    editor.error = Some("Paste rejected: input changed or text is invalid");
                }
            }
            _ => editor.error = Some("Clipboard unavailable"),
        }
    }
    /// The whole paste must classify as local paths. Otherwise native text keeps it.
    pub fn stage(&mut self, text: &str, policy: &FilePastePolicy) -> Option<NativeInput> {
        if self.pending() {
            return None;
        }
        let paths = clipboard::admit_paths(
            paths::parse(text)?,
            policy.allow_files,
            &self.context.budget,
        )
        .ok()?;
        self.stage_source(Source::Paths(paths), policy)
    }
    pub fn stage_clipboard(
        &mut self,
        editor: &Editor,
        policy: &FilePastePolicy,
    ) -> Option<NativeInput> {
        self.stage_source(
            Source::Clipboard {
                allow_files: policy.allow_files,
                target: editor.paste_target(),
            },
            policy,
        )
    }
    fn stage_source(&mut self, source: Source, policy: &FilePastePolicy) -> Option<NativeInput> {
        let replace_failed = matches!(source, Source::Clipboard { .. })
            && self.read.is_none()
            && self
                .batch
                .as_ref()
                .is_some_and(|batch| matches!(batch.source, Source::Failed));
        if self.pending() && !replace_failed {
            return None;
        }
        let memory = self
            .context
            .budget
            .reserve(Kind::Encoded, policy.context.len())
            .ok()?;
        if replace_failed {
            // Preserve the old failure if new admission cannot reserve memory.
            // Only a physical gesture can replace the failed clipboard read.
            self.selection.clear();
            self.batch = None;
            self.dirty = false;
        }
        let choose = self.selection.stage()?;
        self.batch = Some(Batch {
            context: policy.context.clone(),
            source,
            staged: true,
            _memory: memory,
        });
        Some(NativeInput {
            replaceable: false,
            key: "file_choose".into(),
            value: serde_json::to_value(choose).unwrap(),
        })
    }
    pub fn suspend(&mut self) {
        self.selection.cancel_intent();
        if self.batch.as_ref().is_some_and(|batch| batch.staged) {
            self.batch = None;
        }
    }
    pub fn commit(
        &mut self,
        request: Option<&str>,
        command: Option<&FilePasteCommand>,
        enabled: bool,
    ) {
        if !enabled {
            self.suspend();
        }
        if self.selection.accept(request) {
            self.batch
                .as_mut()
                .expect("admitted original file paste")
                .staged = false;
            self.start();
            self.dirty = true;
        }
        let Some(command) = command else { return };
        let observation = self.selection.observation();
        let Some(batch) = self.batch.as_mut() else {
            return;
        };
        if command.selection.owner != observation.owner
            || command.selection.revision != observation.revision
        {
            return;
        }
        match command.action {
            FilePasteAction::Cancel => {
                batch.source = Source::Empty;
                self.selection.clear();
            }
            FilePasteAction::Next
                if matches!(observation.phase, FilePhase::Ready | FilePhase::Failed) =>
            {
                match &mut batch.source {
                    Source::Paths(paths) => {
                        paths.items.pop_front();
                    }
                    _ => batch.source = Source::Empty,
                }
                self.selection.clear();
                if batch.source.remaining() != 0 {
                    self.advance();
                }
            }
            FilePasteAction::Retry
                if observation.phase == FilePhase::Failed
                    && matches!(batch.source, Source::Paths(_)) =>
            {
                self.advance()
            }
            _ => return,
        }
        self.dirty = true;
    }
    fn advance(&mut self) {
        // Guest dispositions can continue original paths, never reread a clipboard.
        let choose = self.selection.stage().expect("settled file selection");
        assert!(self.selection.accept(Some(&choose.request)));
        self.start();
    }
    fn start(&mut self) {
        let budget = self.context.budget.clone();
        let source = &self.batch.as_ref().unwrap().source;
        let read: Box<dyn FnOnce() -> Result<Output, FileError> + Send> = match source {
            Source::Paths(paths) => {
                let path = paths.items.front().unwrap().clone();
                Box::new(move || read_selected(&path, &budget).map(Output::File))
            }
            Source::Clipboard { allow_files, .. } => {
                let allow_files = *allow_files;
                #[cfg(test)]
                if let Some(value) = self.clipboard.take() {
                    return self.spawn(Box::new(move || {
                        let memory = budget.reserve(Kind::Encoded, value.capacity())?;
                        Ok(Output::Clipboard(Content::Text(clipboard::Text {
                            value,
                            _memory: memory,
                        })))
                    }));
                }
                Box::new(move || clipboard::read(allow_files, &budget).map(Output::Clipboard))
            }
            _ => return,
        };
        self.spawn(read);
    }
    fn spawn(&mut self, read: Box<dyn FnOnce() -> Result<Output, FileError> + Send>) {
        let mut ticket = match self.context.resources.reserve() {
            Ok(ticket) => ticket,
            Err(_) => {
                self.fail(FileError::WorkerFailed);
                return;
            }
        };
        let revision = self.selection.observation().revision;
        let (sender, receiver) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("native-file-paste".into())
            .spawn(move || {
                ticket.start();
                let (result, cleanup) =
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(read)) {
                        Ok(result) => (result, Ok(())),
                        Err(_) => (
                            Err(FileError::WorkerFailed),
                            Err("native paste worker panicked".into()),
                        ),
                    };
                let _ = sender.send(result);
                ticket.complete(cleanup);
            }) {
            Ok(_) => self.read = Some(Read { revision, receiver }),
            Err(_) => self.fail(FileError::WorkerFailed),
        }
    }
    fn fail(&mut self, error: FileError) {
        if self.text_read {
            self.text_read = false;
            self.text_target = None;
            self.text_error = Some(error);
            return;
        }
        if let Some(batch) = &mut self.batch
            && matches!(batch.source, Source::Clipboard { .. })
        {
            batch.source = Source::Failed;
        }
        self.selection.settle(Err(error));
        self.dirty = true;
    }
    pub fn poll(&mut self, editor: &mut Editor, current_context: Option<&str>) {
        let Some(read) = &self.read else { return };
        let result = match read.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Disconnected) => Err(FileError::WorkerFailed),
            Err(mpsc::TryRecvError::Empty) => return,
        };
        let revision = read.revision;
        self.read = None;
        if self.selection.observation().revision != revision {
            return;
        }
        let batch = self.batch.as_mut().expect("original admitted batch");
        match result {
            Err(error) => self.fail(error),
            Ok(Output::File(file)) => self.selection.settle(Ok(Some(file))),
            Ok(Output::Clipboard(Content::Image(file))) => {
                batch.source = Source::Snapshot;
                self.selection.settle(Ok(Some(file)));
            }
            Ok(Output::Clipboard(Content::Files(paths))) => {
                batch.source = Source::Paths(paths);
                self.start();
            }
            Ok(Output::Clipboard(Content::Text(text))) => {
                let Source::Clipboard {
                    allow_files,
                    target,
                } = batch.source
                else {
                    unreachable!()
                };
                if let Some(paths) = paths::parse(&text.value).and_then(|paths| {
                    clipboard::admit_paths(paths, allow_files, &self.context.budget).ok()
                }) {
                    batch.source = Source::Paths(paths);
                    self.start();
                } else if current_context == Some(batch.context.as_str())
                    && editor.paste_at(target, &text.value)
                {
                    batch.source = Source::Empty;
                    self.selection.clear();
                } else {
                    self.fail(FileError::PasteRejected);
                }
            }
        }
        self.dirty = true;
    }
    pub fn observe(&mut self) -> Option<NativeInput> {
        let batch = self.batch.as_ref()?;
        if !self.dirty || batch.staged {
            return None;
        }
        let observation = FilePasteObservation {
            context: batch.context.clone(),
            selection: self.selection.observation(),
            remaining: batch.source.remaining(),
            retryable: matches!(batch.source, Source::Paths(_)),
        };
        self.dirty = false;
        if observation.remaining == 0 {
            self.batch = None;
        }
        Some(NativeInput {
            replaceable: false,
            key: "file_observe".into(),
            value: serde_json::to_value(observation).unwrap(),
        })
    }
    pub fn capture(
        &self,
        input: &serde_json::Value,
    ) -> maka_ui_runtime::Result<Arc<dyn maka_capabilities::call::ReadResource>> {
        let capture: FileCapture = serde_json::from_value(input.clone())
            .map_err(|error| maka_ui_runtime::Error::Invalid(error.to_string()))?;
        self.selection.capture(&capture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maka_ui_runtime::budget::{BudgetLimits, NativeBudget};
    #[test]
    fn text_only_worker_preserves_target_and_rejects_stale_or_cancelled_insertion() {
        for change in [
            "selected", "cursor", "draft", "disabled", "focus", "invalid",
        ] {
            let resources = Arc::new(maka_capabilities::call::Resources::default());
            let budget = NativeBudget::default();
            let mut paste = Paste::new(NativeContext {
                checkpoint: None,
                resources: resources.clone(),
                budget: budget.clone(),
            });
            let mut editor = Editor::default();
            editor.insert("original");
            editor.select_all();
            paste.set_clipboard_text(
                if change == "invalid" {
                    "bad\u{1b}"
                } else {
                    "/tmp/plain-path\r\n中文"
                }
                .into(),
            );
            paste.text(&editor, false);
            match change {
                "cursor" => {
                    editor.key(crossterm::event::KeyEvent::new(
                        crossterm::event::KeyCode::End,
                        crossterm::event::KeyModifiers::NONE,
                    ));
                }
                "draft" => {
                    editor.insert("changed");
                }
                "focus" => paste.cancel_text(),
                _ => {}
            }
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(resources.finish())
                .unwrap();
            paste.poll_text(&mut editor, change != "disabled");
            assert_eq!(
                editor.text(),
                match change {
                    "selected" | "cursor" => "/tmp/plain-path\n中文",
                    "draft" => "changed",
                    _ => "original",
                }
            );
            assert_eq!(
                editor.error.is_some(),
                matches!(change, "draft" | "invalid")
            );
            assert!(
                paste.observe().is_none(),
                "text controls cannot publish file observations"
            );
            assert!(!paste.pending());
            assert_eq!(budget.usage(), (0, 0, 0));
        }
    }
    fn settled(paste: &mut Paste) -> FilePasteObservation {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            paste.poll(&mut Editor::default(), Some("original-session"));
            if let Some(input) = paste.observe() {
                let observed: FilePasteObservation = serde_json::from_value(input.value).unwrap();
                if observed.selection.phase != FilePhase::Choosing {
                    return observed;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "original worker did not settle"
            );
            std::thread::yield_now();
        }
    }
    fn command(observed: &FilePasteObservation, action: FilePasteAction) -> FilePasteCommand {
        FilePasteCommand {
            selection: FileCapture {
                owner: observed.selection.owner.clone(),
                revision: observed.selection.revision,
            },
            action,
        }
    }
    #[test]
    fn nine_original_files_are_deduplicated_and_consumed_with_exact_bytes_and_custody() {
        let directory = tempfile::tempdir().unwrap();
        let originals: Vec<_> = (0..9)
            .map(|index| {
                let path = directory.path().join(format!("original file {index}.txt"));
                let bytes = format!("original bytes {index} 中文").into_bytes();
                std::fs::write(&path, &bytes).unwrap();
                (path, bytes)
            })
            .collect();
        let mut paths: Vec<_> = originals.iter().map(|(path, _)| path).collect();
        paths.insert(1, &originals[0].0);
        paths.push(&originals[8].0);
        let text = paths
            .iter()
            .map(|path| format!("\"{}\"", path.display()))
            .collect::<Vec<_>>()
            .join(" ");
        let resources = Arc::new(maka_capabilities::call::Resources::default());
        let budget = NativeBudget::default();
        let mut paste = Paste::new(NativeContext {
            checkpoint: None,
            resources: resources.clone(),
            budget: budget.clone(),
        });
        let staged = paste
            .stage(
                &text,
                &FilePastePolicy {
                    context: "original-session".into(),
                    allow_files: true,
                },
            )
            .expect("all nine original files can be admitted");
        paste.commit(staged.value["request"].as_str(), None, true);
        let mut held = None;
        for (index, (path, bytes)) in originals.iter().enumerate() {
            let observed = settled(&mut paste);
            assert_eq!(observed.context, "original-session");
            assert_eq!(observed.remaining, originals.len() - index);
            assert_eq!(observed.selection.phase, FilePhase::Ready);
            let next = command(&observed, FilePasteAction::Next);
            let captured = paste
                .capture(&serde_json::to_value(&next.selection).unwrap())
                .unwrap();
            assert_eq!(captured.bytes(), bytes.len());
            assert_eq!(captured.read(0..captured.bytes()).unwrap(), *bytes);
            std::fs::remove_file(path).unwrap();
            paste.commit(None, Some(&next), true);
            if index == originals.len() - 1 {
                held = Some(captured);
            }
        }
        assert_eq!(settled(&mut paste).remaining, 0);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(resources.finish())
            .unwrap();
        assert!(!paste.pending());
        assert_eq!(budget.usage(), (65536, 0, 0));
        drop(held);
        assert_eq!(budget.usage(), (0, 0, 0));
    }
    #[test]
    fn original_batch_advances_only_by_revision_and_retains_capture_budget_until_release() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.txt");
        let second = directory.path().join("second.txt");
        let bytes = vec![b'a'; 65537];
        std::fs::write(&first, &bytes).unwrap();
        let resources = Arc::new(maka_capabilities::call::Resources::default());
        let budget = NativeBudget::new(BudgetLimits {
            encoded_bytes: 2 * 65536 + 16384,
            ..Default::default()
        });
        let mut paste = Paste::new(NativeContext {
            checkpoint: None,
            resources: resources.clone(),
            budget: budget.clone(),
        });
        let policy = FilePastePolicy {
            context: "original-session".into(),
            allow_files: true,
        };
        let staged = paste
            .stage(
                &format!(
                    "\"{}\" \"{}\" \"{}\"",
                    first.display(),
                    first.display(),
                    second.display()
                ),
                &policy,
            )
            .unwrap();
        assert!(
            !staged
                .value
                .to_string()
                .contains(directory.path().to_str().unwrap())
        );
        assert!(paste.read.is_none());
        paste.commit(Some("forged"), None, true);
        assert!(paste.read.is_none());
        paste.commit(staged.value["request"].as_str(), None, true);
        let observed = settled(&mut paste);
        assert_eq!(observed.remaining, 2, "duplicate native paths use one slot");
        assert_eq!(observed.context, "original-session");
        assert_eq!(observed.selection.phase, FilePhase::Ready);
        let capture = paste
            .capture(
                &serde_json::to_value(&command(&observed, FilePasteAction::Next).selection)
                    .unwrap(),
            )
            .unwrap();
        std::fs::remove_file(&first).unwrap();
        assert_eq!(capture.read(0..32).unwrap(), bytes[..32]);
        let mut invalid = command(&observed, FilePasteAction::Next);
        invalid.selection.owner = "another-owner".into();
        paste.commit(None, Some(&invalid), true);
        assert_eq!(paste.selection.observation(), observed.selection);
        // The later file did not exist when this batch was first admitted.
        std::fs::write(&second, vec![b'b'; 65537]).unwrap();
        let next = command(&observed, FilePasteAction::Next);
        paste.commit(None, Some(&next), true);
        let failed = settled(&mut paste);
        assert_eq!(failed.selection.error, Some(FileError::BudgetExhausted));
        assert_eq!(failed.remaining, 1);
        assert!(
            paste
                .capture(&serde_json::to_value(&next.selection).unwrap())
                .is_err()
        );
        drop(capture);
        paste.commit(None, Some(&command(&failed, FilePasteAction::Retry)), true);
        let second = settled(&mut paste);
        assert_eq!(second.selection.phase, FilePhase::Ready);
        let held = paste
            .capture(
                &serde_json::to_value(&command(&second, FilePasteAction::Next).selection).unwrap(),
            )
            .unwrap();
        assert_eq!(held.read(0..4).unwrap(), b"bbbb");
        paste.commit(None, Some(&next), true);
        assert_eq!(
            paste.selection.observation(),
            second.selection,
            "old disposition cannot consume the next item"
        );
        paste.commit(None, Some(&command(&second, FilePasteAction::Next)), true);
        assert_eq!(settled(&mut paste).remaining, 0);
        assert_eq!(budget.usage().0, 2 * 65536);
        drop(held);
        assert_eq!(budget.usage(), (0, 0, 0));
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(resources.finish())
            .unwrap();
    }
    #[test]
    fn uncommitted_or_cancelled_paste_cannot_read_or_publish_a_late_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file.txt");
        std::fs::write(&path, b"private").unwrap();
        let resources = Arc::new(maka_capabilities::call::Resources::default());
        let budget = NativeBudget::default();
        let mut paste = Paste::new(NativeContext {
            checkpoint: None,
            resources: resources.clone(),
            budget: budget.clone(),
        });
        let policy = FilePastePolicy {
            context: "session".into(),
            allow_files: true,
        };
        let staged = paste.stage(path.to_str().unwrap(), &policy).unwrap();
        paste.suspend();
        paste.commit(staged.value["request"].as_str(), None, true);
        assert!(paste.read.is_none());
        assert_eq!(budget.usage(), (0, 0, 0));
        let staged = paste.stage(path.to_str().unwrap(), &policy).unwrap();
        paste.commit(staged.value["request"].as_str(), None, true);
        let choosing = FilePasteObservation {
            context: "session".into(),
            selection: paste.selection.observation(),
            remaining: 1,
            retryable: true,
        };
        paste.commit(
            None,
            Some(&command(&choosing, FilePasteAction::Cancel)),
            false,
        );
        assert_eq!(settled(&mut paste).remaining, 0);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(resources.finish())
            .unwrap();
        paste.poll(&mut Editor::default(), Some("session"));
        assert!(
            paste.observe().is_none(),
            "late worker cannot restore a cancelled batch"
        );
        assert_eq!(budget.usage(), (0, 0, 0));
    }
    fn clipboard_text(value: &str, budget: &maka_ui_runtime::budget::NativeBudget) -> Content {
        let value = value.to_owned();
        let memory = budget.reserve(Kind::Encoded, value.capacity()).unwrap();
        Content::Text(clipboard::Text {
            value,
            _memory: memory,
        })
    }
    #[test]
    fn clipboard_text_uses_original_selection_and_rejects_changed_draft_or_context() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        for change in ["cursor", "draft", "context", "full"] {
            let resources = Arc::new(maka_capabilities::call::Resources::default());
            let budget = maka_ui_runtime::budget::NativeBudget::default();
            let mut paste = Paste::new(NativeContext {
                checkpoint: None,
                resources: resources.clone(),
                budget: budget.clone(),
            });
            let mut editor = Editor::default();
            editor.insert("beforeafter");
            for _ in 0..5 {
                editor.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
            }
            let policy = FilePastePolicy {
                context: "original".into(),
                allow_files: change != "full",
            };
            let staged = paste.stage_clipboard(&editor, &policy).unwrap();
            // Admit the same private marker, substituting only the OS response.
            assert!(paste.selection.accept(staged.value["request"].as_str()));
            paste.batch.as_mut().unwrap().staged = false;
            let content = clipboard_text(
                if change == "full" {
                    "/tmp/clipboard.txt"
                } else {
                    "中\r\n"
                },
                &budget,
            );
            paste.spawn(Box::new(move || Ok(Output::Clipboard(content))));
            editor.key(KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL));
            if change == "draft" {
                editor.insert("new-");
            }
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(resources.finish())
                .unwrap();
            paste.poll(
                &mut editor,
                Some(if change == "context" {
                    "other"
                } else {
                    "original"
                }),
            );
            let observed: FilePasteObservation =
                serde_json::from_value(paste.observe().unwrap().value).unwrap();
            if matches!(change, "cursor" | "full") {
                assert_eq!(
                    editor.text(),
                    if change == "full" {
                        "before/tmp/clipboard.txtafter"
                    } else {
                        "before中\nafter"
                    }
                );
                assert_eq!(observed.remaining, 0);
                assert_eq!(budget.usage(), (0, 0, 0));
            } else {
                assert_eq!(
                    editor.text(),
                    if change == "draft" {
                        "new-beforeafter"
                    } else {
                        "beforeafter"
                    }
                );
                assert_eq!(observed.selection.error, Some(FileError::PasteRejected));
                assert!(!observed.retryable);
                paste.commit(
                    None,
                    Some(&command(&observed, FilePasteAction::Retry)),
                    true,
                );
                assert!(
                    paste.read.is_none(),
                    "guest Retry cannot reread the clipboard"
                );
                let pressure = budget
                    .reserve(Kind::Encoded, 64 * 1024 * 1024 - budget.usage().0)
                    .unwrap();
                assert!(paste.stage_clipboard(&editor, &policy).is_none());
                assert_eq!(
                    paste.selection.observation(),
                    observed.selection,
                    "failed replacement admission preserves the original failure"
                );
                drop(pressure);
                assert!(
                    paste.stage_clipboard(&editor, &policy).is_some(),
                    "a new physical paste may replace the failed clipboard read"
                );
                paste.suspend();
                assert_eq!(budget.usage(), (0, 0, 0));
            }
        }
    }
    #[test]
    fn cancelled_clipboard_worker_keeps_its_original_ticket_and_drops_late_content() {
        let resources = Arc::new(maka_capabilities::call::Resources::default());
        let budget = maka_ui_runtime::budget::NativeBudget::default();
        let mut paste = Paste::new(NativeContext {
            checkpoint: None,
            resources: resources.clone(),
            budget: budget.clone(),
        });
        let mut editor = Editor::default();
        let staged = paste
            .stage_clipboard(
                &editor,
                &FilePastePolicy {
                    context: "original".into(),
                    allow_files: true,
                },
            )
            .unwrap();
        assert!(paste.selection.accept(staged.value["request"].as_str()));
        paste.batch.as_mut().unwrap().staged = false;
        let (entered, wait) = mpsc::sync_channel(1);
        let (release, held) = mpsc::sync_channel(1);
        let original = budget.clone();
        paste.spawn(Box::new(move || {
            let text = clipboard_text("late", &original);
            entered.send(()).unwrap();
            held.recv().unwrap();
            Ok(Output::Clipboard(text))
        }));
        wait.recv().unwrap();
        let choosing = FilePasteObservation {
            context: "original".into(),
            selection: paste.selection.observation(),
            remaining: 1,
            retryable: false,
        };
        paste.commit(
            None,
            Some(&command(&choosing, FilePasteAction::Cancel)),
            true,
        );
        assert_eq!(settled(&mut paste).remaining, 0);
        assert_eq!(budget.usage(), (4, 0, 0));
        assert!(
            paste.pending(),
            "physical clipboard work still owns the input admission"
        );
        release.send(()).unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(resources.finish())
            .unwrap();
        paste.poll(&mut editor, Some("original"));
        assert!(editor.text().is_empty());
        assert!(paste.observe().is_none());
        assert!(!paste.pending());
        assert_eq!(budget.usage(), (0, 0, 0));
    }
}
