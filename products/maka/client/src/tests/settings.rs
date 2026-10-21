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
use maka_capabilities::call::ReadResource;
use maka_plugin_core::contract::ResourceType;
use std::ops::Range;

struct Secret(&'static str);
impl ReadResource for Secret {
    fn encoding(&self) -> ResourceEncoding {
        ResourceEncoding::Utf8
    }
    fn resource_type(&self) -> ResourceType {
        text_resource_type()
    }
    fn bytes(&self) -> usize {
        self.0.len()
    }
    fn metadata(&self) -> Value {
        json!({})
    }
    fn read(&self, range: Range<usize>) -> Result<Vec<u8>> {
        self.0
            .get(range)
            .map(|text| text.as_bytes().to_vec())
            .ok_or_else(|| "invalid secret range".into())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settings_admission_and_original_secret_receipt_survive_independent_clients_and_restart() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let fixture = Fixture::new().await;
        let first = fixture.client().await;
        let second = fixture.client().await;
        let secret = || -> Attachments {
            Attachments::from([(
                "secret".into(),
                Arc::new(Secret("k€y\\\"raw")) as Arc<dyn ReadResource>,
            )])
        };
        for (endpoint, method, resources, expected) in [
            ("settings-setup", "discoverModels", Attachments::new(), "unsupported settings mutation method"),
            ("settings", "create", secret(), "settings mutation does not accept resources"),
            ("settings", "saveCredential", Attachments::new(), "settings credential requires one UTF-8 secret resource"),
        ] {
            assert_eq!(first.settings_mutation(Uuid::new_v4(), endpoint, method, Value::Null, resources).await.err().unwrap(), expected);
        }
        for (endpoint, method, session, correlation) in [
            ("agent", "selectBackend", Some("session-one".into()), json!({"kind":"agent-control"})),
            ("settings", "create", Some("session-one".into()), json!({"kind":"settings"})),
            ("settings", "create", None, json!({"kind":"settings","extra":true})),
            ("settings", "create", None, json!("{\"kind\":\"settings\"}")),
        ] {
            let other = first.mutation(Uuid::new_v4(), endpoint, method, Value::Null, Attachments::new(), session, correlation).await.unwrap();
            assert_eq!(first.operation_for_settings(other.id()).await.unwrap_err(), "original client operation belongs to another settings owner");
            other.close().await.unwrap();
        }
        assert!(first.pending_settings().await.unwrap().is_empty());
        let (left, right) = tokio::join!(
            first.settings_mutation(Uuid::new_v4(), "settings", "create", json!({"name":"private settings input"}), Attachments::new()),
            second.settings_mutation(Uuid::new_v4(), "setup", "discoverModels", json!({"connectionId":"one"}), Attachments::new()),
        );
        let admitted = match (left, right) {
            (Ok(admitted), Err(error)) | (Err(error), Ok(admitted)) => {
                assert_eq!(error, "settings operation already exists or has an unresolved original");
                admitted
            }
            _ => panic!("exactly one independent settings client must be admitted"),
        };
        let original_id = admitted.id();
        assert_eq!(first.pending_requests().await + second.pending_requests().await, 1);
        for client in [&first, &second] {
            let pending = client.pending_settings().await.unwrap();
            assert_eq!(pending.iter().map(|operation| operation.id).collect::<Vec<_>>(), [original_id]);
            assert_eq!(pending[0].correlation, json!({"kind":"settings"}));
            assert_eq!(client.recover(original_id).await.unwrap_err(), "client attempt has no confirmed preparation; it cannot be replayed");
        }
        let restarted = fixture.client().await;
        assert!(restarted.abandon_unprepared(original_id).await.unwrap().abandoned);
        assert_eq!(restarted.operation_for_settings(original_id).await.unwrap().id, original_id);
        assert!(admitted.prepare().await.is_err());
        assert_eq!(first.query("agent", "read", Value::Null).await.unwrap(), json!({"sent":0}));
        assert!(second.pending_settings().await.unwrap().is_empty());

        fixture.counter.blocked.store(true, Ordering::SeqCst);
        let id = Uuid::new_v4();
        let saved = first.settings_mutation(id, "settings", "saveCredential", json!({"expected":{"connectionId":"one"},"expectedCredential":null}), secret()).await.unwrap();
        let prepared = saved.prepare().await.unwrap();
        let fenced = restarted.abandon_unprepared(id).await.unwrap();
        assert!(!fenced.abandoned);
        assert_eq!(fenced.reference, prepared.reference);
        let mut confirmation = Box::pin(saved.confirm());
        tokio::select! {
            result = &mut confirmation => panic!("gated Settings returned early: {result:?}"),
            () = fixture.counter.started.notified() => {},
        }
        drop(confirmation);
        saved.disconnect().await;
        drop(saved);
        assert_eq!(second.settings_mutation(Uuid::new_v4(), "settings", "create", Value::Null, Attachments::new()).await.err().unwrap(), "settings operation already exists or has an unresolved original");
        fixture.counter.release.notify_one();
        loop {
            let recovered = restarted.recover(id).await.unwrap();
            if let Receipt::Recorded { outcome: OperationOutcome::Returned { value }, cleanup: OperationCleanup::Confirmed } = recovered.receipt {
                assert_eq!(value, json!({"sent":1}));
                assert_eq!(recovered.reference, prepared.reference.clone().unwrap());
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(fixture.counter.secrets.lock().unwrap().as_slice(), &[vec![107, 226, 130, 172, 121, 92, 34, 114, 97, 119]]);
        assert_eq!(first.pending_requests().await, 1, "receipt recovery does not acknowledge physical cleanup");
        assert_eq!(second.operation_for_settings(id).await.unwrap().id, id);
        assert!(second.pending_settings().await.unwrap().is_empty());
        let contexts = sqlx::query_scalar::<_, String>("SELECT context FROM operations")
            .fetch_all(&restarted.0.journal.0).await.unwrap();
        for context in contexts {
            assert!(!context.contains("k€y"));
            assert!(!context.contains("private settings input"));
        }
        assert_eq!(second.settings_mutation(id, "setup", "discoverModels", Value::Null, Attachments::new()).await.err().unwrap(), "settings operation already exists or has an unresolved original");
        fixture.counter.blocked.store(false, Ordering::SeqCst);
        let next = second.settings_mutation(Uuid::new_v4(), "setup", "discoverModels", Value::Null, Attachments::new()).await.unwrap();
        next.prepare().await.unwrap();
        assert!(matches!(next.confirm().await.unwrap().receipt, Receipt::Recorded { outcome: OperationOutcome::Returned { ref value }, .. } if value == &json!({"sent":2})));
        assert_eq!(restarted.query("agent", "read", Value::Null).await.unwrap(), json!({"sent":2}));
        first.close().await.unwrap();
        second.close().await.unwrap();
        restarted.close().await.unwrap();
        fixture.finish().await;
    }).await.unwrap();
}
