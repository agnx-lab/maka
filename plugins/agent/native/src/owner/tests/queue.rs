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

#[tokio::test]
async fn queued_followup_uses_original_worker_and_a_new_invocation() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let fixture = Fixture::new(&base, None).await;
        let session = fixture.create("Queued followup").await;
        let first = fixture.send(&session).await.unwrap();
        assert_eq!(first["kind"], "started");
        let (mut first_socket, _) = listener.accept().await.unwrap();
        request(&mut first_socket).await;
        let next = fixture.mutation(2, "send", json!({"sessionId":session,"message":"original followup","model":fixture.selection().await}), Uuid::new_v4()).await.unwrap();
        assert_eq!(next["kind"], "queued");
        assert_eq!(fixture.query("work",json!({"sessionId":session,"target":{"kind":"message","messageId":next["messageId"]}})).await.unwrap(),json!({"kind":"pending"}));
        assert_eq!(fixture.query("work",json!({"sessionId":session,"target":{"kind":"message","messageId":"absent_message"}})).await.unwrap(),json!({"kind":"missing"}));
        let other = fixture.create("Other message owner").await;
        assert_eq!(fixture.query("work",json!({"sessionId":other,"target":{"kind":"message","messageId":next["messageId"]}})).await.unwrap(),json!({"kind":"missing"}));
        assert!(fixture.query("work",json!({"sessionId":"foreign_session","target":{"kind":"message","messageId":next["messageId"]}})).await.is_err());
        assert_eq!(fixture.owner.runs.lock().await.controls.len(), 1);
        let pending = fixture.store.pending_messages(&session).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].invocation.invocation_id, first["invocationId"]);
        response(&mut first_socket).await;
        let (mut second_socket, _) = listener.accept().await.unwrap();
        let second = request(&mut second_socket).await;
        assert_eq!(second["model"], "fixture-model");
        assert!(second["messages"].as_array().unwrap().iter().any(|message| message["content"] == "original followup"));
        let current = fixture.query("observe", json!({"sessionId":session})).await.unwrap();
        assert_ne!(current["run"]["invocationId"], first["invocationId"]);
        assert!(fixture.store.pending_messages(&session).await.unwrap().is_empty());
        // Put a real send at the gate before the completed successor worker.
        // Its Running handle drops while we hold the gate, cancelling that
        // handle's token; this is not a user stop or a SQL interrupt fence.
        let selection = fixture.selection().await;
        let client = fixture.app.client();
        let prepared = client.prepare_mutation(fixture.binding(2).await, "send".into(), json!({"sessionId":session,"message":"tail followup","model":selection}), scope(&fixture.issuer, true), Uuid::new_v4(), Attachments::new().into()).await.unwrap();
        let reference = prepared.reference().clone();
        drop(prepared);
        let gate = fixture.owner.runs.lock().await;
        let completed = gate.controls[&session].cancellation.clone();
        let parent = scope(&fixture.issuer, true);
        let mut tail = Box::pin(fixture.owner.send(Send { session_id: session.clone(), message: "tail followup".into(), model: selection, thinking_level: None, attachments: vec![] }, &reference, &parent));
        assert!(futures_util::poll!(tail.as_mut()).is_pending());
        response(&mut second_socket).await;
        completed.cancelled().await;
        drop(gate);
        assert!(matches!(tail.await.unwrap(), SendResult::Queued { .. }));
        let (mut third_socket, _) = listener.accept().await.unwrap();
        let third = request(&mut third_socket).await;
        assert!(third["messages"].as_array().unwrap().iter().any(|message| message["content"] == "tail followup"));
        response(&mut third_socket).await;
        fixture.drained().await;
        assert!(fixture.owner.runs.lock().await.controls.is_empty());
        fixture.app.shutdown().await.unwrap();
        fixture.config.shutdown().await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn stopping_original_run_retracts_queue_and_fences_tail_admission() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let fixture = Fixture::new(&base, None).await;
        let session = fixture.create("Stop queue").await;
        let first = fixture.send(&session).await.unwrap();
        let (mut socket, _) = listener.accept().await.unwrap();
        request(&mut socket).await;
        let queued = fixture.send(&session).await.unwrap();
        assert_eq!(queued["kind"], "queued");
        let selection = fixture.selection().await;
        let client = fixture.app.client();
        let stop = client
            .prepare_mutation(
                fixture.binding(2).await,
                "stop".into(),
                json!({"sessionId":session,"invocationId":first["invocationId"]}),
                scope(&fixture.issuer, true),
                Uuid::new_v4(),
                Attachments::new().into(),
            )
            .await
            .unwrap();
        let tail = client
            .prepare_mutation(
                fixture.binding(2).await,
                "send".into(),
                json!({"sessionId":session,"message":"after stop","model":selection}),
                scope(&fixture.issuer, true),
                Uuid::new_v4(),
                Attachments::new().into(),
            )
            .await
            .unwrap();
        let stop_reference = stop.reference().clone();
        let tail_reference = tail.reference().clone();
        drop((stop, tail));
        assert!(
            fixture
                .owner
                .stop_run(
                    &session,
                    first["invocationId"].as_str().unwrap(),
                    &stop_reference
                )
                .await
                .unwrap()
        );
        // This current-thread executor has not polled the cancelled worker
        // since stop returned. Retain its gate and queue the tail send before
        // yielding, so only the durable interrupt fence can reject admission.
        let gate = fixture.owner.runs.lock().await;
        assert_eq!(
            gate.controls[&session].invocation.invocation_id,
            first["invocationId"]
        );
        let parent = scope(&fixture.issuer, true);
        let mut tail = Box::pin(fixture.owner.send(
            Send {
                session_id: session.clone(),
                message: "after stop".into(),
                model: selection,
                thinking_level: None,
                attachments: vec![],
            },
            &tail_reference,
            &parent,
        ));
        assert!(futures_util::poll!(tail.as_mut()).is_pending());
        drop(gate);
        assert_eq!(tail.await.err().unwrap(), "agent_admission_failed");
        fixture.drained().await;
        assert!(
            fixture
                .store
                .pending_messages(&session)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            fixture
                .store
                .message_cancelled(&session, queued["messageId"].as_str().unwrap())
                .await
                .unwrap()
        );
        assert_eq!(
            fixture
                .query(
                    "work",
                    json!({"sessionId":session,"target":{"kind":"message","messageId":queued["messageId"]}})
                )
                .await
                .unwrap(),
            json!({"kind":"cancelled"})
        );
        fixture.app.shutdown().await.unwrap();
        fixture.config.shutdown().await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn activation_recovers_all_pending_sessions_without_new_sends() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let fixture = Fixture::new(&base, None).await;
        use maka_agent_facts::execution::ThinkingLevel::Off;
use maka_agent_facts::execution::ThinkingLevel::Low;
use maka_agent_facts::execution::ThinkingLevel::High;
        selection::thinking(&fixture, vec![Off, Low, High], High).await;
        let deployment = fixture.app.client().snapshot().effective.unwrap().deployment;
        let mut sessions = Vec::new();
        let raw = rusqlite::Connection::open(fixture.root.join(maka_event_log::root::ROOT_DATABASE)).unwrap();
        raw.execute_batch("CREATE TRIGGER hold_opening BEFORE INSERT ON event_log WHEN NEW.kind='invocation_opened' BEGIN SELECT RAISE(ABORT, 'fixture opening failure'); END;").unwrap();
        for (index, level) in [Some(Off), Some(Low), None].into_iter().enumerate() {
            let session = fixture.create(&format!("Recovery {index}")).await;
            assert!(fixture.mutation(2, "send", json!({"sessionId":session,"message":"restored thinking","model":fixture.selection().await,"thinkingLevel":level}), Uuid::new_v4()).await.is_err());
            assert_eq!(fixture.store.pending_messages(&session).await.unwrap().len(), 1);
            sessions.push(session);
        }
        fixture.app.shutdown().await.unwrap();
        raw.execute_batch("DROP TRIGGER hold_opening").unwrap();
        drop(raw);
        let mut native = NativeRegistry::default();
        crate::owner::tests::provide_all(&mut native).unwrap();
        maka_settings_native::provide(&mut native).unwrap();
        let app = Application::start_from_store(fixture.store.clone(), deployment, native, fixture.issuer.clone(), Arc::new(Grant(fixture.issuer.clone(), None)), Some(maka_host_js::factory(maka_js_runtime::plugin::Limits::default()))).await.unwrap();
        app.ready().await.unwrap();
        let mut efforts = Vec::new();
        for _ in 0..3 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let body = request(&mut socket).await;
            assert_eq!(body["model"], "fixture-model");
            efforts.push(body["reasoning_effort"].as_str().unwrap().to_owned());
            response(&mut socket).await;
        }
        efforts.sort();
        assert_eq!(efforts, ["high", "low", "none"]);
        for session in sessions {
            while !fixture.store.pending_messages(&session).await.unwrap().is_empty() { tokio::task::yield_now().await; }
        }
        app.shutdown().await.unwrap();
        fixture.config.shutdown().await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn queue_controls_are_bounded_revisioned_and_observed_without_transcript_progress() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let fixture = Fixture::new(&base, None).await;
        let session = fixture.create("Queue controls").await;
        let started = fixture.send(&session).await.unwrap();
        let (mut socket, _) = listener.accept().await.unwrap();
        request(&mut socket).await;
        let selection = fixture.selection().await;
        let mut ids = Vec::new();
        for _ in 0..64 {
            let result = fixture.mutation(2, "send", json!({"sessionId":session,"message":"🦀\"\n".repeat(80),"model":selection}), Uuid::new_v4()).await.unwrap();
            ids.push(result["messageId"].as_str().unwrap().to_owned());
        }
        let queue = fixture.query("queue", json!({"sessionId":session})).await.unwrap();
        assert_eq!(queue["entries"].as_array().unwrap().len(), 64);
        assert!(serde_json::to_vec(&queue).unwrap().len() < 48 * 1024);
        assert!(!queue.to_string().contains("private-fixture-key"));
        assert!(!queue.to_string().contains("execution"));
        let through = fixture.query("observe", json!({"sessionId":session})).await.unwrap()["through"].as_u64().unwrap();
        let mut watch = fixture.app.client().subscribe(fixture.binding(2).await, "watch".into(), json!({"sessionId":session,"after":through}), scope(&fixture.issuer, true), Attachments::new().into()).await.unwrap();
        let first = watch.recv().await.unwrap();
        assert_eq!(first["kind"], "queue");
        assert_eq!(first["queue"], queue);
        let interactions = watch.recv().await.unwrap();
        assert_eq!(interactions["kind"], "interactions");
        assert_eq!(interactions["pending"]["entries"], json!([]));
        let events = watch.recv().await.unwrap();
        assert_eq!(events["kind"], "events");
        assert_eq!(events["events"], json!([]));
        let message = "x".repeat(96 * 1024);
        let update = json!({"sessionId":session,"expectedRevision":queue["revision"],"edit":{"kind":"update","messageId":ids[0],"message":message}});
        let operation = Uuid::new_v4();
        let changed = fixture.mutation(2, "editQueue", update.clone(), operation).await.unwrap();
        assert_eq!(changed["kind"], "committed");
        assert_eq!(fixture.mutation(2, "editQueue", update.clone(), operation).await.unwrap(), changed);
        assert_eq!(fixture.mutation(2, "editQueue", update, Uuid::new_v4()).await.unwrap()["kind"], "revision_conflict");
        let notice = loop {
            let update = watch.recv().await.unwrap();
            if update["kind"] == "queue" { break update; }
            if update["kind"] == "interactions" { continue; }
            assert_eq!(update["through"], through);
            assert_eq!(update["events"], json!([]));
        };
        assert_eq!(notice["queue"]["revision"], changed["revision"]);
        assert_eq!(notice["queue"]["entries"][0]["preview"], "x".repeat(80));
        assert_eq!(fixture.query("observe", json!({"sessionId":session})).await.unwrap()["through"], through);
        let read = fixture.query("queueMessage", json!({"sessionId":session,"messageId":ids[0],"expectedRevision":changed["revision"]})).await.unwrap();
        assert_eq!(read, json!({"kind":"ready","revision":changed["revision"],"message":message,"hasAttachments":false}));
        assert_eq!(fixture.query("queueMessage", json!({"sessionId":session,"messageId":ids[0],"expectedRevision":queue["revision"]})).await.unwrap()["kind"], "revision_conflict");
        ids.reverse();
        let reordered = fixture.mutation(2, "editQueue", json!({"sessionId":session,"expectedRevision":changed["revision"],"edit":{"kind":"reorder","messageIds":ids}}), Uuid::new_v4()).await.unwrap();
        assert_eq!(reordered["kind"], "committed");
        assert_eq!(fixture.query("queue", json!({"sessionId":session})).await.unwrap()["entries"][0]["messageId"], ids[0]);
        let retracted = fixture.mutation(2, "editQueue", json!({"sessionId":session,"expectedRevision":reordered["revision"],"edit":{"kind":"retract","messageId":ids[0]}}), Uuid::new_v4()).await.unwrap();
        assert_eq!(retracted["kind"], "committed");
        assert!(fixture.store.message_cancelled(&session, &ids[0]).await.unwrap());
        assert_eq!(fixture.query("queueMessage", json!({"sessionId":session,"messageId":ids[0],"expectedRevision":retracted["revision"]})).await.unwrap()["kind"], "missing");
        assert!(fixture.query("queue", json!({"sessionId":"foreign_session"})).await.is_err());
        drop(watch);
        fixture.mutation(2, "stop", json!({"sessionId":session,"invocationId":started["invocationId"]}), Uuid::new_v4()).await.unwrap();
        fixture.drained().await;
        fixture.app.shutdown().await.unwrap();
        fixture.config.shutdown().await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn stopping_near_capacity_queue_preserves_full_receipt_and_cancels_every_message() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fixture = Fixture::new(&format!("http://{}/v1", listener.local_addr().unwrap()), None).await;
        let session = fixture.create("Stop accepted queue").await;
        let started = fixture.send(&session).await.unwrap();
        let (mut held, _) = listener.accept().await.unwrap();
        request(&mut held).await;
        let selection = fixture.selection().await;
        let mut ids = Vec::new();
        for index in 0..7 {
            let result = fixture.mutation(2, "send", json!({"sessionId":session,"message":format!("q{index}"),"model":selection}), Uuid::new_v4()).await.unwrap();
            ids.push(result["messageId"].as_str().unwrap().to_owned());
        }
        // Each public message/edit text stays <=600 KiB. Prepared and editable text
        // are both retained, so seven edits approach the admitted queue budget.
        for id in &ids {
            let queue = fixture.store.message_queue(&session).await.unwrap();
            let bytes: usize = queue.entries.iter().map(|entry| serde_json::to_vec(entry).unwrap().len()).sum();
            let length = (2 + (8 * 1024 * 1024 - 4 - bytes) / 2).min(600 * 1024);
            let message = "x".repeat(length);
            let changed = fixture.mutation(2, "editQueue", json!({"sessionId":session,"expectedRevision":queue.revision,"edit":{"kind":"update","messageId":id,"message":message}}), Uuid::new_v4()).await.unwrap();
            assert_eq!(changed["kind"], "committed");
            let read = fixture.query("queueMessage", json!({"sessionId":session,"messageId":id,"expectedRevision":changed["revision"]})).await.unwrap();
            assert_eq!(read["message"], message);
        }
        let queue = fixture.store.message_queue(&session).await.unwrap();
        let bytes: usize = queue.entries.iter().map(|entry| serde_json::to_vec(entry).unwrap().len()).sum();
        let receipt = maka_event_log::message_queue::QueueReceipt { revision:queue.revision+1,retracted:queue.entries.clone() };
        assert!(bytes <= 8 * 1024 * 1024);
        assert!(serde_json::to_vec(&receipt).unwrap().len() > 8 * 1024 * 1024);
        let cancellation = fixture.owner.runs.lock().await.controls[&session].cancellation.clone();
        let input = json!({"sessionId":session,"invocationId":started["invocationId"]});
        let operation = Uuid::new_v4();
        let stopped = fixture.mutation(2, "stop", input.clone(), operation).await;
        let replayed = fixture.mutation(2, "stop", input, operation).await;
        let cancelled_by_stop = cancellation.is_cancelled();
        if stopped.is_err() {
            // Keep baseline failure cleanup on the public path. The final
            // assertion still requires the original full queue to Stop.
            fixture.mutation(2, "editQueue", json!({"sessionId":session,"expectedRevision":queue.revision,"edit":{"kind":"update","messageId":ids[6],"message":"cleanup"}}), Uuid::new_v4()).await.unwrap();
            fixture.mutation(2, "stop", json!({"sessionId":session,"invocationId":started["invocationId"]}), Uuid::new_v4()).await.unwrap();
        }
        fixture.drained().await;
        let remaining = fixture.store.message_queue(&session).await.unwrap();
        let mut cancellations = Vec::new();
        for id in &ids {
            cancellations.push(fixture.store.message_cancelled(&session, id).await.unwrap());
            assert!(fixture.store.root_message(&session, id).await.unwrap().is_none());
        }
        fixture.app.shutdown().await.unwrap();
        fixture.config.shutdown().await.unwrap();
        assert_eq!(stopped, Ok(json!({"requested":true})));
        assert_eq!(replayed, stopped);
        assert!(cancelled_by_stop);
        assert_eq!(remaining.revision, receipt.revision);
        assert!(remaining.entries.is_empty());
        assert_eq!(cancellations, vec![true; 7]);
    }).await.unwrap();
}
