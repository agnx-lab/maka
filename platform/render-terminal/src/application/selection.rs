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

//! One native selection authority per reading boundary; logical participants are
//! weak references to original Source documents, never a registry or copied range.
use super::Placement;
mod keyboard;
mod layout;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
pub(super) use layout::Layout;
use ratatui::layout::Position;
use std::{
    cell::RefCell,
    ops::Range,
    rc::{Rc, Weak},
    sync::Arc,
};
pub(super) type DocumentRef = Rc<RefCell<Document>>;
pub(super) type DocumentWeak = Weak<RefCell<Document>>;
/// Geometry metadata for the original document, committed only after flush.
#[derive(Clone)]
pub(super) struct Participant {
    pub document: DocumentWeak,
    pub child: Option<String>,
    pub geometry: Option<(Placement, u64)>,
}
impl std::ops::Deref for Participant {
    type Target = DocumentWeak;
    fn deref(&self) -> &Self::Target {
        &self.document
    }
}
impl From<DocumentWeak> for Participant {
    fn from(document: DocumentWeak) -> Self {
        Self {
            document,
            child: None,
            geometry: None,
        }
    }
}
#[derive(Clone)]
pub(super) struct Glyphs {
    pub source: Arc<str>,
    pub layout: Layout,
    pub area: Placement,
}
pub(super) struct Document {
    pub source: Arc<str>,
    pub geometry_revision: u64,
    pub markdown: Option<Arc<crate::markdown::prepared::Document>>,
    pub disabled: bool,
    pub alive: bool,
    pub physical: bool,
    pub glyphs: Option<Glyphs>,
    pub binding: Weak<RefCell<Scope>>,
    pub standalone: Option<SelectionScope>,
}
#[derive(Clone)]
struct Endpoint {
    document: DocumentWeak,
    byte: usize,
    trailing: bool,
}
#[derive(Default)]
pub(super) struct Scope {
    documents: Vec<Participant>,
    selection: Option<(Endpoint, Endpoint)>,
    drag: Option<DocumentWeak>,
    pub revision: u64,
    intent: u64,
    pub enabled: bool,
    area: Option<Placement>,
    column: Option<usize>,
    keyboard_focus_pending: bool,
    reveal: bool,
}
#[derive(Clone, Default)]
pub(super) struct SelectionScope(pub Rc<RefCell<Scope>>);
impl SelectionScope {
    pub fn revision(&self) -> u64 {
        self.0.borrow().revision
    }
    /// Physical intent and automatic document invalidation have different causes.
    pub fn intent(&self) -> u64 {
        self.0.borrow().intent
    }
    pub fn touch(&self) {
        let mut scope = self.0.borrow_mut();
        scope.revision += 1;
        scope.intent += 1;
        scope.keyboard_focus_pending = false;
        scope.reveal = false;
    }
    pub fn clear(&self) {
        self.touch();
        let mut scope = self.0.borrow_mut();
        scope.selection = None;
        scope.drag = None;
        scope.column = None;
    }
    pub fn suspend(&self) {
        let mut scope = self.0.borrow_mut();
        scope.enabled = false;
        scope.drag = None;
        scope.keyboard_focus_pending = false;
        scope.reveal = false;
    }
    pub fn close(&self) {
        let mut scope = self.0.borrow_mut();
        scope.enabled = false;
        scope.drag = None;
        scope.selection = None;
        scope.column = None;
        scope.keyboard_focus_pending = false;
        scope.reveal = false;
        scope.documents.clear();
    }
    pub fn release_pointer(&self, document: &DocumentRef) {
        let mut scope = self.0.borrow_mut();
        if scope
            .drag
            .as_ref()
            .is_some_and(|drag| drag.ptr_eq(&Rc::downgrade(document)))
        {
            scope.drag = None;
        }
    }
    pub fn pointer_capture(&self, document: &DocumentRef) -> bool {
        let scope = self.0.borrow();
        scope.enabled
            && scope
                .drag
                .as_ref()
                .is_some_and(|captured| captured.ptr_eq(&Rc::downgrade(document)))
    }
    pub fn active(document: &DocumentRef) -> Option<Self> {
        document.borrow().binding.upgrade().map(Self)
    }
    fn owns(&self, document: &DocumentRef) -> bool {
        document.borrow().binding.ptr_eq(&Rc::downgrade(&self.0))
    }
    pub fn withdraw(&self, document: &DocumentRef) {
        let id = Rc::downgrade(document);
        let mut scope = self.0.borrow_mut();
        let position = scope
            .documents
            .iter()
            .position(|candidate| candidate.ptr_eq(&id));
        let selected = scope.selection.as_ref().is_some_and(|(a, b)| {
            let a = scope
                .documents
                .iter()
                .position(|candidate| candidate.ptr_eq(&a.document));
            let b = scope
                .documents
                .iter()
                .position(|candidate| candidate.ptr_eq(&b.document));
            matches!((position,a,b),(Some(i),Some(a),Some(b)) if i>=a.min(b)&&i<=a.max(b))
        });
        if selected {
            scope.selection = None;
            scope.column = None;
            scope.keyboard_focus_pending = false;
            scope.reveal = false;
            scope.revision += 1;
        }
        if scope
            .drag
            .as_ref()
            .is_some_and(|captured| captured.ptr_eq(&id))
        {
            scope.drag = None;
        }
        scope.documents.retain(|candidate| !candidate.ptr_eq(&id));
    }
    /// Only embedding-confirmed output calls this. Doc binding is one weak
    /// identity; transferring it immediately withdraws the previous scope.
    pub fn present(&self, documents: Vec<Participant>, area: Placement) {
        for weak in &documents {
            if let Some(document) = weak.upgrade() {
                let old = Self::active(&document);
                if let Some(old) = old
                    && !Rc::ptr_eq(&old.0, &self.0)
                {
                    old.withdraw(&document);
                }
                let mut doc = document.borrow_mut();
                doc.binding = Rc::downgrade(&self.0);
                doc.standalone = None;
            }
        }
        let mut scope = self.0.borrow_mut();
        if let Some((a, b)) = &scope.selection {
            let position = |docs: &[Participant], endpoint: &Endpoint| {
                docs.iter().position(|doc| doc.ptr_eq(&endpoint.document))
            };
            let unchanged = match (
                position(&scope.documents, a),
                position(&scope.documents, b),
                position(&documents, a),
                position(&documents, b),
            ) {
                (Some(oa), Some(ob), Some(na), Some(nb)) => {
                    let old = &scope.documents[oa.min(ob)..=oa.max(ob)];
                    let new = &documents[na.min(nb)..=na.max(nb)];
                    old.len() == new.len() && old.iter().zip(new).all(|(a, b)| a.ptr_eq(b))
                }
                _ => false,
            };
            if !unchanged {
                scope.selection = None;
                scope.column = None;
                scope.keyboard_focus_pending = false;
                scope.reveal = false;
                scope.drag = None;
                scope.revision += 1;
            }
        }
        for old in &scope.documents {
            if !documents.iter().any(|doc| doc.ptr_eq(old))
                && let Some(document) = old.upgrade()
            {
                let mut doc = document.borrow_mut();
                if doc.binding.ptr_eq(&Rc::downgrade(&self.0)) {
                    doc.binding = Weak::new();
                }
            }
        }
        scope.documents = documents;
        scope.area = Some(area);
        scope.enabled = !area.is_empty();
        if let Some((a, b)) = &scope.selection
            && (!scope.documents.iter().any(|doc| doc.ptr_eq(&a.document))
                || !scope.documents.iter().any(|doc| doc.ptr_eq(&b.document)))
        {
            scope.selection = None;
            scope.column = None;
            scope.keyboard_focus_pending = false;
            scope.reveal = false;
            scope.drag = None;
            scope.revision += 1;
        }
    }
    pub fn standalone(document: &DocumentRef, area: Placement) {
        let existing = document.borrow().standalone.clone();
        let scope = existing.unwrap_or_default();
        let geometry = (area, document.borrow().geometry_revision);
        scope.present(
            vec![Participant {
                document: Rc::downgrade(document),
                child: None,
                geometry: Some(geometry),
            }],
            area,
        );
        document.borrow_mut().standalone = Some(scope);
    }
    fn endpoints(&self) -> Option<(usize, usize, usize, usize)> {
        let scope = self.0.borrow();
        let (a, b) = scope.selection.as_ref()?;
        let ai = scope
            .documents
            .iter()
            .position(|doc| doc.ptr_eq(&a.document))?;
        let bi = scope
            .documents
            .iter()
            .position(|doc| doc.ptr_eq(&b.document))?;
        Some(if (ai, a.byte) <= (bi, b.byte) {
            (ai, a.byte, bi, b.byte)
        } else {
            (bi, b.byte, ai, a.byte)
        })
    }
    pub fn range(&self, document: &DocumentRef) -> Range<usize> {
        let Some((a, start, b, end)) = self.endpoints() else {
            return 0..0;
        };
        let scope = self.0.borrow();
        let id = Rc::downgrade(document);
        let Some(i) = scope.documents.iter().position(|doc| doc.ptr_eq(&id)) else {
            return 0..0;
        };
        if i < a || i > b {
            return 0..0;
        }
        let len = document.borrow().source.len();
        if a == b {
            start..end
        } else {
            (if i == a { start } else { 0 })..(if i == b { end } else { len })
        }
    }
    pub fn cursor(&self, document: &DocumentRef) -> (usize, bool) {
        self.0
            .borrow()
            .selection
            .as_ref()
            .filter(|(_, cursor)| cursor.document.ptr_eq(&Rc::downgrade(document)))
            .map_or((0, false), |(_, cursor)| (cursor.byte, cursor.trailing))
    }
    /// The native focus endpoint, including a collapsed caret. Suspension by a
    /// modal retains context; retirement or transfer of the document withdraws it.
    pub fn selected_child(&self) -> Option<String> {
        let scope = self.0.borrow();
        let (_, focus) = scope.selection.as_ref()?;
        let participant = scope
            .documents
            .iter()
            .find(|doc| doc.ptr_eq(&focus.document))?;
        let document = participant.upgrade()?;
        let doc = document.borrow();
        (doc.alive && !doc.disabled && doc.binding.ptr_eq(&Rc::downgrade(&self.0)))
            .then(|| participant.child.clone())
            .flatten()
    }
    pub fn has_selection(&self) -> bool {
        self.endpoints()
            .is_some_and(|(a, start, b, end)| a != b || start != end)
    }
    pub fn has_pointer_capture(&self) -> bool {
        let scope = self.0.borrow();
        scope.enabled
            && scope
                .drag
                .as_ref()
                .and_then(Weak::upgrade)
                .is_some_and(|document| {
                    let doc = document.borrow();
                    doc.alive
                        && doc.physical
                        && !doc.disabled
                        && doc.binding.ptr_eq(&Rc::downgrade(&self.0))
                })
    }
    pub fn copy(&self) -> maka_ui_runtime::Result<Option<String>> {
        if !self.0.borrow().enabled {
            return Ok(None);
        }
        let Some((a, start, b, end)) = self.endpoints() else {
            return Ok(None);
        };
        if a == b && start == end {
            return Ok(None);
        }
        let docs = self.0.borrow().documents.clone();
        let mut bytes = 0usize;
        // Both passes read the same original owners on the foreground thread.
        // Reject before allocation; no partial clipboard and no second range ledger.
        for (i, weak) in docs.iter().enumerate().take(b + 1).skip(a) {
            let Some(document) = weak.upgrade() else {
                return Ok(None);
            };
            if !self.owns(&document) {
                return Ok(None);
            }
            let doc = document.borrow();
            if !doc.alive || doc.disabled {
                return Ok(None);
            }
            let range =
                (if i == a { start } else { 0 })..(if i == b { end } else { doc.source.len() });
            let Some(part) = doc.source.get(range) else {
                return Ok(None);
            };
            bytes = bytes
                .checked_add(part.len())
                .and_then(|bytes| bytes.checked_add(usize::from(i > a)))
                .ok_or_else(|| {
                    maka_ui_runtime::Error::Invalid(
                        "source selection length is not representable".into(),
                    )
                })?;
        }
        let mut text = String::with_capacity(bytes);
        for (i, weak) in docs.iter().enumerate().take(b + 1).skip(a) {
            let document = weak
                .upgrade()
                .expect("foreground document retained between copy passes");
            let doc = document.borrow();
            let range =
                (if i == a { start } else { 0 })..(if i == b { end } else { doc.source.len() });
            if i > a {
                text.push('\n');
            }
            text.push_str(&doc.source[range]);
        }
        Ok(Some(text))
    }
    fn hit(&self, point: Position) -> Option<Endpoint> {
        let scope = self.0.borrow();
        let mut nearest = None;
        let mut distance = i64::MAX;
        for weak in &scope.documents {
            let Some(document) = weak.upgrade() else {
                continue;
            };
            let doc = document.borrow();
            if !doc.alive
                || doc.disabled
                || !doc.physical
                || !doc.binding.ptr_eq(&Rc::downgrade(&self.0))
            {
                continue;
            }
            let Some(glyphs) = &doc.glyphs else {
                continue;
            };
            if glyphs.source.as_ref() != doc.source.as_ref() || glyphs.area.is_empty() {
                continue;
            }
            let visible = glyphs.area.visible();
            let d = if point.y < visible.y {
                i64::from(visible.y - point.y)
            } else if point.y >= visible.bottom() {
                i64::from(point.y - visible.bottom() + 1)
            } else {
                0
            };
            let dx = if point.x < visible.x {
                i64::from(visible.x - point.x)
            } else if point.x >= visible.right() {
                i64::from(point.x - visible.right() + 1)
            } else {
                0
            };
            let d = d + dx;
            // Match physical hit order when overlapping; only gaps use nearest glyphs.
            if d <= distance {
                distance = d;
                let (byte, trailing) = glyphs.layout.pointer(glyphs.area, 0, point);
                nearest = Some(Endpoint {
                    document: weak.document.clone(),
                    byte,
                    trailing,
                });
            }
        }
        nearest
    }
    pub fn input(&self, document: &DocumentRef, event: &Event) -> bool {
        if !self.owns(document) || !self.0.borrow().enabled {
            return false;
        }
        let id = Rc::downgrade(document);
        let mut pair = None;
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let doc = document.borrow();
                    let Some(glyphs) = &doc.glyphs else {
                        return false;
                    };
                    if doc.disabled
                        || !doc.alive
                        || !doc.physical
                        || glyphs.source.as_ref() != doc.source.as_ref()
                        || !glyphs.area.contains((mouse.column, mouse.row).into())
                    {
                        return false;
                    }
                    let (byte, trailing) =
                        glyphs
                            .layout
                            .pointer(glyphs.area, 0, (mouse.column, mouse.row).into());
                    let cursor = Endpoint {
                        document: id.clone(),
                        byte,
                        trailing,
                    };
                    let anchor = if mouse
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
                    {
                        self.0
                            .borrow()
                            .selection
                            .as_ref()
                            .map(|(anchor, _)| anchor.clone())
                            .unwrap_or(Endpoint {
                                document: id.clone(),
                                byte: 0,
                                trailing: false,
                            })
                    } else {
                        cursor.clone()
                    };
                    pair = Some((anchor, cursor));
                    self.0.borrow_mut().drag = Some(id.clone());
                }
                MouseEventKind::Drag(MouseButton::Left) if self.pointer_capture(document) => {
                    let Some(cursor) = self.hit((mouse.column, mouse.row).into()) else {
                        return false;
                    };
                    let Some((anchor, _)) = self.0.borrow().selection.clone() else {
                        return false;
                    };
                    pair = Some((anchor, cursor));
                }
                MouseEventKind::Up(MouseButton::Left) if self.pointer_capture(document) => {
                    self.0.borrow_mut().drag = None;
                }
                _ => return false,
            },
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if matches!(key.code, KeyCode::Char('a' | 'A'))
                    && key.modifiers == KeyModifiers::CONTROL
                {
                    let docs = self.0.borrow().documents.clone();
                    let Some(first) = docs.first() else {
                        return false;
                    };
                    let Some(last) = docs.last() else {
                        return false;
                    };
                    let Some(last_doc) = last.upgrade() else {
                        return false;
                    };
                    pair = Some((
                        Endpoint {
                            document: first.document.clone(),
                            byte: 0,
                            trailing: false,
                        },
                        Endpoint {
                            document: last.document.clone(),
                            byte: last_doc.borrow().source.len(),
                            trailing: false,
                        },
                    ));
                    self.0.borrow_mut().drag = None;
                } else if key.code == KeyCode::Esc && key.modifiers.is_empty() {
                    if self.0.borrow().selection.is_none() && self.0.borrow().drag.is_none() {
                        return false;
                    }
                    self.0.borrow_mut().selection = None;
                    self.0.borrow_mut().drag = None;
                } else if key.modifiers == KeyModifiers::SHIFT {
                    return self.extend_keyboard(document, key.code);
                } else {
                    return false;
                }
            }
            _ => return false,
        }
        let mut scope = self.0.borrow_mut();
        if let Some(pair) = pair {
            scope.selection = Some(pair);
        }
        scope.column = None;
        scope.keyboard_focus_pending = false;
        scope.reveal = false;
        scope.revision += 1;
        scope.intent += 1;
        true
    }
}
pub(super) struct ScopePaint {
    pub root: Option<(uuid::Uuid, String)>,
    pub child: Option<String>,
    pub scope: SelectionScope,
    pub documents: Vec<Participant>,
    pub area: Placement,
    pub find: Option<super::reading::find::Paint>,
    pub reveal: Option<super::reading::find::Reveal>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::layout::Layout as LiteralLayout;
    use crossterm::event::KeyEvent;
    use ratatui::layout::Rect;
    #[test]
    fn unflushed_transfer_preserves_original_scope_and_success_withdraws_it() {
        let area: Placement = Rect::new(0, 0, 12, 2).into();
        let document = Rc::new(RefCell::new(Document {
            source: Arc::from("original"),
            geometry_revision: 0,
            markdown: None,
            disabled: false,
            alive: true,
            physical: true,
            glyphs: Some(Glyphs {
                source: Arc::from("original"),
                layout: LiteralLayout::literal("original", 12).into(),
                area,
            }),
            binding: Weak::new(),
            standalone: None,
        }));
        SelectionScope::standalone(&document, area);
        let old = SelectionScope::active(&document).unwrap();
        old.input(
            &document,
            &Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        );
        let new = SelectionScope::default();
        let candidate = ScopePaint {
            root: None,
            child: None,
            scope: new.clone(),
            documents: vec![Rc::downgrade(&document).into()],
            area,
            find: None,
            reveal: None,
        };
        assert_eq!(Rc::strong_count(&document), 1);
        drop(candidate);
        assert_eq!(old.copy().unwrap().as_deref(), Some("original"));
        new.present(vec![Rc::downgrade(&document).into()], area);
        assert_eq!(old.copy().unwrap(), None);
        assert!(!old.has_selection());
        assert!(document.borrow().standalone.is_none());
        assert!(Rc::ptr_eq(
            &SelectionScope::active(&document).unwrap().0,
            &new.0
        ));
        new.input(
            &document,
            &Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        );
        assert_eq!(new.copy().unwrap().as_deref(), Some("original"));
    }
    #[test]
    fn stale_child_geometry_cannot_reveal_an_appended_document_until_its_new_flush() {
        use ratatui::layout::Rect;
        let area: Placement = Rect::new(0, 0, 3, 2).into();
        let document = Rc::new(RefCell::new(Document {
            source: Arc::from("abcdef"),
            geometry_revision: 0,
            markdown: None,
            disabled: false,
            alive: true,
            physical: true,
            glyphs: None,
            binding: Weak::new(),
            standalone: None,
        }));
        SelectionScope::standalone(&document, area);
        let scope = SelectionScope::active(&document).unwrap();
        assert!(scope.input(
            &document,
            &Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT))
        ));
        let painted = scope.0.borrow().documents.clone();
        // An independently updated descendant may append without a new parent
        // Reading frame. Its old placement cannot stand in for the new layout.
        document.borrow_mut().source = Arc::from("abcdefgh");
        document.borrow_mut().geometry_revision += 1;
        scope.present(painted, area);
        assert_eq!(scope.take_reveal_row(), None);
        assert_eq!(scope.copy().unwrap().as_deref(), Some("abc"));
        scope.present(
            vec![Participant {
                document: Rc::downgrade(&document),
                child: None,
                geometry: Some((area, 1)),
            }],
            area,
        );
        assert_eq!(scope.take_reveal_row(), Some(0));
        assert_eq!(scope.take_reveal_row(), None);
    }
}
