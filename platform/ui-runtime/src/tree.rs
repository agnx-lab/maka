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
    collections::{BTreeSet, VecDeque},
    time::Instant,
};

#[derive(Clone)]
pub(super) struct Child {
    binding: String,
    member: u32,
    pub mount: Mount,
    on: BTreeMap<String, String>,
}

#[derive(Clone)]
struct Subscription {
    parent: Mount,
    slot: String,
    on: BTreeMap<String, String>,
}
struct Route {
    parent: Mount,
    child: Mount,
    slot: String,
    handler: String,
}
struct Notification {
    route: Route,
    event: Emission,
}

pub(super) struct Mounted<V: ?Sized> {
    mount: Mount,
    pub frame: Frame<NativeView<V>, Arc<Value>>,
    pub props: Arc<Value>,
    pub children: BTreeMap<String, Child>,
    pub(super) subscriptions: BTreeMap<String, actions::subscriptions::Demand>,
}

pub(super) struct QueuedInput {
    mount: Mount,
    event: abi::ComponentInput,
    publication: Option<tokio::sync::oneshot::Sender<Result<()>>>,
}
pub(super) struct Feedback {
    pub consumer: Mount,
    pub request: Option<Uuid>,
    pub item: bool,
}
struct Call {
    mount: Mount,
    request: backend::Call,
    subscription: Option<Subscription>,
    depth: usize,
    basis: Option<(u64, Option<Surface>)>,
    root: bool,
}
struct FinishStage<V: ?Sized> {
    mount: Mount,
    actions: VecDeque<actions::Action>,
    declarations: VecDeque<actions::Action>,
    preparing: Option<actions::PreparingAction<V>>,
    demand: Option<actions::Action>,
    demands: BTreeMap<String, actions::subscriptions::Demand>,
    previous_subscriptions: BTreeMap<String, actions::subscriptions::Demand>,
}
/// Read-only original candidate. This borrows the pending batch; it is not a
/// second committed tree and cannot dispatch input, actions or native commands.
pub struct Candidate<'a, V: ?Sized> {
    changed: &'a BTreeMap<Uuid, Mounted<V>>,
}
impl<'a, V: ?Sized> Candidate<'a, V> {
    pub fn changed(&self) -> impl Iterator<Item = (&Mount, &Frame<NativeView<V>, Arc<Value>>)> {
        self.changed.values().map(|view| (&view.mount, &view.frame))
    }
}

/// A composed input is one transaction, not a sequence of published child
/// frames. Newly created mounts cannot be observed by a renderer until commit.
pub(super) struct Update<V: ?Sized> {
    changed: BTreeMap<Uuid, Mounted<V>>,
    created: Vec<Mount>,
    removed: BTreeMap<Uuid, Mount>,
    events: VecDeque<Notification>,
    actions: Vec<actions::Prepared<V>>,
    cancellations: Vec<(Mount, String)>,
    calls: VecDeque<Call>,
    finishes: VecDeque<FinishStage<V>>,
    pending: Option<(Call, PendingInvocation<V>)>,
    feedback: Option<Feedback>,
    ready: bool,
    native_prepared: bool,
    publication: Option<tokio::sync::oneshot::Sender<Result<()>>>,
}
impl<V: ?Sized> Update<V> {
    fn new() -> Self {
        Self {
            changed: BTreeMap::new(),
            created: Vec::new(),
            removed: BTreeMap::new(),
            events: VecDeque::new(),
            actions: Vec::new(),
            cancellations: Vec::new(),
            calls: VecDeque::new(),
            finishes: VecDeque::new(),
            pending: None,
            feedback: None,
            ready: false,
            native_prepared: false,
            publication: None,
        }
    }
    pub fn step(&mut self, depth: usize) -> Result<()> {
        if depth > 32 {
            return Err(Error::Invalid(
                "component mount depth limit exceeded".into(),
            ));
        }
        Ok(())
    }
    fn emit(
        &mut self,
        mount: &Mount,
        subscription: Option<Subscription>,
        events: Vec<Emission>,
    ) -> Result<()> {
        if let Some(subscription) = subscription {
            for event in events {
                if let Some(handler) = subscription.on.get(&event.name) {
                    self.events.push_back(Notification {
                        route: Route {
                            parent: subscription.parent.clone(),
                            child: mount.clone(),
                            slot: subscription.slot.clone(),
                            handler: handler.clone(),
                        },
                        event,
                    });
                }
            }
        }
        Ok(())
    }
}
impl<V: ?Sized> Drop for Update<V> {
    fn drop(&mut self) {
        if let Some(publication) = self.publication.take() {
            let _ = publication.send(Err(maka_plugin_core::Error::Retired.into()));
        }
    }
}

impl<V: ?Sized + 'static> Application<V> {
    pub fn root_kind(&self, mount: &Mount) -> Result<maka_plugin_core::composition::RootKind> {
        self.runtime
            .root_kind(mount)?
            .ok_or_else(|| Error::Invalid("bootstrap root required".into()))
    }

    pub fn can_recover(&self, mount: &Mount) -> bool {
        self.runtime
            .root_kind(mount)
            .is_ok_and(|kind| kind == Some(maka_plugin_core::composition::RootKind::Launch))
            && self.runtime.desired().recovery().is_some()
    }

    pub fn can_resume_launch(&self, mount: &Mount) -> bool {
        let status = self.deployment_status();
        self.root_kind(mount)
            .is_ok_and(|kind| kind == maka_plugin_core::composition::RootKind::Recovery)
            && !status.closing
            && !status.converging
            && status.error.is_none()
    }

    /// Bootstrap fallback uses the same ordinary mount transaction. Original
    /// failed mounts and execution resources remain owned by this Application.
    pub fn mount_bootstrap(&mut self, props: Value) -> Result<Mount> {
        match self.mount_launch(props.clone()) {
            Ok(root) => Ok(root),
            Err(error) if self.runtime.desired().recovery().is_some() => {
                self.deployment_error.get_or_insert(error.to_string());
                self.mount_selected_root(maka_plugin_core::composition::RootKind::Recovery, props)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn mount_bootstrap_ready(&mut self, props: Value) -> Result<Mount> {
        self.wait_ready().await?;
        let root = self.mount_bootstrap(props.clone())?;
        let launch = self.root_kind(&root)? == maka_plugin_core::composition::RootKind::Launch;
        match self.wait_frame(&root).await {
            Ok(()) => Ok(root),
            Err(error) if launch && self.runtime.desired().recovery().is_some() => {
                self.deployment_error.get_or_insert(error.to_string());
                let recovery = self.mount_selected_root(
                    maka_plugin_core::composition::RootKind::Recovery,
                    props,
                )?;
                self.wait_frame(&recovery).await?;
                Ok(recovery)
            }
            Err(error) => Err(error),
        }
    }

    /// Reconcile after the adapter confirms the old physical surface closed.
    /// A failed replacement may enter ordinary recovery without clearing the
    /// original failure. Pending activation alone still waits for readiness.
    pub fn mount_replacement_root(
        &mut self,
        mut kind: maka_plugin_core::composition::RootKind,
        props: Value,
    ) -> Result<Option<Mount>> {
        use maka_plugin_core::composition::RootKind;
        if let Err(error) = self.advance_deployment() {
            if self.runtime.desired().recovery().is_none() {
                return Err(error);
            }
            self.deployment_error.get_or_insert(error.to_string());
            kind = RootKind::Recovery;
        }
        if kind == RootKind::Launch {
            let status = self.deployment_status();
            if let Some(error) = status.error {
                if self.runtime.desired().recovery().is_none() {
                    return Err(Error::Invalid(error));
                }
                kind = RootKind::Recovery;
            } else if status.converging {
                return Ok(None);
            }
        }
        let mounted = match kind {
            RootKind::Launch => self.mount_bootstrap(props),
            RootKind::Recovery => self.mount_selected_root(kind, props),
        };
        match mounted {
            Err(Error::Core(maka_plugin_core::Error::NotReady(_)))
                if kind == RootKind::Recovery =>
            {
                Ok(None)
            }
            result => result.map(Some),
        }
    }

    pub fn mount_launch(&mut self, props: Value) -> Result<Mount> {
        self.mount_selected_root(maka_plugin_core::composition::RootKind::Launch, props)
    }

    /// Recovery is an ordinary explicitly selected root, with its own bindings.
    /// Mounting it never settles work retained by the failed original root.
    pub fn mount_selected_root(
        &mut self,
        kind: maka_plugin_core::composition::RootKind,
        props: Value,
    ) -> Result<Mount> {
        let mount = self.runtime.mount_root(kind)?;
        let mut update = Update::new();
        update.created.push(mount.clone());
        if let Err(error) = self.check_props(&mount, &props) {
            self.rollback(&update)?;
            return Err(error);
        }
        update.calls.push_back(Call {
            mount: mount.clone(),
            request: backend::Call::Mount {
                props: Arc::new(props),
            },
            subscription: None,
            depth: 0,
            basis: None,
            root: true,
        });
        self.install_update(update)?;
        Ok(mount)
    }

    /// A child receives input against its own revision and state. A parent input
    /// may stage props updates/new children, but cannot partially publish them.
    pub fn dispatch(
        &mut self,
        mount: &Mount,
        revision: u64,
        key: &str,
        event: Value,
    ) -> Result<()> {
        if self.is_retiring(mount) {
            return Err(maka_plugin_core::Error::Retired.into());
        }
        let current = self
            .views
            .get(&mount.id())
            .ok_or_else(|| Error::Invalid("mount has no committed frame".into()))?;
        if current.frame.revision != revision {
            return Err(Error::Invalid("stale or undeclared UI event".into()));
        }
        current.frame.view.validate_input(key, &event)?;
        self.enqueue_input(
            mount,
            abi::ComponentInput::Raw {
                key: key.into(),
                event,
            },
            None,
        )?;
        Ok(())
    }

    /// Native evidence remains in its original renderer's custody until this
    /// whole component transaction commits. Admission alone is not publication.
    pub fn dispatch_observation(
        &mut self,
        mount: &Mount,
        revision: u64,
        key: &str,
        event: Value,
    ) -> Result<super::backend::Job<()>> {
        if self.is_retiring(mount) {
            return Err(maka_plugin_core::Error::Retired.into());
        }
        let current = self
            .views
            .get(&mount.id())
            .ok_or_else(|| Error::Invalid("mount has no committed frame".into()))?;
        if current.frame.revision != revision {
            return Err(Error::Invalid("stale native observation".into()));
        }
        current.frame.view.validate_input(key, &event)?;
        let (publication, receipt) = tokio::sync::oneshot::channel();
        self.enqueue_input(
            mount,
            abi::ComponentInput::Raw {
                key: key.into(),
                event,
            },
            Some(publication),
        )?;
        Ok(super::backend::Job::new(async move {
            receipt
                .await
                .map_err(|_| Error::Invalid("original UI publication owner stopped".into()))?
        }))
    }

    fn enqueue_input(
        &mut self,
        mount: &Mount,
        event: abi::ComponentInput,
        publication: Option<tokio::sync::oneshot::Sender<Result<()>>>,
    ) -> Result<()> {
        self.ui_inputs.push_back(QueuedInput {
            mount: mount.clone(),
            event,
            publication,
        });
        self.advance_ui().map(|_| ())
    }

    pub(super) fn dispatch_action_result(
        &mut self,
        mount: &Mount,
        key: &str,
        event: Value,
        feedback: Feedback,
    ) -> Result<()> {
        self.dispatch_origin(
            mount,
            super::abi::ComponentInput::Action {
                key: key.into(),
                event,
            },
            Some(feedback),
            None,
        )
        .map(|_| ())
    }
    fn dispatch_origin(
        &mut self,
        mount: &Mount,
        event: super::abi::ComponentInput,
        feedback: Option<Feedback>,
        publication: Option<tokio::sync::oneshot::Sender<Result<()>>>,
    ) -> Result<()> {
        let mut update = Update::new();
        update.feedback = feedback;
        update.publication = publication;
        let current = self
            .views
            .get(&mount.id())
            .ok_or_else(|| Error::Invalid("mount has no committed frame".into()))?;
        let request = backend::Call::Event {
            state: current.frame.state.clone(),
            props: current.props.clone(),
            input: Arc::new(event),
        };
        let revision = current.frame.revision;
        let surface = current.frame.surface.clone();
        let depth = self.mount_depth(mount)?;
        let subscription = self.subscription(mount, &update)?;
        update.calls.push_back(Call {
            mount: mount.clone(),
            request,
            subscription,
            depth,
            basis: Some((revision, surface)),
            root: false,
        });
        self.install_update(update)?;
        // Physical input policy is declared by the native binding before enqueue.
        Ok(())
    }

    fn install_update(&mut self, update: Update<V>) -> Result<()> {
        if self.ui_update.is_some() {
            self.rollback(&update)?;
            return Err(Error::Invalid(
                "UI logical transaction is already pending".into(),
            ));
        }
        self.ui_update = Some(update);
        self.advance_ui()?;
        if self.ui_update.as_ref().is_some_and(|update| update.ready) {
            self.waker().wake_by_ref();
        }
        Ok(())
    }

    /// Run only ready work. The committed tree remains available to native input
    /// while this single logical transaction waits for its original worker.
    pub fn advance_ui(&mut self) -> Result<bool> {
        let Some(mut update) = self.ui_update.take() else {
            while let Some(input) = self.ui_inputs.pop_front() {
                if self.is_retiring(&input.mount) {
                    if let Some(publication) = input.publication {
                        let _ = publication.send(Err(maka_plugin_core::Error::Retired.into()));
                    }
                    continue;
                }
                self.dispatch_origin(&input.mount, input.event, None, input.publication)?;
                return Ok(true);
            }
            return Ok(false);
        };
        if update.ready {
            self.ui_update = Some(update);
            return Ok(false);
        }
        let wake = self.waker();
        let mut cx = Context::from_waker(&wake);
        // Yield long compositions to input and presentation without rejecting
        // their original transaction or counting asynchronous waits as work.
        let yield_at = Instant::now() + std::time::Duration::from_millis(4);
        let result = (|| {
            loop {
                if Instant::now() >= yield_at {
                    wake.wake_by_ref();
                    return Ok(false);
                }
                if let Some((call, mut pending)) = update.pending.take() {
                    let invocation = match self.poll_invocation(&mut pending, &mut cx) {
                        Poll::Pending => {
                            update.pending = Some((call, pending));
                            return Ok(false);
                        }
                        Poll::Ready(result) => result?,
                    };
                    if let Some((revision, surface)) = &call.basis {
                        Self::check_next(*revision, surface, &invocation.output.frame)?;
                    } else {
                        Self::check_initial(&invocation.output.frame)?;
                        if call.root {
                            self.validate_surface(&call.mount, &invocation.output.frame)?;
                        } else if invocation.output.frame.surface.is_some() {
                            return Err(Error::Invalid(
                                "embedded component cannot create a surface".into(),
                            ));
                        }
                    }
                    self.stage(
                        &call.mount,
                        call.request.props().clone(),
                        invocation,
                        call.subscription,
                        call.depth,
                        &mut update,
                    )?;
                    continue;
                }
                if let Some(call) = update.calls.pop_front() {
                    update.step(call.depth)?;
                    let previous = match &call.basis {
                        Some((revision, _)) => {
                            let current = self.current(&call.mount, &update)?;
                            (current.frame.revision == *revision)
                                .then(|| current.frame.view.clone())
                        }
                        None => None,
                    };
                    let pending =
                        self.begin_invoke(&call.mount, &call.request, previous.as_ref())?;
                    update.pending = Some((call, pending));
                    continue;
                }
                if let Some(finish) = update.finishes.pop_front() {
                    if !self.finish_stage(finish, &mut update, &mut cx)? {
                        return Ok(false);
                    }
                    continue;
                }
                if self.deliver_one(&mut update)? {
                    continue;
                }
                self.preflight(&mut update)?;
                if self.native_commit {
                    update.ready = true;
                    return Ok(false);
                }
                self.commit(&mut update)?;
                return Ok(true);
            }
        })();
        match result {
            Ok(false) => {
                self.ui_update = Some(update);
                Ok(false)
            }
            Ok(true) => {
                if let Some(publication) = update.publication.take() {
                    let _ = publication.send(Ok(()));
                }
                if let Some(feedback) = update.feedback.take()
                    && feedback.item
                    && let Some(id) = feedback.request
                {
                    self.acknowledge_update(id);
                }
                if !self.ui_inputs.is_empty() {
                    self.waker().wake_by_ref();
                }
                Ok(true)
            }
            Err(error) => self.reject_update(update, error),
        }
    }

    /// The embedding runs fallible native preparation on the original owner,
    /// then publishes the whole pending batch in this same bounded owner turn.
    pub fn commit_ui(
        &mut self,
        surface: &Mount,
        prepare: impl FnOnce(&Self, Candidate<'_, V>) -> Result<()>,
    ) -> Result<bool> {
        if let Some(update) = self.ui_update.as_ref().filter(|update| update.ready)
            && let Some(changed) = update.changed.values().next()
            && self.mount_root(&changed.mount)? != *surface
        {
            // Another physical owner cannot prepare or reject this batch.
            return Ok(false);
        }
        let Some(mut update) = self.ui_update.take() else {
            return Ok(false);
        };
        if !update.ready {
            self.ui_update = Some(update);
            return Ok(false);
        }
        update.native_prepared = true;
        let result = (|| {
            self.validate_commit_fence(&update)?;
            for changed in update.changed.values() {
                if self.mount_root(&changed.mount)? != *surface {
                    return Err(Error::Invalid(
                        "UI candidate crosses native surfaces".into(),
                    ));
                }
            }
            prepare(
                self,
                Candidate {
                    changed: &update.changed,
                },
            )?;
            self.commit(&mut update)
        })();
        match result {
            Ok(()) => {
                if let Some(publication) = update.publication.take() {
                    let _ = publication.send(Ok(()));
                }
                if let Some(feedback) = update.feedback.take()
                    && feedback.item
                    && let Some(id) = feedback.request
                {
                    self.acknowledge_update(id);
                }
                self.waker().wake_by_ref();
                Ok(true)
            }
            Err(error) => self.reject_update(update, error),
        }
    }
    fn reject_update(&mut self, mut update: Update<V>, error: Error) -> Result<bool> {
        self.rollback(&update)?;
        let retired_feedback = matches!(error, Error::Core(maka_plugin_core::Error::Retired))
            && update
                .feedback
                .as_ref()
                .is_some_and(|feedback| self.is_retiring(&feedback.consumer));
        if let Some(feedback) = update.feedback.take() {
            if feedback.item
                && let Some(id) = feedback.request
            {
                self.fail_update(id, "progress delivery failed".into());
            }
            // Actual mount close may precede its already accepted callback.
            // Reject this original feedback without re-retiring a closed mount.
            if !self.is_retiring(&feedback.consumer) {
                self.runtime.retire_mount(&feedback.consumer)?;
                self.sync_subscription_demand();
            }
        }
        if !self.ui_inputs.is_empty() {
            self.waker().wake_by_ref();
        }
        if let Some(publication) = update.publication.take() {
            let _ = publication.send(Err(error));
            Ok(false)
        } else if retired_feedback {
            Ok(false)
        } else {
            Err(error)
        }
    }

    /// Startup may read only its original prepared declaration to create the
    /// platform surface. No input or painting can use it before commit_ui.
    pub fn prepared_frame(&self, mount: &Mount) -> Result<&Frame<NativeView<V>, Arc<Value>>> {
        self.ui_update
            .as_ref()
            .filter(|update| update.ready)
            .and_then(|update| update.changed.get(&mount.id()))
            .filter(|view| view.mount == *mount)
            .map(|view| &view.frame)
            .ok_or_else(|| Error::Invalid("mount has no prepared UI frame".into()))
    }
    pub async fn wait_prepared(&mut self, mount: &Mount) -> Result<()> {
        std::future::poll_fn(|cx| {
            self.set_waker(cx.waker().clone());
            if let Err(error) = self.advance_ui() {
                return Poll::Ready(Err(error));
            }
            if self.prepared_frame(mount).is_ok()
                || (self.ui_update.is_none() && self.frame(mount).is_ok())
            {
                Poll::Ready(Ok(()))
            } else if self.ui_update.is_some() {
                Poll::Pending
            } else {
                Poll::Ready(Err(Error::Invalid(
                    "initial UI frame was not prepared".into(),
                )))
            }
        })
        .await
    }
    pub async fn mount_bootstrap_prepared(&mut self, props: Value) -> Result<Mount> {
        self.wait_ready().await?;
        let mount = self.mount_bootstrap(props.clone())?;
        let launch = self
            .root_kind(&mount)
            .is_ok_and(|kind| kind == maka_plugin_core::composition::RootKind::Launch);
        match self.wait_prepared(&mount).await {
            Ok(()) => Ok(mount),
            Err(error) if launch && self.runtime.desired().recovery().is_some() => {
                self.deployment_error.get_or_insert(error.to_string());
                let recovery = self.mount_selected_root(
                    maka_plugin_core::composition::RootKind::Recovery,
                    props,
                )?;
                self.wait_prepared(&recovery).await.map_err(|second| {
                    Error::Invalid(format!(
                        "initial mount failed: {error}; recovery failed: {second}"
                    ))
                })?;
                Ok(recovery)
            }
            Err(error) => Err(error),
        }
    }

    pub fn logical_pending(&self) -> bool {
        self.ui_update.is_some()
    }

    /// Whether admitted UI input or its original logical transaction is pending.
    /// This does not include independent service executions or native interaction.
    pub fn ui_pending(&self) -> bool {
        self.logical_pending() || !self.ui_inputs.is_empty()
    }

    fn check_initial(frame: &Frame<NativeView<V>>) -> Result<()> {
        if frame.revision != 0 {
            return Err(Error::Invalid("initial UI revision must be zero".into()));
        }
        Ok(())
    }
    fn check_next(
        revision: u64,
        surface: &Option<Surface>,
        frame: &Frame<NativeView<V>>,
    ) -> Result<()> {
        if revision.checked_add(1) != Some(frame.revision) {
            return Err(Error::Invalid("UI revision must advance once".into()));
        }
        if &frame.surface != surface {
            return Err(Error::Invalid(
                "surface configuration is fixed for this mount".into(),
            ));
        }
        Ok(())
    }
    fn check_props(&mut self, mount: &Mount, value: &Value) -> Result<()> {
        let execution = self.runtime.admit_mount(mount)?;
        let Export::Component { props, .. } = execution.value() else {
            unreachable!()
        };
        let result = props
            .validate(value)
            .map_err(|error| Error::Invalid(error.to_string()));
        self.runtime.settle(&execution, Ok(()))?;
        result
    }
    pub(super) fn provider(&self, mount: &Mount) -> Result<Activation> {
        Ok(self.runtime.mount_provider(mount)?.clone())
    }
    pub(super) fn mount_depth(&self, mount: &Mount) -> Result<usize> {
        Ok(self.mount_chain(mount)?.len() - 1)
    }
    /// The owning root comes from the mount parent chain, not display reachability.
    /// Detached mounts remain attributed to their original native surface.
    pub fn mount_root(&self, mount: &Mount) -> Result<Mount> {
        Ok(self
            .mount_chain(mount)?
            .pop()
            .expect("nonempty mount chain"))
    }
    /// Whether the exact live mount chain remains on the committed visible route.
    pub fn mount_visible(&self, mount: &Mount) -> bool {
        let Ok(chain) = self.mount_chain(mount) else {
            return false;
        };
        chain.iter().all(|mount| {
            !self.is_retiring(mount)
                && self
                    .frame(mount)
                    .is_ok_and(|frame| frame.view.root_visible())
        }) && chain.windows(2).all(|edge| {
            let (child, parent) = (&edge[0], &edge[1]);
            self.frame(parent).is_ok_and(|frame| {
                frame.view.visible_slots().any(|slot| {
                    self.child_mount(parent, &slot.key)
                        .is_ok_and(|mount| mount == child)
                })
            })
        })
    }
    pub(super) fn mount_chain(&self, mount: &Mount) -> Result<Vec<Mount>> {
        let mut current = mount;
        let mut chain = vec![mount.clone()];
        while let Some(parent) = self.runtime.mount_parent(current)? {
            chain.push(parent.clone());
            current = parent;
        }
        Ok(chain)
    }
    pub(super) fn current<'a>(
        &'a self,
        mount: &Mount,
        update: &'a Update<V>,
    ) -> Result<&'a Mounted<V>> {
        update
            .changed
            .get(&mount.id())
            .or_else(|| self.views.get(&mount.id()))
            .filter(|view| view.mount == *mount && !update.removed.contains_key(&mount.id()))
            .ok_or_else(|| Error::Invalid("component is absent from the transaction".into()))
    }
    fn subscription(&self, mount: &Mount, update: &Update<V>) -> Result<Option<Subscription>> {
        let Some(parent) = self.runtime.mount_parent(mount)?.cloned() else {
            return Ok(None);
        };
        let view = self.current(&parent, update)?;
        Ok(view
            .children
            .iter()
            .find(|(_, child)| child.mount == *mount)
            .map(|(slot, child)| Subscription {
                parent,
                slot: slot.clone(),
                on: child.on.clone(),
            }))
    }
    fn detach(&self, mount: &Mount, update: &mut Update<V>) {
        let statuses = self.runtime.mount_statuses();
        update.removed.insert(mount.id(), mount.clone());
        loop {
            let before = update.removed.len();
            for status in &statuses {
                if status
                    .parent
                    .as_ref()
                    .is_some_and(|parent| update.removed.contains_key(&parent.id()))
                {
                    update
                        .removed
                        .insert(status.mount.id(), status.mount.clone());
                }
            }
            if before == update.removed.len() {
                break;
            }
        }
        update
            .changed
            .retain(|id, _| !update.removed.contains_key(id));
        for action in &mut update.actions {
            action.invalidate(|e| {
                !update.removed.contains_key(&e.parent.id())
                    && !update.removed.contains_key(&e.child.id())
            });
        }
        update.events.retain(|notification| {
            !update.removed.contains_key(&notification.route.parent.id())
                && !update.removed.contains_key(&notification.route.child.id())
        });
    }
    fn check_subscription(&mut self, mount: &Mount, on: &BTreeMap<String, String>) -> Result<()> {
        let execution = self.runtime.admit_mount(mount)?;
        let Export::Component { events, .. } = execution.value() else {
            unreachable!()
        };
        let valid = on.keys().all(|name| events.contains_key(name));
        self.runtime.settle(&execution, Ok(()))?;
        if valid {
            Ok(())
        } else {
            Err(Error::Invalid(
                "subscription to undeclared component event".into(),
            ))
        }
    }
    fn stage(
        &mut self,
        mount: &Mount,
        props: Arc<Value>,
        invocation: Invocation<V>,
        subscription: Option<Subscription>,
        depth: usize,
        update: &mut Update<V>,
    ) -> Result<()> {
        let Invocation { output } = invocation;
        let previous_subscriptions = self
            .current(mount, update)
            .map(|v| v.subscriptions.clone())
            .unwrap_or_default();
        let declarations = output.subscriptions;
        for key in output.cancel {
            if key.is_empty() || key.len() > 128 || output.actions.iter().any(|a| a.key == key) {
                return Err(Error::Invalid("invalid mutation cancellation key".into()));
            }
            update.cancellations.push((mount.clone(), key));
        }
        let actions = output.actions;
        // No one-shot feedback can be confused with a second request or a
        // subscription. Distinct keys permit independent concurrent requests.
        let mut keys = std::collections::BTreeSet::new();
        for action in &actions {
            if !keys.insert(&action.key)
                || declarations.iter().any(|s| s.key == action.key)
                || previous_subscriptions.contains_key(&action.key)
            {
                return Err(Error::Invalid("action feedback key already in use".into()));
            }
        }
        for action in actions.iter().chain(&declarations) {
            if self.actions.key_pending(mount, &action.key)
                || update
                    .actions
                    .iter()
                    .any(|a| !a.subscription && a.consumer == *mount && a.key == action.key)
            {
                return Err(Error::Invalid("action feedback key already in use".into()));
            }
        }
        update.emit(mount, subscription, output.events)?;
        let frame = output.frame.map_state(Arc::new);
        let mut previous = update
            .changed
            .get(&mount.id())
            .or_else(|| self.views.get(&mount.id()))
            .map(|view| view.children.clone())
            .unwrap_or_default();
        let mut children = BTreeMap::new();
        let mut desired = Vec::new();
        frame.view.components(&mut desired);
        for reference in desired {
            let old = previous.remove(reference.key);
            let retained = old.as_ref().filter(|child| {
                child.binding == reference.binding
                    && child.member == reference.member
                    && !self.is_retiring(&child.mount)
                    && !update.removed.contains_key(&child.mount.id())
            });
            let subscription = Some(Subscription {
                parent: mount.clone(),
                slot: reference.key.into(),
                on: reference.on.clone(),
            });
            let child_mount = if let Some(old) = retained {
                self.check_subscription(&old.mount, reference.on)?;
                let child = self.current(&old.mount, update)?;
                if child.props.as_ref() != reference.props {
                    let revision = child.frame.revision;
                    let surface = child.frame.surface.clone();
                    let request = backend::Call::Update {
                        state: child.frame.state.clone(),
                        props: Arc::new(reference.props.clone()),
                    };
                    self.check_props(&old.mount, reference.props)?;
                    update.step(depth + 1)?;
                    update.calls.push_back(Call {
                        mount: old.mount.clone(),
                        request,
                        subscription,
                        depth: depth + 1,
                        basis: Some((revision, surface)),
                        root: false,
                    });
                }
                old.mount.clone()
            } else {
                if let Some(old) = old {
                    self.detach(&old.mount, update);
                }
                let provider = self.provider(mount)?;
                let capabilities = self.runtime.bind(&provider, reference.binding)?;
                let capability = capabilities
                    .get(reference.member as usize)
                    .ok_or_else(|| Error::Invalid("component binding member unavailable".into()))?;
                let child = self.runtime.mount_component(capability, Some(mount))?;
                update.created.push(child.clone());
                self.check_subscription(&child, reference.on)?;
                self.check_props(&child, reference.props)?;
                update.step(depth + 1)?;
                update.calls.push_back(Call {
                    mount: child.clone(),
                    request: backend::Call::Mount {
                        props: Arc::new(reference.props.clone()),
                    },
                    subscription,
                    depth: depth + 1,
                    basis: None,
                    root: false,
                });
                child
            };
            children.insert(
                reference.key.into(),
                Child {
                    binding: reference.binding.into(),
                    member: reference.member,
                    mount: child_mount,
                    on: reference.on.clone(),
                },
            );
        }
        for child in previous.into_values() {
            self.detach(&child.mount, update);
        }
        // Revoke queued routes permanently at each edge diff. Later restoring
        // the same subscription cannot resurrect a previously detached event.
        for action in &mut update.actions {
            action.invalidate(|e| {
                !update.removed.contains_key(&e.parent.id())
                    && !update.removed.contains_key(&e.child.id())
            });
        }
        update.events.retain(|notification| {
            notification.route.parent != *mount
                || children.get(&notification.route.slot).is_some_and(|child| {
                    child.mount == notification.route.child
                        && child.on.get(&notification.event.name)
                            == Some(&notification.route.handler)
                })
        });
        for action in &mut update.actions {
            action.invalidate(|e| {
                e.parent != *mount || children.get(&e.slot).is_some_and(|c| c.mount == e.child)
            });
        }
        update.changed.insert(
            mount.id(),
            Mounted {
                mount: mount.clone(),
                frame,
                props,
                children,
                subscriptions: BTreeMap::new(),
            },
        );
        update.finishes.push_back(FinishStage {
            mount: mount.clone(),
            actions: actions.into(),
            declarations: declarations.into(),
            previous_subscriptions,
            preparing: None,
            demand: None,
            demands: BTreeMap::new(),
        });
        Ok(())
    }
    fn finish_stage(
        &mut self,
        mut finish: FinishStage<V>,
        update: &mut Update<V>,
        cx: &mut Context<'_>,
    ) -> Result<bool> {
        loop {
            if let Some(mut preparing) = finish.preparing.take() {
                let prepared = match self.poll_prepare_action(&mut preparing, update, cx) {
                    Poll::Pending => {
                        finish.preparing = Some(preparing);
                        update.finishes.push_front(finish);
                        return Ok(false);
                    }
                    Poll::Ready(result) => result?,
                };
                if let Some(action) = finish.demand.take() {
                    finish.demands.insert(
                        action.key.clone(),
                        actions::subscriptions::Demand {
                            action,
                            request: prepared.id,
                        },
                    );
                }
                update.actions.push(prepared);
                continue;
            }
            if let Some(action) = finish.actions.pop_front() {
                finish.preparing =
                    Some(self.begin_prepare_action(&finish.mount, action, update, false)?);
                continue;
            }
            if let Some(action) = finish.declarations.pop_front() {
                if finish.demands.contains_key(&action.key) {
                    return Err(Error::Invalid("duplicate subscription key".into()));
                }
                let old = self
                    .views
                    .get(&finish.mount.id())
                    .and_then(|view| view.subscriptions.get(&action.key))
                    .filter(|old| old.action == action)
                    .or_else(|| {
                        finish
                            .previous_subscriptions
                            .get(&action.key)
                            .filter(|old| old.action == action)
                    });
                if let Some(old) = old {
                    finish.demands.insert(action.key.clone(), old.clone());
                } else {
                    finish.preparing = Some(self.begin_prepare_action(
                        &finish.mount,
                        action.clone(),
                        update,
                        true,
                    )?);
                    finish.demand = Some(action);
                }
                continue;
            }
            update
                .changed
                .get_mut(&finish.mount.id())
                .expect("staged owner")
                .subscriptions = finish.demands;
            return Ok(true);
        }
    }
    fn deliver_one(&mut self, update: &mut Update<V>) -> Result<bool> {
        while let Some(notification) = update.events.pop_front() {
            let route = notification.route;
            let current = self.current(&route.parent, update)?;
            if !current.children.get(&route.slot).is_some_and(|child| {
                child.mount == route.child
                    && child.on.get(&notification.event.name) == Some(&route.handler)
            }) {
                continue;
            }
            let request = backend::Call::Event {
                state: current.frame.state.clone(),
                props: current.props.clone(),
                input: Arc::new(abi::ComponentInput::Component {
                    key: route.handler.clone(),
                    child: route.slot.clone(),
                    name: notification.event.name,
                    event: notification.event.value,
                }),
            };
            let revision = current.frame.revision;
            let surface = current.frame.surface.clone();
            let depth = self.mount_depth(&route.parent)?;
            let subscription = self.subscription(&route.parent, update)?;
            update.step(depth)?;
            update.calls.push_back(Call {
                mount: route.parent,
                request,
                subscription,
                depth,
                basis: Some((revision, surface)),
                root: false,
            });
            return Ok(true);
        }
        Ok(false)
    }
    fn discard_private(&mut self, update: &mut Update<V>) -> Result<()> {
        let private: Vec<_> = update
            .created
            .iter()
            .rev()
            .filter(|mount| update.removed.contains_key(&mount.id()))
            .cloned()
            .collect();
        for mount in private {
            self.runtime.retire_mount(&mount)?;
            // Never published, never exposed to a renderer, no native effects.
            self.runtime.complete_mount(&mount, Ok(()))?;
            update.created.retain(|created| *created != mount);
            update.removed.remove(&mount.id());
        }
        Ok(())
    }
    fn validate_render_depth(&self, update: &Update<V>) -> Result<()> {
        let Some(limit) = self.render_depth_limit else {
            return Ok(());
        };
        // Walk the proposed whole tree, including retained ancestors and children.
        // Removed/retiring roots are absent from admission. Their already
        // validated frames can remain in custody during native cleanup, which
        // may have completed some children before their root.
        // Hidden slots retain custody but draw nothing.
        let admitted: BTreeSet<_> = self
            .runtime
            .mount_statuses()
            .into_iter()
            .filter(|status| status.phase != Phase::Retiring)
            .map(|status| status.mount.id())
            .collect();
        let mut roots = BTreeMap::new();
        for view in self.views.values().chain(update.changed.values()) {
            if view.frame.surface.is_some()
                && admitted.contains(&view.mount.id())
                && !update.removed.contains_key(&view.mount.id())
            {
                roots.insert(view.mount.id(), view.mount.clone());
            }
        }
        let mut pending: Vec<_> = roots.into_values().map(|mount| (mount, 0u16)).collect();
        while let Some((mount, enclosing)) = pending.pop() {
            let mounted = self.current(&mount, update)?;
            let view = &mounted.frame.view;
            let total = enclosing
                .checked_add(view.render_depth)
                .ok_or_else(|| Error::Invalid("native render depth overflow".into()))?;
            if total > limit {
                return Err(Error::Invalid("composed native render depth limit".into()));
            }
            for slot in view.visible_slots() {
                let cost = view.slot_render_depth.get(&slot.key).copied().unwrap_or(0);
                let child = mounted
                    .children
                    .get(&slot.key)
                    .ok_or_else(|| Error::Invalid("native slot has no proposed child".into()))?;
                pending.push((child.mount.clone(), enclosing + cost));
            }
        }
        Ok(())
    }
    fn preflight(&mut self, update: &mut Update<V>) -> Result<()> {
        self.validate_render_depth(update)?;
        self.discard_private(update)?;
        self.validate_commit_fence(update)
    }
    fn validate_commit_fence(&mut self, update: &Update<V>) -> Result<()> {
        for view in update.changed.values() {
            let execution = self.runtime.admit_mount(&view.mount)?;
            self.runtime.settle(&execution, Ok(()))?;
        }
        Ok(())
    }
    fn commit(&mut self, update: &mut Update<V>) -> Result<()> {
        self.validate_commit_fence(update)?;
        for mount in &update.created {
            self.runtime.publish_mount(mount)?;
        }
        for mount in update.removed.values() {
            self.runtime.retire_mount(mount)?;
        }
        self.views.append(&mut update.changed);
        self.invalidate_pending_captures();
        update.actions.retain(|action| {
            !action.subscription
                || self.subscription_wanted(&action.consumer, &action.key, action.id)
        });
        self.actions.queue.retain(|action| {
            !action.subscription
                || self
                    .views
                    .get(&action.consumer.id())
                    .and_then(|view| view.subscriptions.get(&action.key))
                    .is_some_and(|demand| demand.request == action.id)
        });
        self.actions.queue.extend(update.actions.drain(..));
        for (consumer, key) in update.cancellations.drain(..) {
            self.cancel_mutation(&consumer, &key);
        }
        self.sync_subscription_demand();
        Ok(())
    }
    fn rollback(&mut self, update: &Update<V>) -> Result<()> {
        for mount in update.created.iter().rev() {
            self.runtime.retire_mount(mount)?;
            // A pure failed candidate has no native resources. Once the
            // original renderer attempted preparation, its actual cleanup
            // receipt must settle even provisional mounts.
            if !update.native_prepared {
                self.runtime.complete_mount(mount, Ok(()))?;
            }
        }
        Ok(())
    }
    /// Read the committed child identity. The renderer uses this exact mount,
    /// not a namespace or an implicit global component lookup.
    pub fn child_mount(&self, parent: &Mount, key: &str) -> Result<&Mount> {
        self.views
            .get(&parent.id())
            .filter(|view| view.mount == *parent)
            .and_then(|view| view.children.get(key))
            .map(|child| &child.mount)
            .ok_or_else(|| Error::Invalid("component child is not committed".into()))
    }
    /// Read the exact committed display subtree, including retired components
    /// still displayed pending recovery. Detached mounts are absent.
    pub fn mounted_subtree(&self, root: &Mount) -> Result<Vec<Mount>> {
        let mut mounts = Vec::new();
        let mut pending = vec![root.clone()];
        while let Some(mount) = pending.pop() {
            let view = self
                .views
                .get(&mount.id())
                .filter(|view| view.mount == mount)
                .ok_or_else(|| Error::Invalid("component subtree is not committed".into()))?;
            pending.extend(view.children.values().map(|child| child.mount.clone()));
            mounts.push(mount);
        }
        Ok(mounts)
    }
    /// Removed/faulted mounts remain fenced until the renderer supplies real
    /// resource close acknowledgements. Leaves precede parents for settlement.
    pub fn retiring_mounts(&self) -> Vec<Mount> {
        let mut mounts: Vec<_> = self.runtime.retiring_mounts().collect();
        mounts.sort_by_key(|mount| {
            let mut depth = 0;
            let mut current = mount;
            while let Ok(Some(parent)) = self.runtime.mount_parent(current) {
                depth += 1;
                current = parent;
            }
            std::cmp::Reverse(depth)
        });
        mounts
    }
}
