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

use super::*;
use std::{
    any::{Any, TypeId},
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

pub(super) struct Owner {
    pub identity: Rc<()>,
    pub recipe: Rc<dyn Widget>,
    pub(super) measurements: std::cell::RefCell<paint::RetainedMeasurements>,
    pub state: Box<dyn Any>,
    pub(super) resources: std::sync::Arc<maka_capabilities::call::Resources>,
    kind: TypeId,
    revision: Option<u64>,
    pub live: bool,
    presented: bool,
    // Independent native observation channels must not overwrite each other.
    pub(super) pending: maka_ui_runtime::publication::Observations,
}
type PublicationReservation = (Uuid, maka_ui_runtime::publication::Capacity);
impl Owner {
    fn observe(&mut self, input: NativeInput) -> maka_ui_runtime::Result<bool> {
        Ok(self
            .pending
            .capture(&input.key, input.value, input.replaceable)?
            .is_some())
    }
}
struct OwnerWitness {
    identity: std::rc::Weak<()>,
    value: maka_ui_runtime::actions::ContinuationWitness,
}
#[derive(Default)]
pub(super) struct Resources {
    pub owners: BTreeMap<Uuid, Owner>,
    pub redraw: bool,
    failed: BTreeMap<Uuid, String>,
    pub(super) budget: maka_ui_runtime::budget::NativeBudget,
}
impl Resources {
    pub fn cancel_pastes(&mut self, keep: Option<Uuid>) -> maka_ui_runtime::Result<()> {
        for (id, owner) in &mut self.owners {
            if owner.live
                && Some(*id) != keep
                && catch_unwind(AssertUnwindSafe(|| {
                    owner.recipe.cancel_paste(owner.state.as_mut())
                }))
                .is_err()
            {
                owner.live = false;
                let error = "terminal clipboard cancellation cleanup unconfirmed".to_owned();
                self.failed.insert(*id, error.clone());
                return Err(Error::Invalid(error));
            }
        }
        Ok(())
    }
    pub fn editing_menu(
        &mut self,
        target: &paint::TargetIdentity,
    ) -> maka_ui_runtime::Result<
        Option<(
            maka_ui_runtime::textarea::TextareaMenu,
            maka_ui_runtime::editor::EditorSnapshot,
            [bool; 4],
        )>,
    > {
        Ok(self
            .read_native(target, |recipe, state| recipe.editing_menu(state))?
            .flatten())
    }

    pub fn cancel_pointer(&mut self) -> maka_ui_runtime::Result<()> {
        for (id, owner) in &mut self.owners {
            if owner.live
                && catch_unwind(AssertUnwindSafe(|| {
                    owner.recipe.cancel_pointer(owner.state.as_mut())
                }))
                .is_err()
            {
                owner.live = false;
                let error = "terminal pointer interruption cleanup unconfirmed".to_owned();
                self.failed.insert(*id, error.clone());
                return Err(Error::Invalid(error));
            }
        }
        Ok(())
    }
    pub fn prepare_candidate(
        &mut self,
        app: &Application<dyn Widget>,
        candidate: maka_ui_runtime::Candidate<'_, dyn Widget>,
    ) -> maka_ui_runtime::Result<Vec<PublicationReservation>> {
        // Validate every retained type before allocating any provisional owner.
        // The existing owner is untouched if the candidate cannot commit.
        for (mount, frame) in candidate.changed() {
            if self
                .owners
                .get(&mount.id())
                .is_some_and(|owner| owner.kind != frame.view.view.as_ref().type_id())
            {
                return Err(Error::Invalid(
                    "native recipe type changed on original mount; remount the component".into(),
                ));
            }
            if let Some(reason) = self.failed.get(&mount.id()) {
                return Err(Error::Invalid(reason.clone()));
            }
        }
        for (mount, frame) in candidate.changed() {
            if let std::collections::btree_map::Entry::Vacant(entry) = self.owners.entry(mount.id())
            {
                let recipe = frame.view.view.clone();
                let resources = std::sync::Arc::new(maka_capabilities::call::Resources::with_wake(
                    app.wake_callback(),
                ));
                let context = NativeContext {
                    checkpoint: app.checkpoint_scope(mount)?,
                    resources: resources.clone(),
                    budget: self.budget.clone(),
                };
                match catch_unwind(AssertUnwindSafe(|| recipe.create(context))) {
                    Ok(state) => {
                        entry.insert(Owner {
                            identity: Rc::new(()),
                            kind: recipe.as_ref().type_id(),
                            recipe,
                            measurements: Default::default(),
                            state,
                            resources,
                            revision: None,
                            live: true,
                            presented: false,
                            pending: maka_ui_runtime::publication::Observations::new(
                                self.budget.clone(),
                                app.wake_callback(),
                            ),
                        });
                    }
                    Err(_) => {
                        let error = "terminal native creation cleanup unconfirmed".to_owned();
                        self.failed.insert(mount.id(), error.clone());
                        return Err(Error::Invalid(error));
                    }
                }
            }
        }
        let mut reservations = Vec::new();
        for (mount, frame) in candidate.changed() {
            let owner = self
                .owners
                .get(&mount.id())
                .expect("prepared original owner");
            let capacity = owner
                .pending
                .prepare(frame.view.view.publication_capacity(owner.state.as_ref()))?;
            reservations.push((mount.id(), capacity));
        }
        Ok(reservations)
    }

    pub fn prepare(
        &mut self,
        app: &Application<dyn Widget>,
        mounts: &[Mount],
    ) -> maka_ui_runtime::Result<bool> {
        for mount in mounts {
            if app.is_retiring(mount) {
                continue;
            }
            if let Some(reason) = self.failed.get(&mount.id()) {
                return Err(Error::Invalid(reason.clone()));
            }
            let frame = app.frame(mount)?;
            let recipe = frame.view.view.clone();
            let kind = recipe.as_ref().type_id();
            if self
                .owners
                .get(&mount.id())
                .is_some_and(|owner| owner.kind != kind || !owner.live)
            {
                return Err(Error::Invalid(
                    "native recipe type changed on original mount; remount the component".into(),
                ));
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = self.owners.entry(mount.id())
            {
                let resources = std::sync::Arc::new(maka_capabilities::call::Resources::with_wake(
                    app.wake_callback(),
                ));
                let context = NativeContext {
                    checkpoint: app.checkpoint_scope(mount)?,
                    resources: resources.clone(),
                    budget: self.budget.clone(),
                };
                match catch_unwind(AssertUnwindSafe(|| recipe.create(context))) {
                    Ok(state) => {
                        entry.insert(Owner {
                            identity: Rc::new(()),
                            recipe: recipe.clone(),
                            measurements: Default::default(),
                            state,
                            resources,
                            kind,
                            revision: None,
                            live: true,
                            presented: false,
                            pending: maka_ui_runtime::publication::Observations::new(
                                self.budget.clone(),
                                app.wake_callback(),
                            ),
                        });
                    }
                    Err(_) => {
                        let error = "terminal native creation cleanup unconfirmed".to_owned();
                        self.failed.insert(mount.id(), error.clone());
                        return Err(Error::Invalid(error));
                    }
                }
            }
            let owner = self.owners.get_mut(&mount.id()).unwrap();
            self.redraw |= owner.revision != Some(frame.revision);
            if !Rc::ptr_eq(&owner.recipe, &recipe) {
                *owner.measurements.get_mut() = Default::default();
            }
            owner.recipe = recipe;
            if catch_unwind(AssertUnwindSafe(|| {
                owner.recipe.prepare_focus(
                    owner.state.as_mut(),
                    app.mount_visible(mount),
                    frame.revision,
                )
            }))
            .is_err()
            {
                owner.live = false;
                let error = "terminal native focus preparation cleanup unconfirmed".to_owned();
                self.failed.insert(mount.id(), error.clone());
                return Err(Error::Invalid(error));
            }
        }
        Ok(true)
    }
    /// Derived compatibility with the committed recipe, never a cleanup receipt.
    pub fn ready(
        &self,
        app: &Application<dyn Widget>,
        root: &Mount,
    ) -> maka_ui_runtime::Result<bool> {
        for mount in app.mounted_subtree(root)? {
            if app.is_retiring(&mount) {
                continue;
            }
            let kind = app.frame(&mount)?.view.view.as_ref().type_id();
            if !self
                .owners
                .get(&mount.id())
                .is_some_and(|owner| owner.live && owner.kind == kind)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn poll_observation(&mut self, id: Uuid) -> maka_ui_runtime::Result<()> {
        let Some(owner) = self.owners.get_mut(&id).filter(|owner| owner.live) else {
            return Ok(());
        };
        // Drain the original producer before accepting a causally later input.
        // Stable pending evidence stops immediately; the publication bound also
        // limits a producer that continually returns new values.
        for _ in 0..128 {
            match catch_unwind(AssertUnwindSafe(|| {
                let observed = owner.recipe.observe(owner.state.as_mut());
                self.redraw |= owner.recipe.take_redraw(owner.state.as_mut());
                observed
            })) {
                Ok(Some(observed)) => {
                    let captured = owner.observe(observed)?;
                    self.redraw |= captured;
                    if !captured {
                        return Ok(());
                    }
                }
                Ok(None) => return Ok(()),
                Err(_) => {
                    owner.live = false;
                    let error = "terminal native observation cleanup unconfirmed".to_owned();
                    self.failed.insert(id, error.clone());
                    return Err(Error::Invalid(error));
                }
            }
        }
        Err(Error::Invalid("native observation capture bound".into()))
    }
    fn collect_observations(
        &mut self,
        app: &Application<dyn Widget>,
        mounts: &[Mount],
    ) -> maka_ui_runtime::Result<()> {
        for mount in mounts {
            if !app.is_retiring(mount) {
                self.poll_observation(mount.id())?;
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub fn commit(
        &mut self,
        app: &Application<dyn Widget>,
        root: &Mount,
    ) -> maka_ui_runtime::Result<()> {
        self.commit_mounts(app, &app.mounted_subtree(root)?)
    }
    fn commit_mounts(
        &mut self,
        app: &Application<dyn Widget>,
        mounts: &[Mount],
    ) -> maka_ui_runtime::Result<()> {
        for mount in mounts {
            if app.is_retiring(mount) {
                continue;
            }
            let frame = app.frame(mount)?;
            let owner = self
                .owners
                .get_mut(&mount.id())
                .expect("prepared terminal native owner");
            if owner.revision != Some(frame.revision) {
                let observed = match catch_unwind(AssertUnwindSafe(|| {
                    owner.recipe.commit(owner.state.as_mut())
                })) {
                    Ok(result) => result?,
                    Err(_) => {
                        let error = "terminal native commit cleanup unconfirmed".to_owned();
                        self.failed.insert(mount.id(), error.clone());
                        return Err(Error::Invalid(error));
                    }
                };
                owner.revision = Some(frame.revision);
                if let Some(observed) = observed {
                    owner.observe(observed)?;
                }
            }
            self.poll_observation(mount.id())?;
        }
        Ok(())
    }
    pub fn deactivate(&mut self, app: &Application<dyn Widget>) {
        for mount in app.retiring_mounts() {
            if let Some(owner) = self.owners.get_mut(&mount.id())
                && owner.live
            {
                owner.live = false;
                owner.pending.close();
                if catch_unwind(AssertUnwindSafe(|| {
                    owner.recipe.deactivate(owner.state.as_mut())
                }))
                .is_err()
                {
                    self.failed.insert(
                        mount.id(),
                        "terminal native deactivation cleanup unconfirmed".into(),
                    );
                }
            }
        }
    }
    /// Keep original native custody while its sealed worker group is pending.
    pub fn close(&mut self, mount: &Mount) -> Result<bool, String> {
        if let Some(error) = self.failed.get(&mount.id()) {
            return Err(error.clone());
        }
        if let Some(owner) = self.owners.get_mut(&mount.id()) {
            if owner.live {
                owner.live = false;
                owner.pending.close();
                if catch_unwind(AssertUnwindSafe(|| {
                    owner.recipe.deactivate(owner.state.as_mut());
                }))
                .is_err()
                {
                    let error = "terminal native deactivation cleanup unconfirmed".to_owned();
                    self.failed.insert(mount.id(), error.clone());
                    return Err(error);
                }
            }
            match owner.resources.seal() {
                Ok(false) => return Ok(false),
                Ok(true) => {}
                Err(error) => {
                    let error = error.to_string();
                    self.failed.insert(mount.id(), error.clone());
                    return Err(error);
                }
            }
        }
        if let Some(owner) = self.owners.remove(&mount.id())
            && catch_unwind(AssertUnwindSafe(|| drop(owner))).is_err()
        {
            let error = "terminal native destruction cleanup unconfirmed".to_owned();
            self.failed.insert(mount.id(), error.clone());
            return Err(error);
        }
        Ok(true)
    }
    pub fn observe(&mut self, mount: &Mount, observed: NativeInput) -> maka_ui_runtime::Result<()> {
        if let Some(owner) = self.owners.get_mut(&mount.id()) {
            owner.observe(observed)?;
        }
        Ok(())
    }
    pub fn capture_input(
        &mut self,
        mount: &Mount,
        input: NativeInput,
    ) -> maka_ui_runtime::Result<maka_ui_runtime::backend::Job<()>> {
        let owner = self
            .owners
            .get_mut(&mount.id())
            .filter(|owner| owner.live)
            .ok_or_else(|| Error::Invalid("original native input owner retired".into()))?;
        owner.pending.capture_input(&input.key, input.value)
    }
    /// Identity for an original published logical keyboard target. Pixel/native
    /// resource admission remains `admits`; offscreen keyboard navigation only
    /// needs the live owner frozen in the successfully flushed target list.
    pub(super) fn owns(&self, target: &paint::TargetIdentity) -> bool {
        self.owners
            .get(&target.mount.id())
            .is_some_and(|owner| owner.live && Rc::ptr_eq(&owner.identity, &target.owner))
    }
    pub fn admits(&self, target: &paint::TargetIdentity) -> bool {
        self.owners.get(&target.mount.id()).is_some_and(|owner| {
            owner.live && owner.presented && Rc::ptr_eq(&owner.identity, &target.owner)
        })
    }
    pub fn input(
        &mut self,
        target: &paint::TargetIdentity,
        event: &Event,
    ) -> maka_ui_runtime::Result<Option<WidgetInput>> {
        self.poll_observation(target.mount.id())?;
        let input = self.interact(target, |recipe, state| {
            recipe.input(state, &target.key, event)
        })?;
        if matches!(
            &input,
            Some(WidgetInput::Emitted(_) | WidgetInput::SubmitCopy(..))
        ) {
            self.poll_observation(target.mount.id())?;
        }
        Ok(input)
    }
    pub(super) fn enter_focus(
        &mut self,
        target: &paint::TargetIdentity,
    ) -> maka_ui_runtime::Result<bool> {
        Ok(self
            .interact(target, |recipe, state| {
                Ok(recipe.enter_focus(state, &target.key))
            })?
            .unwrap_or(false))
    }
    fn interact<T>(
        &mut self,
        target: &paint::TargetIdentity,
        action: impl FnOnce(&dyn Widget, &mut dyn Any) -> maka_ui_runtime::Result<T>,
    ) -> maka_ui_runtime::Result<Option<T>> {
        let Some(owner) = self.owners.get_mut(&target.mount.id()).filter(|owner| {
            owner.live && owner.presented && Rc::ptr_eq(&owner.identity, &target.owner)
        }) else {
            return Ok(None);
        };
        match catch_unwind(AssertUnwindSafe(|| {
            action(owner.recipe.as_ref(), owner.state.as_mut())
        })) {
            Ok(result) => result.map(Some),
            Err(_) => {
                let error = "terminal native input cleanup unconfirmed".to_owned();
                owner.live = false;
                self.failed.insert(target.mount.id(), error.clone());
                Err(Error::Invalid(error))
            }
        }
    }
    pub(super) fn captures_keyboard(
        &mut self,
        target: &paint::TargetIdentity,
    ) -> maka_ui_runtime::Result<bool> {
        Ok(self
            .read_native(target, |recipe, state| recipe.captures_keyboard(state))?
            .unwrap_or(false))
    }
    pub(super) fn raw_pointer(
        &mut self,
        target: &paint::TargetIdentity,
    ) -> maka_ui_runtime::Result<bool> {
        Ok(self
            .read_native(target, |recipe, state| recipe.raw_pointer(state))?
            .unwrap_or(false))
    }
    pub(super) fn handles_tab(
        &mut self,
        target: &paint::TargetIdentity,
    ) -> maka_ui_runtime::Result<bool> {
        Ok(self
            .read_native(target, |recipe, state| {
                recipe.handles_tab(state, &target.key)
            })?
            .unwrap_or(false))
    }
    pub(super) fn returns_focus(
        &mut self,
        target: &paint::TargetIdentity,
    ) -> maka_ui_runtime::Result<bool> {
        Ok(self
            .read_native(target, |recipe, _| recipe.returns_focus())?
            .unwrap_or(false))
    }
    pub(super) fn take_focus_exits(&mut self) -> maka_ui_runtime::Result<Vec<super::focus::Exit>> {
        let mut exits = Vec::new();
        for (id, owner) in &mut self.owners {
            if !owner.live {
                continue;
            }
            match catch_unwind(AssertUnwindSafe(|| {
                owner.recipe.take_focus_exit(owner.state.as_mut())
            })) {
                Ok(true) => exits.push(super::focus::Exit {
                    mount: *id,
                    owner: Rc::downgrade(&owner.identity),
                }),
                Ok(false) => {}
                Err(_) => {
                    owner.live = false;
                    let error = "terminal native focus cleanup unconfirmed".to_owned();
                    self.failed.insert(*id, error.clone());
                    return Err(Error::Invalid(error));
                }
            }
        }
        Ok(exits)
    }
    fn read_native<T>(
        &mut self,
        target: &paint::TargetIdentity,
        read: impl FnOnce(&dyn Widget, &dyn Any) -> T,
    ) -> maka_ui_runtime::Result<Option<T>> {
        let Some(owner) = self.owners.get_mut(&target.mount.id()).filter(|owner| {
            owner.live && owner.presented && Rc::ptr_eq(&owner.identity, &target.owner)
        }) else {
            return Ok(None);
        };
        match catch_unwind(AssertUnwindSafe(|| {
            read(owner.recipe.as_ref(), owner.state.as_ref())
        })) {
            Ok(value) => Ok(Some(value)),
            Err(_) => {
                owner.live = false;
                let error = "terminal native read cleanup unconfirmed".to_owned();
                self.failed.insert(target.mount.id(), error.clone());
                Err(Error::Invalid(error))
            }
        }
    }
    pub fn pointer_capture(
        &mut self,
        target: &paint::TargetIdentity,
    ) -> maka_ui_runtime::Result<bool> {
        Ok(self
            .read_native(target, |recipe, state| recipe.pointer_capture(state))?
            .unwrap_or(false))
    }
    pub fn copy(
        &mut self,
        target: &paint::TargetIdentity,
        selection_only: bool,
    ) -> maka_ui_runtime::Result<Option<String>> {
        self.read_native(target, |recipe, state| recipe.copy(state, selection_only))?
            .unwrap_or(Ok(None))
    }
    pub fn present_focus(&mut self, target: &paint::TargetIdentity) -> bool {
        let Some(owner) = self.owners.get_mut(&target.mount.id()).filter(|owner| {
            owner.live && owner.presented && Rc::ptr_eq(&owner.identity, &target.owner)
        }) else {
            return false;
        };
        match catch_unwind(AssertUnwindSafe(|| {
            owner.recipe.present_focus(owner.state.as_mut())
        })) {
            Ok(offered) => offered,
            Err(_) => {
                owner.live = false;
                self.failed.insert(
                    target.mount.id(),
                    "terminal native focus presentation cleanup unconfirmed".into(),
                );
                false
            }
        }
    }
    pub fn present(&mut self, mount: &Mount, identity: &Rc<()>, geometry: Box<dyn Any>) {
        let Some(owner) = self
            .owners
            .get_mut(&mount.id())
            .filter(|owner| owner.live && owner.presented && Rc::ptr_eq(&owner.identity, identity))
        else {
            return;
        };
        match catch_unwind(AssertUnwindSafe(|| {
            owner.recipe.present(owner.state.as_mut(), geometry)
        })) {
            Ok(Some(observed)) => {
                if let Err(error) = owner.observe(observed) {
                    self.failed.insert(mount.id(), error.to_string());
                }
            }
            Ok(None) => {}
            Err(_) => {
                owner.live = false;
                self.failed.insert(
                    mount.id(),
                    "terminal native presentation cleanup unconfirmed".into(),
                );
            }
        }
    }
    pub fn presentation(&mut self, placed: &std::collections::BTreeSet<Uuid>) {
        for (id, owner) in &mut self.owners {
            let visible = placed.contains(id);
            if !owner.live || owner.presented == visible {
                continue;
            }
            owner.presented = visible;
            if catch_unwind(AssertUnwindSafe(|| {
                owner.recipe.presentation(owner.state.as_mut(), visible)
            }))
            .is_err()
            {
                owner.live = false;
                self.failed.insert(
                    *id,
                    "terminal native presentation cleanup unconfirmed".into(),
                );
            }
        }
    }
    pub fn capture(
        &mut self,
        target: &maka_ui_runtime::actions::CaptureTarget,
    ) -> maka_ui_runtime::Result<maka_ui_runtime::actions::NativeCapture> {
        let owner = self
            .owners
            .get(&target.mount().id())
            .filter(|owner| owner.live)
            .ok_or_else(|| Error::Invalid("terminal native owner unavailable".into()))?;
        match catch_unwind(AssertUnwindSafe(|| {
            use maka_ui_runtime::actions::{ContinuationWitness, NativeCapture};
            match target.output() {
                maka_plugin_core::contract::CaptureOutput::Resource { .. } => owner
                    .recipe
                    .capture(owner.state.as_ref(), target)
                    .map(NativeCapture::Resource),
                maka_plugin_core::contract::CaptureOutput::Continuation => {
                    let value = owner
                        .recipe
                        .capture_continuation(owner.state.as_ref(), target)?;
                    let bytes = value
                        .bytes()
                        .checked_add(std::mem::size_of::<OwnerWitness>())
                        .ok_or_else(|| Error::Invalid("continuation byte overflow".into()))?;
                    Ok(NativeCapture::Continuation(ContinuationWitness::new(
                        OwnerWitness {
                            identity: Rc::downgrade(&owner.identity),
                            value,
                        },
                        bytes,
                    )))
                }
            }
        })) {
            Ok(result) => result,
            Err(_) => {
                let error = "terminal native capture cleanup unconfirmed".to_owned();
                self.failed.insert(target.mount().id(), error.clone());
                Err(Error::Invalid(error))
            }
        }
    }
    pub fn validate_continuation(
        &mut self,
        target: &maka_ui_runtime::actions::CaptureTarget,
        recipe: &dyn Widget,
        witness: &maka_ui_runtime::actions::ContinuationWitness,
    ) -> maka_ui_runtime::Result<bool> {
        let Some(witness) = witness.downcast_ref::<OwnerWitness>() else {
            return Ok(false);
        };
        let Some(identity) = witness.identity.upgrade() else {
            return Ok(false);
        };
        let Some(owner) = self.owners.get_mut(&target.mount().id()).filter(|owner| {
            owner.live && Rc::ptr_eq(&owner.identity, &identity) && owner.kind == recipe.type_id()
        }) else {
            return Ok(false);
        };
        match catch_unwind(AssertUnwindSafe(|| {
            recipe.validate_continuation(owner.state.as_ref(), target, &witness.value)
        })) {
            Ok(result) => result,
            Err(_) => {
                owner.live = false;
                let error = "terminal native continuation cleanup unconfirmed".to_owned();
                self.failed.insert(target.mount().id(), error.clone());
                Err(Error::Invalid(error))
            }
        }
    }
}
impl Presenter {
    pub(super) fn sync_native(&mut self) -> maka_ui_runtime::Result<()> {
        self.sync_native_inner(true)
    }
    pub(super) fn progress_native(&mut self) -> maka_ui_runtime::Result<()> {
        self.sync_native_inner(false)
    }
    fn sync_native_inner(&mut self, prepare: bool) -> maka_ui_runtime::Result<()> {
        self.resources.deactivate(&self.application);
        self.resources.cancel_pastes(
            self.focus
                .as_ref()
                .filter(|_| self.window_focused)
                .map(|focus| focus.mount.id()),
        )?;
        let mut mounts = if self.application.is_retiring(&self.root)
            || self.application.frame(&self.root).is_err()
        {
            Vec::new()
        } else {
            self.application.mounted_subtree(&self.root)?
        };
        // Transfer completed original work before preflight reserves the next
        // command's capacity. A newer command cannot replace an uncaptured result.
        self.resources
            .collect_observations(&self.application, &mounts)?;
        let resources = &mut self.resources;
        let mut reservations = Vec::new();
        let committed = self.application.commit_ui(&self.root, |app, candidate| {
            reservations = resources.prepare_candidate(app, candidate)?;
            Ok(())
        })?;
        if committed {
            mounts = self.application.mounted_subtree(&self.root)?;
            for (mount, capacity) in reservations {
                if let Some(owner) = self.resources.owners.get_mut(&mount) {
                    owner.pending.adopt(capacity);
                }
            }
            self.resources.redraw = true;
        }
        if !self.surface.is_live() || self.application.is_retiring(&self.root) {
            return Ok(());
        }
        // The first pump boundary also covers local native focus changes. Later
        // boundaries need preparation only when a UI batch actually committed.
        let prepare = prepare || committed;
        if prepare && !self.resources.prepare(&self.application, &mounts)? {
            self.clear_hover()?;
            self.scene.clear();
            self.focus = None;
            self.drag = None;
            return Ok(());
        }
        let resources = &mut self.resources;
        self.application
            .capture_actions(&self.root, |target| resources.capture(target))?;
        self.resources.commit_mounts(&self.application, &mounts)?;
        let queues: Vec<_> = mounts
            .into_iter()
            .filter(|mount| !self.application.is_retiring(mount))
            .filter_map(|mount| {
                self.resources
                    .owners
                    .get(&mount.id())
                    .map(|owner| (mount, owner.pending.clone()))
            })
            .collect();
        for (mount, queue) in queues {
            if prepare {
                // Retain reserved capacity too, even when no observation is queued.
                queue.retain(|key| {
                    self.application
                        .frame(&mount)
                        .is_ok_and(|frame| frame.view.declares_input(key))
                });
            }
            if !queue.pending() {
                continue;
            }
            let revision = self.application.frame(&mount)?.revision;
            let root = self.application.mount_root(&mount)?;
            let basis = (self.application.frame(&root)?.revision, revision);
            let application = &mut self.application;
            let resources = &mut self.resources;
            let errors = queue.advance(
                basis,
                &mut (),
                |key, event, _| application.dispatch_observation(&mount, revision, key, event),
                |key, value, _| {
                    if let Some(owner) = resources
                        .owners
                        .get_mut(&mount.id())
                        .filter(|owner| owner.live)
                    {
                        owner.recipe.published(owner.state.as_mut(), key, value);
                    }
                },
            );
            for error in errors {
                self.report(&error);
            }
        }
        self.sync_modal_focus()?;
        self.apply_focus_exits()?;
        if let Some(target) = &self.drag
            && !self.resources.pointer_capture(target)?
        {
            self.drag = None;
        }
        Ok(())
    }
}

impl Resources {
    pub(super) fn hover(
        &mut self,
        target: &paint::Target,
        position: Option<ratatui::layout::Position>,
    ) -> maka_ui_runtime::Result<bool> {
        let Some(owner) = self
            .owners
            .get_mut(&target.mount.id())
            .filter(|owner| owner.live && std::rc::Rc::ptr_eq(&owner.identity, &target.owner))
        else {
            return Ok(false);
        };
        match catch_unwind(AssertUnwindSafe(|| {
            owner
                .recipe
                .hover(owner.state.as_mut(), &target.key, position)
        })) {
            Ok(changed) => Ok(changed),
            Err(_) => {
                owner.live = false;
                self.failed.insert(
                    target.mount.id(),
                    "terminal native hover cleanup unconfirmed".into(),
                );
                Err(maka_ui_runtime::Error::Invalid(
                    "terminal native hover cleanup unconfirmed".into(),
                ))
            }
        }
    }
}
