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

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

fn scheduler() -> tokio::runtime::Handle {
    static TIMER: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    TIMER
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_time()
                .build()
                .unwrap()
        })
        .handle()
        .clone()
}

use maka_js_runtime::plugin::{Foreground, ForegroundModule, Limits};
use serde_json::{Value, json};

fn realm() -> Foreground {
    Foreground::new(
        Limits {
            synchronous_slice: Duration::from_secs(1),
            ..Limits::default()
        },
        scheduler(),
    )
    .unwrap()
}

#[test]
fn borrowed_foreground_arguments_preserve_json_snapshot_isolation() {
    let mut realm = realm();
    let module = realm
        .load(
            "snapshot.mjs",
            "export const panel = { event(state, input, props) { state.items[0] = input.value; return {state, label:props.label}; } };",
        )
        .unwrap();
    let callbacks = realm
        .capture(&module, &["panel".into()], &["event"])
        .unwrap();
    let state = json!({"items":[7],"exact":9_007_199_254_740_991u64});
    let props = json!({"label":"shared"});
    for value in [8, 9] {
        let input = json!({"value":value});
        assert_eq!(
            realm
                .call_bound(&callbacks, "event", &[&state, &input, &props])
                .unwrap(),
            json!({"state":{"items":[value],"exact":9_007_199_254_740_991u64},"label":"shared"})
        );
        assert_eq!(state["items"], json!([7]));
        assert_eq!(props, json!({"label":"shared"}));
    }
    realm.close(&module).unwrap();
}

fn call(
    realm: &mut Foreground,
    module: &ForegroundModule,
    name: &str,
    args: &[Value],
) -> Result<Value, maka_js_runtime::plugin::Error> {
    realm.call(module, &[name.into()], args)
}

#[test]
fn foreground_runs_dynamic_interaction_without_ambient_io_or_tokio_context() {
    assert!(tokio::runtime::Handle::try_current().is_err());
    let mut realm = realm();
    let module = realm.load("graph.mjs", r#"
        export function capabilities() {
            return [typeof Deno, typeof fetch, typeof setTimeout, typeof TextEncoder, typeof URL, typeof __bootstrap, typeof console, typeof queueMicrotask];
        }
        export function drag(state, event) {
            const point = {x: Math.round(event.x / state.grid) * state.grid, y: Math.round(event.y / state.grid) * state.grid};
            const accepted = !state.occupied.some(node => node.x === point.x && node.y === point.y);
            return {state: {...state, preview: point}, commands: accepted ? [{kind:'move', node:event.node, point}] : [], consumed: accepted};
        }
    "#).unwrap();
    assert_eq!(
        call(&mut realm, &module, "capabilities", &[]).unwrap(),
        json!([
            "undefined",
            "undefined",
            "undefined",
            "function",
            "function",
            "undefined",
            "undefined",
            "undefined"
        ])
    );
    let state = json!({"grid":8,"occupied":[{"x":16,"y":24}]});
    let accepted = call(
        &mut realm,
        &module,
        "drag",
        &[state.clone(), json!({"node":"a","x":18,"y":35})],
    )
    .unwrap();
    assert_eq!(accepted["state"]["preview"], json!({"x":16,"y":32}));
    assert_eq!(accepted["commands"].as_array().unwrap().len(), 1);
    assert_eq!(accepted["consumed"], true);
    let rejected = call(
        &mut realm,
        &module,
        "drag",
        &[state, json!({"node":"a","x":18,"y":25})],
    )
    .unwrap();
    assert_eq!(rejected["commands"], json!([]));
    assert_eq!(rejected["consumed"], false);
    assert!(tokio::runtime::Handle::try_current().is_err());
}

#[test]
fn failed_handler_cannot_return_partial_intent_and_shared_realm_is_quarantined() {
    let mut realm = realm();
    let module = realm
        .load(
            "editor.mjs",
            r#"
        export function good(value) { return {commands:[{kind:'reset',value}],consumed:true}; }
        export function bad(value) {
            const commands = [{kind:'reset',value}];
            globalThis.uncommitted = commands;
            throw new Error('failure after staged reset');
        }
    "#,
        )
        .unwrap();
    let sibling = realm
        .load(
            "sibling.mjs",
            "export function read() { return globalThis.uncommitted; }",
        )
        .unwrap();
    let mut committed = call(
        &mut realm,
        &module,
        "good",
        &[json!("accepted native text")],
    )
    .unwrap();
    let before = committed.clone();
    let outcome = call(&mut realm, &module, "bad", &[json!("late stale reset")]);
    if let Ok(batch) = &outcome {
        committed = batch.clone();
    }
    assert_eq!(committed, before);
    assert!(
        outcome
            .unwrap_err()
            .0
            .contains("failure after staged reset")
    );
    assert!(realm.failure().is_some());
    assert!(call(&mut realm, &sibling, "read", &[]).is_err());
    realm.close(&module).unwrap();
    realm.close(&sibling).unwrap();
    let mut recovery = self::realm();
    let replacement = recovery
        .load("recovery.mjs", "export function read(){return 'recovery';}")
        .unwrap();
    assert_eq!(
        call(&mut recovery, &replacement, "read", &[]).unwrap(),
        json!("recovery")
    );
}

#[test]
fn synchronous_callbacks_reject_promises_and_budget_json_serialization() {
    let mut asynchronous = realm();
    let module = asynchronous
        .load(
            "async.mjs",
            "export async function handler(){return {consumed:true};}",
        )
        .unwrap();
    assert!(
        call(&mut asynchronous, &module, "handler", &[])
            .unwrap_err()
            .0
            .contains("synchronously")
    );
    assert!(asynchronous.failure().is_some());

    let mut serializing = Foreground::new(
        Limits {
            synchronous_slice: Duration::from_millis(100),
            ..Limits::default()
        },
        scheduler(),
    )
    .unwrap();
    let module = serializing
        .load(
            "getter.mjs",
            "export function handler(){return {toJSON(){while(true){}}};}",
        )
        .unwrap();
    let started = Instant::now();
    assert!(call(&mut serializing, &module, "handler", &[]).is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(serializing.failure().unwrap().0.contains("budget exceeded"));
}

#[test]
fn stalled_module_evaluation_and_handler_retire_the_entire_execution_domain() {
    for (source, handler) in [
        ("while(true){}; export function handler(){}", false),
        ("export function handler(){while(true){}}", true),
    ] {
        let mut realm = Foreground::new(
            Limits {
                synchronous_slice: Duration::from_millis(100),
                ..Limits::default()
            },
            scheduler(),
        )
        .unwrap();
        let started = Instant::now();
        let outcome = realm.load("stall.mjs", source);
        if handler {
            assert!(call(&mut realm, &outcome.unwrap(), "handler", &[]).is_err());
        } else {
            assert!(outcome.is_err());
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(realm.failure().unwrap().0.contains("budget exceeded"));
        assert!(
            realm
                .load("later.mjs", "export function read(){return true;}")
                .is_err()
        );
    }
}

#[test]
fn module_handles_reject_cross_realm_and_post_close_dispatch_without_harming_siblings() {
    let mut first = realm();
    let mut second = realm();
    let module = first
        .load("first.mjs", "export function read(){return 'first';}")
        .unwrap();
    let sibling = first
        .load("sibling.mjs", "export function read(){return 'sibling';}")
        .unwrap();
    let other = second
        .load("second.mjs", "export function read(){return 'second';}")
        .unwrap();
    assert!(call(&mut second, &module, "read", &[]).is_err());
    assert!(second.failure().is_none());
    first.close(&module).unwrap();
    assert!(call(&mut first, &module, "read", &[]).is_err());
    assert_eq!(
        call(&mut first, &sibling, "read", &[]).unwrap(),
        json!("sibling")
    );
    assert_eq!(
        call(&mut second, &other, "read", &[]).unwrap(),
        json!("second")
    );
    // Opaque handles may outlive the realm; only the realm owns V8 Globals.
    drop(first);
    assert_eq!(
        call(&mut second, &other, "read", &[]).unwrap(),
        json!("second")
    );
    drop(module);
    drop(sibling);
}

#[test]
fn foreground_rejects_non_json_results() {
    for body in ["const x={};x.self=x;return x;", "return 1n;"] {
        let mut realm = realm();
        let module = realm
            .load("wire.mjs", &format!("export function read(){{{body}}}"))
            .unwrap();
        assert!(call(&mut realm, &module, "read", &[]).is_err());
        assert!(realm.failure().is_some());
    }
}

#[test]
fn ready_jobs_fail_or_stall_before_the_current_callback_batch_can_commit() {
    for job in ["throw new Error('deferred failure');", "while(true){}"] {
        let mut realm = Foreground::new(
            Limits {
                synchronous_slice: Duration::from_millis(100),
                ..Limits::default()
            },
            scheduler(),
        )
        .unwrap();
        let source = format!(
            "export function handler(){{Promise.resolve().then(()=>{{{job}}});return {{commands:[{{kind:'reset',value:'stale'}}],consumed:true}};}}"
        );
        let module = realm.load("jobs.mjs", &source).unwrap();
        let started = Instant::now();
        assert!(call(&mut realm, &module, "handler", &[]).is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(realm.failure().is_some());
    }
}

#[test]
fn captured_callbacks_keep_receiver_and_function_identity_until_module_close() {
    let mut realm = realm();
    let module=realm.load("callbacks.mjs",r#"
        export let panel = { count:4, read(){return this.count;} };
        export function mutate(){ panel.read=()=>999; panel={count:100,read(){return this.count;}}; return null; }
    "#).unwrap();
    let bound = realm
        .capture(&module, &["panel".into()], &["read"])
        .unwrap();
    assert_eq!(realm.call_bound(&bound, "read", &[]).unwrap(), 4);
    call(&mut realm, &module, "mutate", &[]).unwrap();
    assert_eq!(realm.call_bound(&bound, "read", &[]).unwrap(), 4);
    assert_eq!(
        realm
            .call(&module, &["panel".into(), "read".into()], &[])
            .unwrap(),
        100
    );
    let mut other = Foreground::new(Default::default(), scheduler()).unwrap();
    assert!(other.call_bound(&bound, "read", &[]).is_err());
    realm.close(&module).unwrap();
    assert!(realm.call_bound(&bound, "read", &[]).is_err());
    drop(realm);
    assert!(other.call_bound(&bound, "read", &[]).is_err());
}

#[test]
fn callback_capture_is_atomic_and_getters_obey_the_foreground_budget() {
    let mut missing = realm();
    let module = missing
        .load("missing.mjs", "export const panel={mount(){return null;}};")
        .unwrap();
    assert!(
        missing
            .capture(&module, &["panel".into()], &["mount", "event"])
            .is_err()
    );
    assert!(missing.failure().is_some());
    for source in [
        "export const panel={get mount(){while(true){}},event(){}};",
        "export const panel={mount(){},get event(){while(true){}}};",
    ] {
        let mut realm = Foreground::new(
            Limits {
                synchronous_slice: Duration::from_millis(50),
                ..Default::default()
            },
            scheduler(),
        )
        .unwrap();
        let module = realm.load("getter.mjs", source).unwrap();
        assert!(
            realm
                .capture(&module, &["panel".into()], &["mount", "event"])
                .is_err()
        );
        assert!(realm.failure().is_some());
        realm.close(&module).unwrap();
    }
}
