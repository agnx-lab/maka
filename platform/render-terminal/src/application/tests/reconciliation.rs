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
use maka_event_log::{
    EventLog,
    root::{RootNamespaces, RootOwner},
};
use maka_plugin_core::contract::{Contract, Interface, Method, MethodMode, Transport};
use maka_ui_runtime::{
    NativeCapability, NativeExport,
    actions::{NativeService, ServiceFuture},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn store(runtime: &tokio::runtime::Runtime, dir: &std::path::Path) -> Arc<EventLog> {
    let ns = RootNamespaces {
        ownership: dir.join("owners"),
        control: dir.join("control"),
    };
    Arc::new(
        runtime
            .block_on(EventLog::for_root(Arc::new(
                RootOwner::create(&dir.join("root"), &ns).unwrap(),
            )))
            .unwrap(),
    )
}
fn replacement(mut input: Input, mut packages: Vec<Package>) -> (Input, Vec<Package>) {
    let index = packages
        .iter()
        .position(|p| p.manifest().id == "dynamic.app")
        .unwrap();
    let old = packages.remove(index);
    let source = std::str::from_utf8(old.file("index.mjs").unwrap())
        .unwrap()
        .replace("count:props.count??0", "count:props.count??100");
    assert_ne!(source.as_bytes(), old.file("index.mjs").unwrap());
    let package = Package::new(BTreeMap::from([
        (
            maka_packages::MANIFEST_FILE.into(),
            old.file(maka_packages::MANIFEST_FILE).unwrap().to_vec(),
        ),
        ("index.mjs".into(), source.into_bytes()),
    ]))
    .unwrap();
    input.instances.get_mut("app").unwrap().package = package.application_ref().into();
    packages.push(package);
    (input, packages)
}
struct Held {
    started: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
    calls: Arc<AtomicUsize>,
    settled: Arc<AtomicUsize>,
}
impl NativeService for Held {
    fn call(&self, _: maka_ui_runtime::actions::ServiceRequest) -> ServiceFuture {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let started = self.started.clone();
        let release = self.release.clone();
        let settled = self.settled.clone();
        Box::pin(async move {
            started.add_permits(1);
            release.acquire().await.unwrap().forget();
            settled.fetch_add(1, Ordering::SeqCst);
            ServiceCompletion {
                result: Ok(Value::Null),
                cleanup: Ok(()),
            }
        })
    }
}
#[test]
fn root_handoff_keeps_original_job_and_requires_exact_restoration_receipt() {
    for replace_provider in [false, true] {
        let contract = Contract {
            name: "test.Service".into(),
            version: 1,
            transport: Transport::Local,
            interface: Interface::Service {
                methods: BTreeMap::from([(
                    "run".into(),
                    Method {
                        progress: None,
                        mode: MethodMode::Query,
                        input: json!({"type":"object"}),
                        output: json!({}),
                        resources: BTreeMap::new(),
                    },
                )]),
            },
        };
        let (runtime, input, packages) = service_fixture(
            "const out=frame({count:state.count+1,revision:state.revision+1});if(event.kind==='raw')out.actions.push({key:'finished',binding:'service',member:0,method:'run',input:{},resources:{}});return out;",
            &contract,
        );
        let dir = tempfile::tempdir().unwrap();
        let store = store(&runtime, dir.path());
        let deployment = Uuid::new_v4();
        let original = runtime
            .block_on(store.commit_application(
                deployment,
                0,
                input.clone(),
                vec![],
                packages.clone(),
            ))
            .unwrap();
        let started = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let settled = Arc::new(AtomicUsize::new(0));
        let service = Arc::new(Held {
            started: started.clone(),
            release: release.clone(),
            calls: calls.clone(),
            settled: settled.clone(),
        });
        let mut native = registry();
        native
            .register("test.service", move |_, _| {
                Ok(BTreeMap::from([(
                    "Service".into(),
                    NativeExport {
                        contract: contract.clone(),
                        value: NativeCapability::Service(service.clone()),
                    },
                )]))
            })
            .unwrap();
        let mut tui = runtime
            .block_on(Presenter::start_from_store(
                store.clone(),
                original.clone(),
                runtime.handle().clone(),
                native,
                json!({}),
                vec![],
                maka_ui_js::execution(Default::default()),
            ))
            .unwrap();
        draw(&mut tui, 30, 12);
        tui.input_settled(key(KeyCode::Enter)).unwrap();
        tui.pump_settled().unwrap();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), started.acquire())
                .await
                .unwrap()
                .unwrap()
                .forget();
        });
        let old = tui.surface();
        let (mut input, packages) = replacement(input, packages);
        if replace_provider {
            input
                .instances
                .get_mut("service")
                .unwrap()
                .configuration
                .values
                .insert("revision".into(), json!(2));
        }
        let next = runtime
            .block_on(store.commit_application(deployment, 1, input, vec![], packages))
            .unwrap();
        let update = runtime
            .block_on(tui.load_committed_update(next.clone()).unwrap())
            .unwrap();
        tui.adopt_update(update, vec![]).unwrap();
        assert!(tui.requires_surface_replacement());
        assert!(tui.try_replace_surface(json!({})).is_err());
        tui.pump_settled().unwrap();
        assert!(tui.application.frame(&old).is_ok());
        assert_eq!(tui.jobs.len(), 1);
        // Low-level owner receipt seam: this seam alone does not prove actual Guard/PTY restoration.
        tui.surface_closed(&old, Ok(())).unwrap();
        assert!(tui.application.frame(&old).is_err());
        assert_eq!(tui.jobs.len(), 1);
        assert_eq!(settled.load(Ordering::SeqCst), 0);
        let resumed = if replace_provider {
            // This original provider still holds accepted work; waiting for a
            // replacement here would hide the boundary the fixture must prove.
            tui.try_replace_surface(json!({})).unwrap()
        } else {
            tui.replace_surface_settled(json!({})).unwrap()
        };
        assert_eq!(resumed, !replace_provider);
        if replace_provider {
            // Only the provider actually holding accepted work gates its replacement.
            assert_eq!(tui.deployment_status().effective, Some(original));
        } else {
            assert_eq!(tui.deployment_status().effective, Some(next.clone()));
            assert_eq!(tui.jobs.len(), 1);
        }
        release.add_permits(1);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < deadline);
            tui.pump_settled().unwrap();
            if resumed {
                if tui.jobs.is_empty() {
                    break;
                }
            } else if tui.try_replace_surface(json!({})).unwrap() {
                break;
            }
            std::thread::yield_now();
        }
        let fresh = tui.surface();
        assert_ne!(fresh, old);
        assert_eq!(tui.deployment_status().effective, Some(next));
        assert_eq!(tui.application.frame(&fresh).unwrap().state["count"], 100);
        assert!(tui.jobs.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(settled.load(Ordering::SeqCst), 1);
        assert!(tui.scene.is_empty());
        assert!(tui.focus.is_none());
        assert!(tui.surface_closed(&old, Ok(())).is_err());
        assert!(tui.surface_recovered(&old).is_err());
        assert!(!tui.application.is_retiring(&fresh));
        draw(&mut tui, 30, 12);
        tui.surface_closed(&fresh, Ok(())).unwrap();
        tui.shutdown_settled().unwrap();
        drop(tui);
        runtime
            .block_on(Arc::try_unwrap(store).ok().unwrap().close())
            .unwrap();
    }
}

#[test]
fn unrelated_adoption_then_original_realm_fault_is_exit_not_handoff() {
    for fault_first in [false, true] {
        let (runtime, mut input, packages) = fixture("throw Error('original realm failed');");
        let dir = tempfile::tempdir().unwrap();
        let store = store(&runtime, dir.path());
        let deployment = Uuid::new_v4();
        let original = runtime
            .block_on(store.commit_application(
                deployment,
                0,
                input.clone(),
                vec![],
                packages.clone(),
            ))
            .unwrap();
        let mut tui = runtime
            .block_on(Presenter::start_from_store(
                store.clone(),
                original,
                runtime.handle().clone(),
                registry(),
                json!({}),
                vec![],
                maka_ui_js::execution(Default::default()),
            ))
            .unwrap();
        let old = tui.surface();
        if fault_first {
            draw(&mut tui, 30, 12);
            assert!(tui.input_settled(key(KeyCode::Enter)).is_err());
        }
        let mut extra = input.instances["surface"].clone();
        extra.key = Uuid::new_v4();
        input.instances.insert("extra".into(), extra);
        let next = runtime
            .block_on(store.commit_application(deployment, 1, input, vec![], packages))
            .unwrap();
        let update = runtime
            .block_on(tui.load_committed_update(next).unwrap())
            .unwrap();
        tui.adopt_update(update, vec![]).unwrap();
        tui.pump_settled().unwrap();
        if !fault_first {
            draw(&mut tui, 30, 12);
            assert!(tui.input_settled(key(KeyCode::Enter)).is_err());
        }
        assert!(tui.application.is_retiring(&old));
        assert!(!tui.requires_surface_replacement());
        tui.surface_closed(&old, Ok(())).unwrap();
        assert!(tui.try_replace_surface(json!({})).is_err());
        tui.shutdown_settled().unwrap();
        drop(tui);
        runtime
            .block_on(Arc::try_unwrap(store).ok().unwrap().close())
            .unwrap();
    }
}

#[test]
fn failed_original_restore_blocks_handoff_and_duplicate_receipt_cannot_clear_it() {
    let (runtime, input, packages) = fixture("return frame({...state,revision:state.revision+1});");
    let dir = tempfile::tempdir().unwrap();
    let store = store(&runtime, dir.path());
    let deployment = Uuid::new_v4();
    let original = runtime
        .block_on(store.commit_application(deployment, 0, input.clone(), vec![], packages.clone()))
        .unwrap();
    let mut tui = runtime
        .block_on(Presenter::start_from_store(
            store.clone(),
            original,
            runtime.handle().clone(),
            registry(),
            json!({}),
            vec![],
            maka_ui_js::execution(Default::default()),
        ))
        .unwrap();
    let old = tui.surface();
    let (input, packages) = replacement(input, packages);
    let next = runtime
        .block_on(store.commit_application(deployment, 1, input, vec![], packages))
        .unwrap();
    let update = runtime
        .block_on(tui.load_committed_update(next).unwrap())
        .unwrap();
    tui.adopt_update(update, vec![]).unwrap();
    assert!(
        tui.surface_closed(&old, Err("actual Guard restoration failed".into()))
            .is_err()
    );
    assert!(tui.surface_closed(&old, Ok(())).is_err());
    assert!(tui.try_replace_surface(json!({})).is_err());
    assert_eq!(tui.surface(), old);
    assert!(tui.application.frame(&old).is_ok());
    assert!(!tui.can_shutdown());
    // Only an explicit successful original Guard restoration retry may recover it.
    tui.surface_recovered(&old).unwrap();
    assert!(tui.replace_surface_settled(json!({})).unwrap());
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    tui.shutdown_settled().unwrap();
    drop(tui);
    runtime
        .block_on(Arc::try_unwrap(store).ok().unwrap().close())
        .unwrap();
}

#[test]
fn failed_original_native_drop_never_creates_replacement_root() {
    use std::{any::Any, rc::Rc};
    struct State;
    impl Drop for State {
        fn drop(&mut self) {
            panic!("actual native destructor failed");
        }
    }
    struct View(Arc<AtomicUsize>);
    impl Widget for View {
        fn create(&self, _: NativeContext) -> Box<dyn Any> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::new(State)
        }
        fn measure(&self, _: &Slots<'_>, width: u16) -> maka_ui_runtime::Result<(u16, u32)> {
            Ok((width, 1))
        }
        fn paint(
            &self,
            _: &mut Slots<'_>,
            _: Placement,
            _: &mut Buffer,
        ) -> maka_ui_runtime::Result<()> {
            Ok(())
        }
    }
    let (runtime, input, packages) = fixture("return frame({...state,revision:state.revision+1});");
    let dir = tempfile::tempdir().unwrap();
    let store = store(&runtime, dir.path());
    let deployment = Uuid::new_v4();
    let original = runtime
        .block_on(store.commit_application(deployment, 0, input.clone(), vec![], packages.clone()))
        .unwrap();
    let creates = Arc::new(AtomicUsize::new(0));
    let count = creates.clone();
    let native = NativeRegistry::with_surface(PresentationTarget::Tui)
        .with_scene_compiler(
            scene::language(),
            move |_| {
                maka_ui_runtime::PreparedScene::new(View(count.clone()), BTreeMap::new(), vec![])
            },
            |prepared| prepared.finish::<View, dyn Widget>(|recipe| Rc::new(recipe)),
        )
        .unwrap();
    let mut tui = runtime
        .block_on(Presenter::start_from_store(
            store.clone(),
            original,
            runtime.handle().clone(),
            native,
            json!({}),
            vec![],
            maka_ui_js::execution(Default::default()),
        ))
        .unwrap();
    let old = tui.surface();
    assert_eq!(creates.load(Ordering::SeqCst), 1);
    let (input, packages) = replacement(input, packages);
    let next = runtime
        .block_on(store.commit_application(deployment, 1, input, vec![], packages))
        .unwrap();
    let update = runtime
        .block_on(tui.load_committed_update(next).unwrap())
        .unwrap();
    tui.adopt_update(update, vec![]).unwrap();
    assert!(tui.surface_closed(&old, Ok(())).is_err());
    for _ in 0..3 {
        assert!(tui.pump_settled().is_err());
        assert!(tui.try_replace_surface(json!({})).is_err());
    }
    assert_eq!(creates.load(Ordering::SeqCst), 1);
    assert_eq!(tui.surface(), old);
    assert!(tui.application.frame(&old).is_ok());
    assert!(!tui.can_shutdown());
    // Keep the failed owner/custody and immutable Root evidence; no fabricated recovery.
    eprintln!("retained failed native Root: {}", dir.keep().display());
    std::mem::forget(tui);
    std::mem::forget(store);
}

#[cfg(unix)]
#[test]
#[ignore = "requires an actual interactive PTY; run alone with --nocapture"]
fn actual_guard_restores_original_tty_before_reentering_for_fresh_root() {
    use std::io::IsTerminal;
    assert!(std::io::stdin().is_terminal() && std::io::stdout().is_terminal());
    fn flags() -> libc::tcflag_t {
        let mut value = std::mem::MaybeUninit::<libc::termios>::uninit();
        assert_eq!(
            unsafe { libc::tcgetattr(libc::STDIN_FILENO, value.as_mut_ptr()) },
            0
        );
        // PENDIN is kernel pending-input state (Darwin sys/termios.h), not
        // an application setting; canonical restoration may set it transiently.
        unsafe { value.assume_init().c_lflag & !libc::PENDIN }
    }
    fn paint_terminal(tui: &mut Presenter, screen: &mut crate::terminal::Screen) {
        let mut published = None;
        crate::terminal::draw(screen, |frame| {
            published = Some(tui.paint(frame.area(), frame.buffer_mut()).unwrap());
        })
        .unwrap();
        tui.present(published.unwrap());
        assert!(
            !tui.scene.is_empty(),
            "actual terminal must publish visible input geometry"
        );
    }
    let initial = flags();
    assert_ne!(initial & libc::ICANON, 0);
    let (runtime, input, packages) = fixture("return frame({...state,revision:state.revision+1});");
    let dir = tempfile::tempdir().unwrap();
    let store = store(&runtime, dir.path());
    let deployment = Uuid::new_v4();
    let original = runtime
        .block_on(store.commit_application(deployment, 0, input.clone(), vec![], packages.clone()))
        .unwrap();
    let mut tui = runtime
        .block_on(Presenter::start_from_store(
            store.clone(),
            original,
            runtime.handle().clone(),
            registry(),
            json!({}),
            vec![],
            maka_ui_js::execution(Default::default()),
        ))
        .unwrap();
    let mut guard = crate::terminal::Guard::new();
    let mut screen = guard.setup("test needs a PTY").unwrap();
    assert_eq!(flags() & libc::ICANON, 0);
    paint_terminal(&mut tui, &mut screen);
    let old = tui.surface();
    let (input, packages) = replacement(input, packages);
    let next = runtime
        .block_on(store.commit_application(deployment, 1, input, vec![], packages))
        .unwrap();
    let update = runtime
        .block_on(tui.load_committed_update(next).unwrap())
        .unwrap();
    tui.adopt_update(update, vec![]).unwrap();
    assert!(tui.try_replace_surface(json!({})).is_err());
    drop(screen);
    let receipt = guard.finish().map_err(|e| e.to_string());
    assert!(receipt.is_ok());
    assert_eq!(flags(), initial);
    tui.surface_closed(&old, receipt).unwrap();
    assert!(tui.replace_surface_settled(json!({})).unwrap());
    let fresh = tui.surface();
    assert_ne!(fresh, old);
    let mut screen = guard.setup("test needs a PTY").unwrap();
    assert_eq!(flags() & libc::ICANON, 0);
    assert!(tui.surface_closed(&old, Ok(())).is_err());
    assert!(!tui.application.is_retiring(&fresh));
    assert_eq!(tui.application.frame(&fresh).unwrap().state["count"], 100);
    paint_terminal(&mut tui, &mut screen);
    drop(screen);
    let receipt = guard.finish().map_err(|e| e.to_string());
    assert!(receipt.is_ok());
    assert_eq!(flags(), initial);
    tui.surface_closed(&fresh, receipt).unwrap();
    tui.shutdown_settled().unwrap();
    drop(tui);
    runtime
        .block_on(Arc::try_unwrap(store).ok().unwrap().close())
        .unwrap();
}

#[test]
fn committed_repair_returns_from_recovery_only_after_its_original_surface_closes() {
    let (runtime, mut input, mut packages) = fixture("throw new Error('primary fault');");
    support::add_recovery(&mut input, &mut packages);
    let dir = tempfile::tempdir().unwrap();
    let store = store(&runtime, dir.path());
    let deployment = Uuid::new_v4();
    let original = runtime
        .block_on(store.commit_application(deployment, 0, input.clone(), vec![], packages.clone()))
        .unwrap();
    let mut tui = runtime
        .block_on(Presenter::start_from_store(
            store.clone(),
            original,
            runtime.handle().clone(),
            registry(),
            json!({}),
            vec![],
            maka_ui_js::execution(Default::default()),
        ))
        .unwrap();
    draw(&mut tui, 30, 12);
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert!(tui.requires_surface_replacement());
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    assert!(tui.replace_surface_settled(json!({})).unwrap());
    let recovery = tui.surface();
    draw(&mut tui, 30, 12);
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(tui.application.frame(&recovery).unwrap().state["count"], 1);
    let index = packages
        .iter()
        .position(|p| p.manifest().id == "dynamic.app")
        .unwrap();
    let broken = packages.remove(index);
    let mut files = broken.files().clone();
    {
        let name = "index.mjs";
        let source = std::str::from_utf8(&files[name]).unwrap().replace(
            "throw new Error('primary fault');",
            "return frame({count:state.count+1,revision:state.revision+1});",
        );
        files.insert(name.into(), source.into_bytes());
    }
    let fixed = Package::new(files).unwrap();
    input.instances.get_mut("app").unwrap().package = fixed.application_ref().into();
    if !packages
        .iter()
        .any(|p| p.application_ref() == fixed.application_ref())
    {
        packages.push(fixed);
    }
    let repaired = runtime
        .block_on(store.commit_application(deployment, 1, input, vec![], packages))
        .unwrap();
    let update = runtime
        .block_on(tui.load_committed_update(repaired.clone()).unwrap())
        .unwrap();
    tui.adopt_update(update, vec![]).unwrap();
    assert_eq!(tui.surface(), recovery);
    assert!(
        !tui.requires_surface_replacement(),
        "unchanged recovery must survive the input commit"
    );
    tui.pump_settled().unwrap();
    assert_eq!(tui.deployment_status().effective, Some(repaired));
    assert!(tui.requires_surface_replacement());
    assert!(tui.try_replace_surface(json!({})).is_err());
    tui.surface_closed(&recovery, Ok(())).unwrap();
    assert!(tui.replace_surface_settled(json!({})).unwrap());
    assert_eq!(
        tui.application.root_kind(&tui.surface()).unwrap(),
        RootKind::Launch
    );
    assert_ne!(tui.surface(), recovery);
    draw(&mut tui, 30, 12);
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(tui.application.frame(&tui.root).unwrap().state["count"], 1);
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    tui.shutdown_settled().unwrap();
    drop(tui);
    runtime
        .block_on(Arc::try_unwrap(store).ok().unwrap().close())
        .unwrap();
}

#[test]
fn failed_committed_launch_replacement_can_mount_ordinary_recovery() {
    for stage in ["activation", "mount"] {
        let (runtime, mut input, mut packages) =
            fixture("return frame({count:state.count+1,revision:state.revision+1});");
        support::add_recovery(&mut input, &mut packages);
        let dir = tempfile::tempdir().unwrap();
        let store = store(&runtime, dir.path());
        let deployment = Uuid::new_v4();
        let original = runtime
            .block_on(store.commit_application(
                deployment,
                0,
                input.clone(),
                vec![],
                packages.clone(),
            ))
            .unwrap();
        let mut tui = runtime
            .block_on(Presenter::start_from_store(
                store.clone(),
                original.clone(),
                runtime.handle().clone(),
                registry(),
                json!({}),
                vec![],
                maka_ui_js::execution(Default::default()),
            ))
            .unwrap();
        draw(&mut tui, 30, 12);
        let old = tui.surface();
        let index = packages
            .iter()
            .position(|p| p.manifest().id == "dynamic.app")
            .unwrap();
        let mut files = packages[index].files().clone();
        let needle = if stage == "activation" {
            "export function activate(configuration) {"
        } else {
            "mount(props){"
        };
        {
            let name = "index.mjs";
            let source = std::str::from_utf8(&files[name]).unwrap();
            assert!(source.contains(needle));
            let source = source.replace(
                needle,
                &format!("{needle}throw new Error('replacement {stage} fault');"),
            );
            files.insert(name.into(), source.into_bytes());
        }
        let broken = Package::new(files).unwrap();
        input.instances.get_mut("app").unwrap().package = broken.application_ref().into();
        packages.push(broken);
        let next = runtime
            .block_on(store.commit_application(deployment, 1, input, vec![], packages))
            .unwrap();
        let update = runtime
            .block_on(tui.load_committed_update(next.clone()).unwrap())
            .unwrap();
        tui.adopt_update(update, vec![]).unwrap();
        assert!(tui.requires_surface_replacement());
        assert!(tui.try_replace_surface(json!({})).is_err());
        tui.surface_closed(&old, Ok(())).unwrap();
        assert!(tui.replace_surface_settled(json!({})).unwrap());
        assert_eq!(
            tui.application.root_kind(&tui.surface()).unwrap(),
            RootKind::Recovery
        );
        assert_eq!(tui.deployment_status().desired, Some(next.clone()));
        assert_eq!(
            tui.deployment_status().effective,
            Some(if stage == "activation" {
                original
            } else {
                next
            })
        );
        assert!(tui.deployment_status().error.is_some());
        draw(&mut tui, 30, 12);
        tui.input_settled(key(KeyCode::Enter)).unwrap();
        assert_eq!(tui.application.frame(&tui.root).unwrap().state["count"], 1);
        tui.surface_closed(&tui.surface(), Ok(())).unwrap();
        tui.shutdown_settled().unwrap();
        drop(tui);
        runtime
            .block_on(Arc::try_unwrap(store).ok().unwrap().close())
            .unwrap();
    }
}

#[cfg(unix)]
#[test]
#[ignore = "requires an actual interactive PTY; run alone with --nocapture"]
fn actual_guard_restores_failed_launch_before_entering_ordinary_recovery() {
    use std::io::IsTerminal;
    assert!(std::io::stdin().is_terminal() && std::io::stdout().is_terminal());
    fn flags() -> libc::tcflag_t {
        let mut value = std::mem::MaybeUninit::<libc::termios>::uninit();
        assert_eq!(
            unsafe { libc::tcgetattr(libc::STDIN_FILENO, value.as_mut_ptr()) },
            0
        );
        unsafe { value.assume_init().c_lflag & !libc::PENDIN }
    }
    fn paint(tui: &mut Presenter, screen: &mut crate::terminal::Screen) {
        let mut published = None;
        crate::terminal::draw(screen, |frame| {
            published = Some(tui.paint(frame.area(), frame.buffer_mut()).unwrap())
        })
        .unwrap();
        tui.present(published.unwrap());
        assert!(!tui.scene.is_empty());
    }
    let initial = flags();
    assert_ne!(initial & libc::ICANON, 0);
    let (runtime, mut input, mut packages) = fixture("throw new Error('primary fault');");
    support::add_recovery(&mut input, &mut packages);
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    let mut guard = crate::terminal::Guard::new();
    let mut screen = guard.setup("actual recovery PTY required").unwrap();
    assert_eq!(flags() & libc::ICANON, 0);
    paint(&mut tui, &mut screen);
    let original = tui.surface();
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert!(tui.requires_surface_replacement());
    assert!(tui.try_replace_surface(json!({})).is_err());
    drop(screen);
    let receipt = guard.finish().map_err(|e| e.to_string());
    assert!(receipt.is_ok());
    assert_eq!(flags(), initial);
    tui.surface_closed(&original, receipt).unwrap();
    assert!(tui.replace_surface_settled(json!({})).unwrap());
    assert_eq!(
        tui.application.root_kind(&tui.surface()).unwrap(),
        RootKind::Recovery
    );
    assert_ne!(tui.surface(), original);
    let mut screen = guard.setup("actual recovery PTY required").unwrap();
    assert_eq!(flags() & libc::ICANON, 0);
    paint(&mut tui, &mut screen);
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    paint(&mut tui, &mut screen);
    assert_eq!(tui.application.frame(&tui.root).unwrap().state["count"], 1);
    drop(screen);
    let receipt = guard.finish().map_err(|e| e.to_string());
    assert!(receipt.is_ok());
    assert_eq!(flags(), initial);
    tui.surface_closed(&tui.surface(), receipt).unwrap();
    tui.shutdown_settled().unwrap();
}
