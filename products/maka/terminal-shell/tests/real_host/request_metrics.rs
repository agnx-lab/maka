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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn request_speed_updates_while_the_same_turn_continues_after_a_tool() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mut fixture = Fixture::files("write").await;
        let client = fixture.client().await;
        let (mut view, bindings) = fixture.view(fixture.client().await, Some(fixture.session.clone()));
        wait_editable_composer(&mut view).await;
        let started = support::mutate(&client, "send", json!({
            "sessionId":fixture.session,"message":"Write a document, then continue", "model":fixture.selection,
        })).await;
        assert_eq!(started["kind"], "started");
        let mut first = fixture.next_request().await;
        first.delta("REQUEST_METRICS_FIRST_STREAM").await;
        wait_view(&mut view, |screen| screen.contains("REQUEST_METRICS_FIRST_STREAM")).await;
        first.tool("WorkspaceDocument", json!({"path":"metrics.txt","content":"written"})).await;
        let mut continuation = fixture.next_request().await;
        continuation.delta("REQUEST_METRICS_CONTINUATION_STREAM").await;
        let observed = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        assert_eq!(observed["run"]["invocationId"], started["invocationId"]);
        let speed = observed["metrics"]["outputTokensPerSecond"].as_f64()
            .expect("the completed first request must have its provider token rate");
        let expected = format!("{speed:.1} tok/s");
        let live = wait_view(&mut view, |screen| {
            screen.contains("REQUEST_METRICS_CONTINUATION_STREAM") && screen.contains(&expected)
        }).await;
        assert!(!live.contains("(last request)") && !live.contains("(last known)"));
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 2);
        evidence("request-speed-before-continuation-completes", &live);
        continuation.finish().await;
        fixture.idle(&client, &fixture.session).await;
        let completed = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        let speed = completed["metrics"]["outputTokensPerSecond"].as_f64().unwrap();
        wait_view(&mut view, |screen| screen.contains(&format!("{speed:.1} tok/s"))).await;
        drop(view);
        bindings.close().await.unwrap();
        client.close().await.unwrap();
        fixture.finish().await;
    }).await.expect("per-request speed did not update before the turn ended");
}
