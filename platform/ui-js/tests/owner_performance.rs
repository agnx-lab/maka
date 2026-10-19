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

//! Retain this exact fixture on both revisions when comparing owner latency.
//! No native terminal, model server, or database work is included here; the
//! companion product PTY fixture verifies the complete user interaction path.

#[path = "../examples/counter/support.rs"]
mod support;

use maka_plugin_core::composition::PresentationTarget;
use maka_ui_runtime::Application;
use serde_json::json;
use std::time::Instant;

#[test]
#[ignore = "debug-profile comparison of owner dispatch and complete guest updates"]
fn large_component_state_owner_responsiveness() {
    let bytes: usize = std::env::var("MAKA_UI_PERF_BYTES")
        .map(|value| value.parse().unwrap())
        .unwrap_or(1024 * 1024);
    let samples: usize = std::env::var("MAKA_UI_PERF_SAMPLES")
        .map(|value| value.parse().unwrap())
        .unwrap_or(32);
    assert!(samples > 0);
    let scheduler = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let (input, packages) = support::fixture(
        "return frame({...state,count:state.count+1,revision:state.revision+1,payload:event.event.payload??state.payload});",
    );
    let mut app = Application::new(
        &input,
        packages,
        scheduler.handle().clone(),
        maka_ui_js::execution(Default::default()),
        support::scene::registry::<()>(PresentationTarget::Tui),
        Default::default(),
    )
    .unwrap();
    scheduler.block_on(app.wait_ready()).unwrap();
    let mount = app.mount_launch(json!({"count":3})).unwrap();
    scheduler.block_on(app.wait_idle()).unwrap();
    let payload = "snapshot-".repeat(bytes.div_ceil(9));
    let payload_bytes = payload.len();
    app.dispatch(&mount, 0, "inc", json!({"payload":payload}))
        .unwrap();
    scheduler.block_on(app.wait_idle()).unwrap();

    let mut owner_us = Vec::with_capacity(samples);
    let mut complete_us = Vec::with_capacity(samples);
    for sample in 0..samples {
        let revision = app.frame(&mount).unwrap().revision;
        let started = Instant::now();
        app.dispatch(&mount, revision, "inc", json!({})).unwrap();
        owner_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        scheduler.block_on(app.wait_idle()).unwrap();
        complete_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        let frame = app.frame(&mount).unwrap();
        assert_eq!(frame.state["count"], sample + 5);
        assert_eq!(frame.revision, sample as u64 + 2);
        assert_eq!(
            frame.state["payload"].as_str().unwrap().len(),
            payload_bytes
        );
        assert!(
            frame.state["payload"]
                .as_str()
                .unwrap()
                .starts_with("snapshot-")
        );
    }
    eprintln!(
        "MAKA_UI_OWNER_PERF {}",
        json!({
            "debug_assertions": cfg!(debug_assertions),
            "payload_bytes": payload_bytes,
            "completed_updates": samples,
            "final_count": app.frame(&mount).unwrap().state["count"],
            "owner_us": owner_us,
            "complete_us": complete_us,
        })
    );
    app.close(&mount, Ok(())).unwrap();
    scheduler.block_on(std::future::poll_fn(|cx| {
        app.set_waker(cx.waker().clone());
        match app.shutdown() {
            Ok(()) => std::task::Poll::Ready(()),
            Err(maka_ui_runtime::Error::Core(maka_plugin_core::Error::CleanupPending)) => {
                std::task::Poll::Pending
            }
            Err(error) => panic!("{error}"),
        }
    }));
}
