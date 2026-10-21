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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn progress_cancellation_and_scope_drain_preserve_saved_business_receipts() {
    tokio::time::timeout(Duration::from_secs(30), async {
        for (finish, cleanup_failure) in [("next", false), ("mutation", false), ("client", false), ("next", true)] {
            let fixture = Fixture::new().await;
            fixture.counter.blocked.store(true, Ordering::SeqCst);
            let client = fixture.client().await;
            let id = Uuid::new_v4();
            let mutation = client.settings_mutation(id, "authentication", "loginCredential", json!({"progress":true,"cleanupFailure":cleanup_failure}), Attachments::new()).await.unwrap();
            let prepared = mutation.prepare().await.unwrap();
            match mutation.next().await.unwrap().unwrap() {
                MutationEvent::Progress(value) => assert_eq!(value, json!({"url":"https://auth.openai.com/codex/device","userCode":"ABCD-EFGH"})),
                other => panic!("the original Host must publish its progress first: {other:?}"),
            }
            assert_eq!(client.query("agent", "read", Value::Null).await.unwrap(), json!({"sent":1}));
            mutation.request_cancel();
            fixture.counter.release.notify_one();
            match finish {
                "next" => {
                    match mutation.next().await.unwrap().unwrap() {
                        MutationEvent::Finished { reply, cleanup } => {
                            assert_eq!(reply.reference, prepared.reference.clone().unwrap());
                            assert!(matches!(reply.receipt, Receipt::Recorded { outcome: OperationOutcome::Returned { ref value }, .. } if value == &json!({"kind":"saved"})));
                            assert_eq!(cleanup, if cleanup_failure { Err("tool cleanup unconfirmed: counter cleanup failure".into()) } else { Ok(()) });
                        }
                        other => panic!("cancellation must retain the original terminal reply: {other:?}"),
                    }
                    assert!(mutation.next().await.unwrap().is_none());
                }
                "mutation" => mutation.close().await.unwrap(),
                "client" => client.close().await.unwrap(),
                _ => unreachable!(),
            }
            let original = client.operation_for_settings(id).await.unwrap();
            assert_eq!(original.reference, prepared.reference);
            assert!(matches!(&original.receipt, Some(Receipt::Recorded { outcome: OperationOutcome::Returned { value }, .. }) if value == &json!({"kind":"saved"})), "finish={finish}, cleanup_failure={cleanup_failure}, original={original:?}");
            assert_eq!(fixture.counter.effects.load(Ordering::SeqCst), 1);
            if cleanup_failure {
                assert_eq!(client.pending_requests().await, 1, "a business receipt does not release unconfirmed physical custody");
                let _ = client.close().await;
                // This injected resource group explicitly reports failed cleanup.
                // Retiring this local fixture does not assert successful cleanup.
                let _ = fixture.host.shutdown().await;
                let _ = fixture.serving.await;
            } else {
                assert_eq!(client.pending_requests().await, 0);
                client.close().await.unwrap();
                fixture.finish().await;
            }
        }
    }).await.unwrap();
}
