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

//! Transient matches refer to the original native documents. Painting never
//! advances the accepted search, and a stream update never requests a scroll.
use super::super::{
    Slots,
    scroll::SceneState,
    selection::{DocumentRef, DocumentWeak},
    source,
};
use maka_plugin_core::runtime::Mount;
use maka_ui_runtime::reading::{
    FindStep, ReadingCommand, ReadingFind, ReadingMatch, ReadingTarget,
};
use std::{ops::Range, rc::Rc, sync::Arc};

#[derive(Clone)]
pub(super) struct Active {
    document: DocumentWeak,
    offset: usize,
}
#[derive(Clone, Default)]
pub(super) struct State {
    query: String,
    sequence: u64,
    active: Option<Active>,
    pub observation: Option<ReadingFind>,
}
pub(in crate::application) struct Entry {
    pub document: DocumentWeak,
    pub source: Arc<str>,
    pub path: Vec<String>,
}
struct Hit {
    entry: usize,
    range: Range<usize>,
}
pub(in crate::application) struct Paint {
    entries: Vec<Entry>,
    hits: Vec<Hit>,
    active: Option<usize>,
    next: State,
}
impl Paint {
    pub fn valid(&self) -> bool {
        self.entries.iter().all(|entry| {
            entry.document.upgrade().is_some_and(|doc| {
                let doc = doc.borrow();
                doc.alive && !doc.disabled && Arc::ptr_eq(&doc.source, &entry.source)
            })
        })
    }
    pub fn highlights(&self, document: &DocumentRef) -> Vec<(Range<usize>, bool)> {
        let weak = Rc::downgrade(document);
        self.hits
            .iter()
            .enumerate()
            .filter(|(_, hit)| self.entries[hit.entry].document.ptr_eq(&weak))
            .map(|(i, hit)| (hit.range.clone(), self.active == Some(i)))
            .collect()
    }
    pub(super) fn accept(self) -> State {
        self.next
    }
}
pub(super) fn documents(slots: &Slots<'_>) -> maka_ui_runtime::Result<(Vec<Entry>, Vec<Mount>)> {
    let mut result = Vec::new();
    let mut dependencies = Vec::new();
    let mut pending = vec![(slots.parent.clone(), Vec::<String>::new(), false)];
    let mut active = std::collections::BTreeSet::new();
    while let Some((mount, path, leaving)) = pending.pop() {
        if leaving {
            active.remove(&mount.id());
            continue;
        }
        if !active.insert(mount.id()) {
            return Err(maka_ui_runtime::Error::Invalid(
                "cyclic native reading mount".into(),
            ));
        }
        dependencies.push(mount.clone());
        pending.push((mount.clone(), path.clone(), true));
        if let Some(owner) = slots.presenter.resources.owners.get(&mount.id()) {
            if let Some(owner) = owner.state.downcast_ref::<source::Owner>() {
                let doc = owner.document.borrow();
                if doc.alive && !doc.disabled {
                    result.push(Entry {
                        document: Rc::downgrade(&owner.document),
                        source: doc.source.clone(),
                        path,
                    });
                }
                continue;
            }
            if !path.is_empty()
                && owner
                    .state
                    .downcast_ref::<SceneState>()
                    .is_some_and(|state| state.reading.is_some())
            {
                continue;
            }
        }
        for child in slots
            .presenter
            .application
            .frame(&mount)?
            .view
            .slots()
            .iter()
            .rev()
        {
            let mut path = path.clone();
            path.push(child.key.clone());
            pending.push((
                slots
                    .presenter
                    .application
                    .child_mount(&mount, &child.key)?
                    .clone(),
                path,
                false,
            ));
        }
    }
    Ok((result, dependencies))
}
impl State {
    pub(super) fn active(&self) -> bool {
        self.sequence != 0
    }
    pub(super) fn paint(&self, entries: Vec<Entry>, command: Option<&ReadingCommand>) -> Paint {
        let mut next = self.clone();
        let step = match command {
            Some(ReadingCommand {
                sequence,
                target: ReadingTarget::Find { query, step },
            }) => {
                if next.query != *query {
                    next.active = None;
                }
                next.query.clone_from(query);
                next.sequence = *sequence;
                Some(*step)
            }
            Some(ReadingCommand {
                target: ReadingTarget::ClearFind,
                ..
            }) => {
                next = Self::default();
                None
            }
            _ => None,
        };
        let mut hits = Vec::new();
        let limited = false;
        if !next.query.is_empty() {
            for (entry, doc) in entries.iter().enumerate() {
                for (offset, _) in doc.source.match_indices(&next.query) {
                    hits.push(Hit {
                        entry,
                        range: offset..offset + next.query.len(),
                    });
                }
            }
        }
        let previous = next.active.as_ref().and_then(|active| {
            hits.iter().position(|hit| {
                entries[hit.entry].document.ptr_eq(&active.document)
                    && hit.range.start == active.offset
            })
        });
        let active = if hits.is_empty() {
            None
        } else {
            Some(match step {
                Some(FindStep::First) => 0,
                Some(FindStep::Next) => previous.map_or(0, |index| (index + 1) % hits.len()),
                Some(FindStep::Previous) => previous.map_or(hits.len() - 1, |index| {
                    (index + hits.len() - 1) % hits.len()
                }),
                None => previous.unwrap_or(0),
            })
        };
        next.active = active.map(|i| Active {
            document: entries[hits[i].entry].document.clone(),
            offset: hits[i].range.start,
        });
        next.observation = (next.sequence != 0).then(|| ReadingFind {
            sequence: next.sequence,
            count: hits.len() as u32,
            limited,
            index: active.map(|i| i as u32 + 1),
            active: active.map(|i| ReadingMatch {
                path: entries[hits[i].entry].path.clone(),
                offset: hits[i].range.start as u32,
            }),
        });
        Paint {
            entries,
            hits,
            active,
            next,
        }
    }
}

pub(in crate::application) struct Reveal {
    pub document: DocumentWeak,
    pub source: Arc<str>,
    pub offset: usize,
    pub row: Option<i64>,
    pub visible: bool,
}
impl Reveal {
    pub(super) fn new(entries: &[Entry], path: &[String], offset: u32) -> Option<Self> {
        let entry = entries.iter().find(|entry| entry.path == path)?;
        let offset = offset as usize;
        if !entry.source.is_char_boundary(offset) {
            return None;
        }
        Some(Self {
            document: entry.document.clone(),
            source: entry.source.clone(),
            offset,
            row: None,
            visible: false,
        })
    }
    pub fn targets(&self, document: &DocumentRef) -> bool {
        self.document.ptr_eq(&Rc::downgrade(document))
            && Arc::ptr_eq(&self.source, &document.borrow().source)
    }
}
