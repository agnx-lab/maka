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
use crate::{EventLog, context};
use maka_agent_facts::{
    context::{CheckpointMode, CompactOutcome, ContextCheckpoint, ModelPurpose, TextSummary},
    event::{EventWrite, Fact, Invocation, InvocationInput, InvocationOutcome, RuntimeEvent},
    model::ModelStep,
};
use serde_json::json;
use sqlx::Connection;

fn event(id: &str, fact: Fact) -> EventWrite {
    EventWrite::plain(RuntimeEvent::new(
        Invocation {
            session_id: "session".into(),
            turn_id: id.into(),
            run_id: id.into(),
            invocation_id: id.into(),
        },
        fact,
    ))
    .unwrap()
}

async fn closed(log: &EventLog, id: &str) -> String {
    let opening = event(
        id,
        Fact::InvocationOpened {
            configuration: None,
            input: InvocationInput::Message {
                source_messages: vec![],
                content: "literal original question".into(),
                request_fingerprint: None,
            },
        },
    );
    let original = opening.event().id.clone();
    log.append_batch(&[
        opening,
        event(
            id,
            Fact::InvocationEnded {
                outcome: InvocationOutcome::Completed,
            },
        ),
    ])
    .await
    .unwrap();
    original
}

async fn prepared_checkpoint(log: &EventLog, id: &str) -> Vec<EventWrite> {
    log.append(&event(
        id,
        Fact::InvocationOpened {
            configuration: None,
            input: InvocationInput::ContextCompact {
                request_fingerprint: id.into(),
            },
        },
    ))
    .await
    .unwrap();
    let source = log
        .prepare_context_compaction("session", Some(id), &CheckpointMode::Standalone)
        .await
        .unwrap();
    let previous = source
        .baseline
        .as_ref()
        .map(|baseline| baseline.event_id.clone());
    let output: ModelStep = serde_json::from_value(json!({
        "parts":[{"kind":"text","text_kind":"text","text":"literal retained summary"}],
        "finish_reason":"stop","usage":{}}))
    .unwrap();
    log.append(&event(
        id,
        Fact::ModelRequested {
            context_breakdown: None,
            item_acceptance: false,
            effective_source_digest: Some(source.effective_source_digest),
            purpose: ModelPurpose::Summary,
            context: None,
            step_id: id.into(),
            model_id: "test".into(),
            source_scope: source.source_evidence.scope,
            source_high_water: source.source_evidence.high_water,
            source_digest: source.source_evidence.digest.clone(),
            input_digest: "input".into(),
            route_identity: format!("sha256:{}", "a".repeat(64)),
            checkpoint_event_id: previous.clone(),
        },
    ))
    .await
    .unwrap();
    log.append(&event(
        id,
        Fact::ModelCompleted {
            step_id: id.into(),
            output: output.clone(),
        },
    ))
    .await
    .unwrap();
    let checkpoint = event(
        id,
        Fact::ContextCheckpointRecorded {
            checkpoint: ContextCheckpoint {
                mode: CheckpointMode::Standalone,
                covered_through: source.source_evidence.high_water,
                source_digest: source.source_evidence.digest,
                previous_checkpoint_id: previous,
                summary: TextSummary::from_model_step(&output).unwrap(),
                summary_step_id: id.into(),
            },
        },
    );
    let checkpoint_id = checkpoint.event().id.clone();
    vec![
        checkpoint,
        event(
            id,
            Fact::InvocationEnded {
                outcome: InvocationOutcome::ContextCompactFinished {
                    outcome: CompactOutcome::Compacted {
                        checkpoint_id: checkpoint_id.clone(),
                    },
                },
            },
        ),
    ]
}

async fn checkpoint(log: &EventLog, id: &str) -> String {
    let pair = prepared_checkpoint(log, id).await;
    let checkpoint_id = pair[0].event().id.clone();
    log.append_batch(&pair).await.unwrap();
    checkpoint_id
}

async fn certificate(log: &EventLog) -> (Stamp, Certificate) {
    let (stamp, certificate) = observe(
        log.context_cache.clone(),
        &log.connection.writer(),
        "session".into(),
    )
    .await
    .unwrap();
    (
        stamp,
        certificate.expect("actual completed verification published a certificate"),
    )
}

async fn setup(path: &std::path::Path) -> (EventLog, String, String) {
    let log = EventLog::open(path).await.unwrap();
    log.create_session("session", "created", &json!({}), 1)
        .await
        .unwrap();
    let opening = closed(&log, "main").await;
    let checkpoint = checkpoint(&log, "summary").await;
    let source = log.read_model_context("session", None).await.unwrap();
    assert_eq!(
        source.baseline.unwrap().checkpoint.summary.text,
        "literal retained summary"
    );
    (log, opening, checkpoint)
}

fn tamper(path: &std::path::Path, event: &str) {
    rusqlite::Connection::open(path)
        .unwrap()
        .execute(
            "UPDATE event_log SET event_json=' ' || event_json WHERE event_id=?",
            [event],
        )
        .unwrap();
}

#[tokio::test]
async fn opening_only_then_same_invocation_progress_and_cold_reads_keep_exact_history() {
    use maka_agent_facts::{tool_call::ToolCallIdentity, tool_output::ToolSuccess};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("opt-in.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    log.create_session("session", "create", &json!({}), 1)
        .await
        .unwrap();
    let opening = event(
        "active",
        Fact::InvocationOpened {
            configuration: None,
            input: InvocationInput::Message {
                content: "literal tool-loop question".into(),
                source_messages: vec![],
                request_fingerprint: None,
            },
        },
    );
    log.append(&opening).await.unwrap();
    let empty = log
        .prepare_prune_candidates("session", Some("active"), 128, 1024 * 1024, None)
        .await
        .unwrap();
    assert!(empty.candidates.is_empty());
    assert!(empty.next.is_none());
    assert!(log.context_cache.certificates.lock().unwrap().is_empty());
    let first = log
        .read_model_context("session", Some("active"))
        .await
        .unwrap();
    assert!(first.baseline.is_none());
    assert!(first.anchor.is_none());
    assert_eq!(first.tail.len(), 1);
    let context::ContextEvent::Canonical(actual) = &first.tail[0] else {
        panic!("original opening")
    };
    assert_eq!(actual.event.id, opening.event().id);
    let Fact::InvocationOpened {
        input: InvocationInput::Message { content, .. },
        ..
    } = &actual.event.fact
    else {
        panic!("original message")
    };
    assert_eq!(content.text, "literal tool-loop question");
    let full = log
        .scoped_prefix(Selection::session("session").scope, 100, 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(first.source_evidence.digest, full.digest);
    assert_eq!(log.context_cache.stamps.load(Ordering::Relaxed), 0);
    assert_eq!(log.context_cache.has_proof("session"), Some(false));
    let mut source = first;
    let mut results = Vec::new();
    for step in ["first-tool", "second-tool"] {
        let before = log.context_cache.stamps.load(Ordering::Relaxed);
        log.append(&event(
            "active",
            Fact::ModelRequested {
                context_breakdown: None,
                item_acceptance: false,
                purpose: ModelPurpose::Main,
                context: None,
                step_id: step.into(),
                model_id: "test".into(),
                source_scope: source.source_evidence.scope.clone(),
                source_high_water: source.source_evidence.high_water,
                source_digest: source.source_evidence.digest.clone(),
                input_digest: "input".into(),
                route_identity: format!("sha256:{}", "a".repeat(64)),
                checkpoint_event_id: None,
                effective_source_digest: Some(source.effective_source_digest.clone()),
            },
        ))
        .await
        .unwrap();
        assert_eq!(
            log.context_cache.stamps.load(Ordering::Relaxed),
            before,
            "ordinary Main with no checkpoint consumes no certificate"
        );
        let output = serde_json::from_value(json!({"parts":[{"kind":"tool_call","call":{
            "id":step,"name":"Read","input":{"kind":"json","data":{"path":"file"}},"provider_executed":false
        }}],"finish_reason":"tool-calls","usage":{}})).unwrap();
        log.append(&event(
            "active",
            Fact::ModelCompleted {
                step_id: step.into(),
                output,
            },
        ))
        .await
        .unwrap();
        let operation = format!("{step}:{step}");
        log.append(&event(
            "active",
            Fact::ToolDispatched {
                presentation: None,
                title: None,
                operation_id: operation.clone(),
                call: ToolCallIdentity::provider(step.into(), step.into()),
                name: "Read".into(),
                input: json!({"path":"file"}),
            },
        ))
        .await
        .unwrap();
        let (result, _) = EventWrite::tool_success(
            format!("result-{step}"),
            std::time::SystemTime::now(),
            opening.event().invocation.clone(),
            operation,
            ToolSuccess::from(json!(format!("literal {step} output"))),
        )
        .unwrap();
        log.append(&result).await.unwrap();
        results.push(result.event().id.clone());
        source = log
            .read_model_context("session", Some("active"))
            .await
            .unwrap();
        assert!(source.baseline.is_none());
        let full = log
            .scoped_prefix(Selection::session("session").scope, 100, 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(source.source_evidence.digest, full.digest);
        let proof = log.context_cache.certificates.lock().unwrap()["session"].clone();
        assert_eq!(
            proof.raw.unwrap().through,
            source.source_evidence.high_water
        );
        assert!(
            source.tail.iter().any(
                |entry| matches!(entry, context::ContextEvent::Canonical(stored)
            if stored.event.id == result.event().id)
            ),
            "real tool result remains visible"
        );
    }
    assert!(
        log.context_cache.raw_reuses.load(Ordering::Relaxed) > 0,
        "same-invocation progress must extend the authenticated raw prefix"
    );
    let before = log.context_cache.stamps.load(Ordering::Relaxed);
    let page = log
        .prepare_prune_candidates("session", Some("active"), 1, 1024 * 1024, None)
        .await
        .unwrap();
    assert_eq!(page.candidates.len(), 1);
    assert_eq!(page.candidates[0].event_id, results[0]);
    assert_eq!(page.candidates[0].tool_call_id, "first-tool");
    assert_eq!(page.candidates[0].tool_name, "Read");
    assert_eq!(
        page.candidates[0].projection,
        maka_agent_facts::tool_output::DurableToolProjection::Text {
            text: "literal first-tool output".into(),
        }
    );
    let cursor = page.next.unwrap();
    let page = log
        .prepare_prune_candidates("session", Some("active"), 1, 1024 * 1024, Some(&cursor))
        .await
        .unwrap();
    assert_eq!(page.candidates.len(), 1);
    assert_eq!(page.candidates[0].event_id, results[1]);
    assert_eq!(page.candidates[0].tool_call_id, "second-tool");
    assert_eq!(
        page.candidates[0].projection,
        maka_agent_facts::tool_output::DurableToolProjection::Text {
            text: "literal second-tool output".into(),
        }
    );
    assert!(page.next.is_none());
    assert_eq!(
        log.context_cache.stamps.load(Ordering::Relaxed),
        before,
        "raw-only hint must not turn prune into certificate work or skip its scan"
    );
    log.append(&event(
        "active",
        Fact::InvocationEnded {
            outcome: InvocationOutcome::Completed,
        },
    ))
    .await
    .unwrap();
    log.close().await.unwrap();
    let log = EventLog::open(&path).await.unwrap();
    assert_eq!(log.context_cache.has_proof("session"), Some(false));
    let cold = log.read_model_context("session", None).await.unwrap();
    assert!(cold.baseline.is_none());
    assert!(log.context_cache.has_raw("session"));
    assert_eq!(log.context_cache.raw_reuses.load(Ordering::Relaxed), 0);
    let warm = log.read_model_context("session", None).await.unwrap();
    let full = log
        .scoped_prefix(Selection::session("session").scope, 100, 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(cold.source_evidence.digest, full.digest);
    assert_eq!(warm.source_evidence.digest, full.digest);
    assert_eq!(warm.tail.len(), cold.tail.len());
    assert_eq!(
        log.context_cache.raw_reuses.load(Ordering::Relaxed),
        1,
        "cold pure reads of non-checkpoint history must warm and reuse Raw"
    );
    log.close().await.unwrap();
}

#[tokio::test]
async fn metadata_writes_preserve_real_certificate_and_new_checkpoint_stops_at_committed_anchor() {
    let directory = tempfile::tempdir().unwrap();
    let (log, _, first) = setup(&directory.path().join("events.sqlite")).await;
    let (before, _) = certificate(&log).await;
    log.connection.run(|connection| Box::pin(async move {
        sqlx::query("INSERT INTO message_queue_state VALUES ('session',1) ON CONFLICT(session_id) DO UPDATE SET revision=revision+1").execute(&mut *connection).await?;
        sqlx::query("INSERT INTO application_ui_checkpoints VALUES ('deployment','{}','key',1,'format','{}','{}',x'0102')").execute(connection).await?;
        Ok(())
    })).await.unwrap();
    assert_eq!(
        log.close_run_interactions(
            &Invocation {
                session_id: "session".into(),
                turn_id: "main".into(),
                run_id: "main".into(),
                invocation_id: "main".into(),
            },
            maka_agent_facts::interaction::ClosureReason::TurnTerminal,
            2
        )
        .await
        .unwrap(),
        0
    );
    let (after, retained) = observe(
        log.context_cache.clone(),
        &log.connection.writer(),
        "session".into(),
    )
    .await
    .unwrap();
    assert_eq!(
        before, after,
        "queue, WITHOUT ROWID UI data and empty interaction closure do not change history evidence"
    );
    assert!(retained.is_some());
    let links = log.context_cache.validated_links.load(Ordering::Relaxed);
    let hits = log.context_cache.anchor_hits.load(Ordering::Relaxed);
    assert_eq!(
        log.read_model_context("session", None)
            .await
            .unwrap()
            .baseline
            .unwrap()
            .event_id,
        first
    );
    assert_eq!(
        log.context_cache.validated_links.load(Ordering::Relaxed),
        links
    );
    assert!(log.context_cache.anchor_hits.load(Ordering::Relaxed) > hits);
    closed(&log, "next-main").await;
    let before_compact = log.read_model_context("session", None).await.unwrap();
    let opening = log
        .append(&event(
            "prune-probe",
            Fact::InvocationOpened {
                configuration: None,
                input: InvocationInput::ContextCompact {
                    request_fingerprint: "prune-probe".into(),
                },
            },
        ))
        .await
        .unwrap();
    let hits_before_prune = log.context_cache.anchor_hits.load(Ordering::Relaxed);
    let candidates = log
        .prepare_prune_candidates("session", Some("prune-probe"), 128, 1024 * 1024, None)
        .await
        .unwrap();
    assert!(candidates.candidates.is_empty());
    assert!(candidates.next.is_none());
    assert!(
        log.context_cache.anchor_hits.load(Ordering::Relaxed) > hits_before_prune,
        "the real prune path must reuse its verified committed checkpoint"
    );
    let (_, after_prune) = certificate(&log).await;
    assert_eq!(
        after_prune.raw.unwrap().through,
        before_compact.source_evidence.high_water,
        "prune must not advance the sole raw frontier to the compact opening"
    );
    let compact = log
        .prepare_context_compaction("session", Some("prune-probe"), &CheckpointMode::Standalone)
        .await
        .unwrap();
    assert_eq!(compact.baseline.unwrap().event_id, first);
    assert!(compact.source_evidence.high_water < opening);
    assert_eq!(
        compact.source_evidence.high_water,
        before_compact.source_evidence.high_water
    );
    assert_eq!(
        compact.source_evidence.digest,
        before_compact.source_evidence.digest
    );
    assert_eq!(
        compact.effective_source_digest,
        before_compact.effective_source_digest
    );
    assert_eq!(
        certificate(&log).await.1.raw.unwrap().through,
        before_compact.source_evidence.high_water
    );
    log.append(&event(
        "prune-probe",
        Fact::InvocationEnded {
            outcome: InvocationOutcome::Cancelled {
                source: "fixture_probe_complete".into(),
            },
        },
    ))
    .await
    .unwrap();
    let second = checkpoint(&log, "next-summary").await;
    let source = log.read_model_context("session", None).await.unwrap();
    assert_eq!(source.baseline.unwrap().event_id, second);
    let (_, actual) = certificate(&log).await;
    assert_eq!(actual.checkpoint.unwrap().id, second);
    let full = log
        .scoped_prefix(Selection::session("session").scope, 100, 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(source.source_evidence.digest, full.digest);
    assert!(
        log.context_cache.validated_links.load(Ordering::Relaxed) - links <= 2,
        "only the new checkpoint link is proved, never the old chain"
    );
    log.close().await.unwrap();
}

#[tokio::test]
async fn external_changes_in_both_confirmation_gaps_use_the_pinned_full_validator() {
    for before_pin in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("events.sqlite");
        let (log, original, _) = setup(&path).await;
        let (origin, candidate) = certificate(&log).await;
        let cache = log.context_cache.clone();
        let writer = log.connection.writer();
        let original_path = path.clone();
        let result = log
            .connection
            .read_interruptible(move |connection| {
                Box::pin(async move {
                    if before_pin {
                        tamper(&original_path, &original);
                    }
                    let mut tx = connection.begin().await?;
                    sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(sequence),0) FROM event_log")
                        .fetch_one(&mut *tx)
                        .await?;
                    if !before_pin {
                        tamper(&original_path, &original);
                    }
                    let (confirmed, _) = observe(cache.clone(), &writer, "session".into()).await?;
                    assert_ne!(origin, confirmed);
                    let mut verification = Verification::new(cache.clone(), origin.clone(), None);
                    let high = candidate
                        .selection
                        .high_water(&mut tx, i64::MAX as u64)
                        .await?;
                    let result = context::read::latest_with_evidence(
                        &mut tx,
                        &candidate.selection,
                        high,
                        &mut verification,
                    )
                    .await;
                    tx.commit().await?;
                    if result.is_ok() {
                        promote(cache, &writer, origin, verification.candidate()).await?;
                    }
                    result.map(|(baseline, _)| baseline.unwrap().checkpoint.summary.text)
                })
            })
            .await;
        if before_pin {
            assert!(matches!(result, Err(StoreError::InvalidTransition(_))));
        } else {
            assert_eq!(result.unwrap(), "literal retained summary");
        }
        assert!(log.read_model_context("session", None).await.is_err());
        log.close().await.unwrap();
    }
}

#[tokio::test]
async fn cold_promotion_and_writer_begin_gap_never_relabel_an_old_snapshot() {
    for writer_gap in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("events.sqlite");
        let (log, original, _) = setup(&path).await;
        let (origin, old) = certificate(&log).await;
        if writer_gap {
            // Exact ordered gate: C1 completed, external commit, then the
            // audited writer obtains its BEGIN IMMEDIATE lock.
            tamper(&path, &original);
            let cache = log.context_cache.clone();
            let writer = log.connection.writer();
            let failures = writer.failures();
            let result = writer
                .run(move |connection| {
                    Box::pin(async move {
                        let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
                        assert!(!cache.confirm_append(&mut tx, &origin, failures).await?);
                        let mut verification = Verification::new(cache, origin, None);
                        let high = old.selection.high_water(&mut tx, i64::MAX as u64).await?;
                        context::read::latest_with_evidence(
                            &mut tx,
                            &old.selection,
                            high,
                            &mut verification,
                        )
                        .await?;
                        tx.rollback().await?;
                        Ok(())
                    })
                })
                .await;
            assert!(matches!(result, Err(StoreError::InvalidTransition(_))));
        } else {
            log.context_cache.certificates.lock().unwrap().clear();
            let (origin, none) = observe(
                log.context_cache.clone(),
                &log.connection.writer(),
                "session".into(),
            )
            .await
            .unwrap();
            assert!(none.is_none());
            let cache = log.context_cache.clone();
            let cold_origin = origin.clone();
            let candidate = log
                .connection
                .read_interruptible(move |connection| {
                    Box::pin(async move {
                        let mut tx = connection.begin().await?;
                        sqlx::query_scalar::<_, i64>(
                            "SELECT COALESCE(MAX(sequence),0) FROM event_log",
                        )
                        .fetch_one(&mut *tx)
                        .await?;
                        let selection = Selection::session("session");
                        let high = selection.high_water(&mut tx, i64::MAX as u64).await?;
                        let mut verification = Verification::new(cache, cold_origin, None);
                        let (baseline, _) = context::read::latest_with_evidence(
                            &mut tx,
                            &selection,
                            high,
                            &mut verification,
                        )
                        .await?;
                        assert_eq!(
                            baseline.unwrap().checkpoint.summary.text,
                            "literal retained summary"
                        );
                        tx.commit().await?;
                        Ok(verification.candidate())
                    })
                })
                .await
                .unwrap();
            // Verification completed under A. B commits before promotion.
            tamper(&path, &original);
            promote(
                log.context_cache.clone(),
                &log.connection.writer(),
                origin,
                candidate,
            )
            .await
            .unwrap();
            let (_, published) = observe(
                log.context_cache.clone(),
                &log.connection.writer(),
                "session".into(),
            )
            .await
            .unwrap();
            assert!(published.is_none());
        }
        assert!(log.read_model_context("session", None).await.is_err());
        log.close().await.unwrap();
    }
}

#[tokio::test]
async fn every_insert_advances_frontier_even_between_cold_observation_and_promotion() {
    let directory = tempfile::tempdir().unwrap();
    let (log, _, _) = setup(&directory.path().join("events.sqlite")).await;
    let (origin, old) = certificate(&log).await;
    let selection = old.selection.clone();
    log.context_cache.certificates.lock().unwrap().clear();
    let later = event(
        "main",
        Fact::ModelObserved {
            step_id: "late-step".into(),
            event: maka_agent_facts::model::ModelEvent::PartDelta {
                id: "text".into(),
                text: "selected later suffix".into(),
                provider_options: None,
            },
        },
    );
    let earlier = event(
        "main",
        Fact::ModelObserved {
            step_id: "late-step".into(),
            event: maka_agent_facts::model::ModelEvent::PartDelta {
                id: "text".into(),
                text: "selected inserted gap".into(),
                provider_options: None,
            },
        },
    );
    log.connection.run(move |connection| Box::pin(async move {
        sqlx::query("INSERT INTO event_log(sequence,event_id,invocation_id,kind,operation_id,event_json) VALUES(1000,?,'main','model_observed',NULL,?)")
            .bind(&later.event().id).bind(serde_json::to_string(later.event())?).execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let (same, none) = observe(
        log.context_cache.clone(),
        &log.connection.writer(),
        "session".into(),
    )
    .await
    .unwrap();
    assert!(none.is_none());
    assert_eq!(
        origin, same,
        "strict suffix preserves previously committed evidence"
    );
    let cache = log.context_cache.clone();
    let pinned = origin.clone();
    let writer = log.connection.writer();
    let (candidate, pinned_digest) = log.connection.read_interruptible(move |connection| Box::pin(async move {
        let mut tx = connection.begin().await?;
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(sequence),0) FROM event_log").fetch_one(&mut *tx).await?;
        let mut verification = Verification::new(cache, pinned, None);
        let high = selection.high_water(&mut tx, i64::MAX as u64).await?;
        assert_eq!(high, 1000, "the new row is actual selected runtime history");
        let (_, source) = context::read::latest_with_evidence(&mut tx, &selection, high, &mut verification).await?;
        writer.run(move |connection| Box::pin(async move {
            sqlx::query("INSERT INTO event_log(sequence,event_id,invocation_id,kind,operation_id,event_json) VALUES(600,?,'main','model_observed',NULL,?)")
                .bind(&earlier.event().id).bind(serde_json::to_string(earlier.event())?).execute(connection).await?;
            Ok(())
        })).await?;
        tx.commit().await?;
        let candidate = verification.candidate().expect("full cold verification completed");
        assert_eq!(candidate.raw.as_ref().unwrap().through, 1000);
        Ok((Some(candidate), source.digest))
    })).await.unwrap();
    promote(
        log.context_cache.clone(),
        &log.connection.writer(),
        origin.clone(),
        candidate,
    )
    .await
    .unwrap();
    let (changed, published) = observe(
        log.context_cache.clone(),
        &log.connection.writer(),
        "session".into(),
    )
    .await
    .unwrap();
    assert_ne!(origin, changed);
    assert!(
        published.is_none(),
        "the pinned accumulator omitted the subsequently inserted row"
    );
    let source = log.read_model_context("session", None).await.unwrap();
    assert_eq!(
        source.baseline.unwrap().checkpoint.summary.text,
        "literal retained summary"
    );
    assert_eq!(source.source_evidence.high_water, 1000);
    assert_ne!(
        source.source_evidence.digest, pinned_digest,
        "fresh history includes sequence 600 that was absent in the pinned snapshot"
    );
    let full = log
        .scoped_prefix(Selection::session("session").scope, 100, 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(source.source_evidence.digest, full.digest);
    log.close().await.unwrap();
}

#[tokio::test]
async fn main_and_temp_trigger_replace_delete_the_original_prefix_and_invalidate_before_reuse() {
    for temporary in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let (log, original, _) = setup(&directory.path().join("events.sqlite")).await;
        log.connection.run(move |connection| Box::pin(async move {
            sqlx::query("INSERT INTO message_queue_state VALUES ('session',1)").execute(&mut *connection).await?;
            let trigger = format!("CREATE {} TRIGGER rewrite_history AFTER UPDATE ON message_queue_state BEGIN
                INSERT OR REPLACE INTO event_log(event_id,invocation_id,kind,operation_id,event_json)
                SELECT event_id,invocation_id,kind,operation_id,event_json FROM event_log WHERE event_id='{}'; END",
                if temporary { "TEMP" } else { "" }, original);
            sqlx::query(sqlx::AssertSqlSafe(trigger)).execute(connection).await?;
            Ok(())
        })).await.unwrap();
        // A schema change causes a cold read; the trigger remains installed
        // while this genuine committed proof is authenticated again.
        log.read_model_context("session", None).await.unwrap();
        let (origin, prior) = certificate(&log).await;
        let cache = log.context_cache.clone();
        log.connection.run(move |connection| Box::pin(async move {
            let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
            let mut verification = Verification::new(cache.clone(), origin.clone(), Some(prior));
            sqlx::query("UPDATE message_queue_state SET revision=revision+1 WHERE session_id='session'").execute(&mut *tx).await?;
            assert_ne!(origin.generation, cache.generation.load(Ordering::Acquire), "implicit REPLACE deletion is observed inside the transaction");
            let selection = Selection::session("session");
            let high = selection.high_water(&mut tx, i64::MAX as u64).await?;
            assert!(context::read::latest_with_evidence(&mut tx, &selection, high, &mut verification).await.is_err(),
                "a cached consumer observes the trigger's invalidation before commit");
            assert!(context::read::latest_selected(&mut tx, &selection, i64::MAX as u64, &mut Verification::default()).await.is_err(),
                "unknown transactions retain the full validator");
            tx.commit().await?;
            Ok(())
        })).await.unwrap();
        assert!(log.read_model_context("session", None).await.is_err());
        log.close().await.unwrap();
    }
}

#[tokio::test]
async fn truncate_members_and_failed_transactions_revoke_combined_certificates() {
    let directory = tempfile::tempdir().unwrap();
    let (log, _, _) = setup(&directory.path().join("events.sqlite")).await;
    let (origin, _) = certificate(&log).await;
    let result = log
        .connection
        .run(|connection| {
            Box::pin(async move {
                let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
                sqlx::query("UPDATE event_log SET event_json=' ' || event_json WHERE sequence=1")
                    .execute(&mut *tx)
                    .await?;
                tx.rollback().await?;
                Err::<(), _>(StoreError::EventConflict)
            })
        })
        .await;
    assert!(matches!(result, Err(StoreError::EventConflict)));
    let (changed, none) = observe(
        log.context_cache.clone(),
        &log.connection.writer(),
        "session".into(),
    )
    .await
    .unwrap();
    assert_ne!(origin, changed);
    assert!(none.is_none());
    assert_eq!(
        log.read_model_context("session", None)
            .await
            .unwrap()
            .baseline
            .unwrap()
            .checkpoint
            .summary
            .text,
        "literal retained summary"
    );
    // Real copied membership makes DELETE-all nonempty; no WHERE and no
    // application row loop means SQLite's truncate optimization is exercised.
    let revision = log
        .get_session::<serde_json::Value>("session")
        .await
        .unwrap()
        .unwrap()
        .revision;
    assert!(matches!(
        log.copy_session(
            crate::sessions::SessionCopy {
                source_session_id: "session".into(),
                target_session_id: "other".into(),
                expected_source_revision: revision,
                purpose: maka_agent_facts::session::CopyPurpose::Branch {
                    turn_id: None,
                    side_conversation: false
                },
            },
            &json!({}),
            2
        )
        .await
        .unwrap(),
        crate::sessions::SessionCopyResult::Committed(_)
    ));
    log.read_model_context("session", None).await.unwrap();
    let (origin, _) = certificate(&log).await;
    let cache = log.context_cache.clone();
    log.connection
        .run(move |connection| {
            Box::pin(async move {
                sqlx::query("DELETE FROM session_history_members")
                    .execute(connection)
                    .await?;
                assert_ne!(origin.generation, cache.generation.load(Ordering::Acquire));
                Ok(())
            })
        })
        .await
        .unwrap();
    let (_, none) = observe(
        log.context_cache.clone(),
        &log.connection.writer(),
        "session".into(),
    )
    .await
    .unwrap();
    assert!(none.is_none());
    log.close().await.unwrap();
}

#[tokio::test]
async fn optional_publication_failure_cannot_change_a_known_successful_commit() {
    let directory = tempfile::tempdir().unwrap();
    let (log, _, _) = setup(&directory.path().join("events.sqlite")).await;
    log.append(&event(
        "committed",
        Fact::InvocationOpened {
            configuration: None,
            input: InvocationInput::Message {
                content: "known committed input".into(),
                source_messages: vec![],
                request_fingerprint: None,
            },
        },
    ))
    .await
    .unwrap();
    let source = log
        .read_model_context("session", Some("committed"))
        .await
        .unwrap();
    let requested = event(
        "committed",
        Fact::ModelRequested {
            context_breakdown: None,
            item_acceptance: false,
            purpose: ModelPurpose::Main,
            context: None,
            step_id: "committed-main".into(),
            model_id: "test".into(),
            source_scope: source.source_evidence.scope,
            source_high_water: source.source_evidence.high_water,
            source_digest: source.source_evidence.digest,
            input_digest: "input".into(),
            route_identity: format!("sha256:{}", "a".repeat(64)),
            checkpoint_event_id: Some(source.baseline.unwrap().event_id),
            effective_source_digest: Some(source.effective_source_digest),
        },
    );
    let generation = log.context_cache.generation.load(Ordering::Acquire);
    let cache = log.context_cache.clone();
    log.connection
        .run(move |connection| {
            Box::pin(async move {
                connection.lock_handle().await?.set_commit_hook(move || {
                    // Real SQLite commit succeeds; only optional cache publication
                    // loses its lock. No database or lifecycle failure is fabricated.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let _locked = cache.certificates.lock().unwrap();
                        panic!("deliberately poison the optional certificate lock");
                    }));
                    true
                });
                Ok(())
            })
        })
        .await
        .unwrap();
    let id = requested.event().id.clone();
    let sequence = log
        .append(&requested)
        .await
        .expect("the original commit was confirmed");
    let recorded: i64 = log
        .connection
        .run(move |connection| {
            Box::pin(async move {
                Ok(
                    sqlx::query_scalar("SELECT sequence FROM event_log WHERE event_id=?")
                        .bind(id)
                        .fetch_one(connection)
                        .await?,
                )
            })
        })
        .await
        .unwrap();
    assert_eq!(sequence, recorded as u64);
    assert!(log.context_cache.certificates.is_poisoned());
    assert_ne!(
        generation,
        log.context_cache.generation.load(Ordering::Acquire),
        "the consumer actually reached the failed optional publication handler"
    );
    let full = log
        .scoped_prefix(Selection::session("session").scope, 100, 1024 * 1024)
        .await
        .unwrap();
    let stamps = log.context_cache.stamps.load(Ordering::Relaxed);
    let source = log
        .read_model_context("session", Some("committed"))
        .await
        .expect("an unavailable optional hint must fall back to full history validation");
    assert_eq!(source.source_evidence.high_water, sequence);
    assert_eq!(source.source_evidence.digest, full.digest);
    assert_eq!(
        source.baseline.unwrap().checkpoint.summary.text,
        "literal retained summary"
    );
    assert_eq!(
        log.context_cache.stamps.load(Ordering::Relaxed),
        stamps,
        "an unavailable map must not be opted back in by a positive SQL hint"
    );
    log.close().await.unwrap();
}

#[tokio::test]
async fn shutdown_rejects_reader_confirmation_then_drains_its_real_rollback() {
    use futures_util::FutureExt;
    let directory = tempfile::tempdir().unwrap();
    let (log, _, _) = setup(&directory.path().join("events.sqlite")).await;
    let log = Arc::new(log);
    let (entered, pinned) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let reader = log.clone();
    let task = tokio::spawn(async move {
        let cache = reader.context_cache.clone();
        let writer = reader.connection.writer();
        reader
            .connection
            .read_interruptible(move |connection| {
                Box::pin(async move {
                    observe(cache.clone(), &writer, "session".into()).await?;
                    let mut tx = connection.begin().await?;
                    sqlx::query_scalar::<_, i64>("SELECT MAX(sequence) FROM event_log")
                        .fetch_one(&mut *tx)
                        .await?;
                    entered.send(()).unwrap();
                    released.await.unwrap();
                    observe(cache, &writer, "session".into()).await?;
                    tx.commit().await?;
                    Ok(())
                })
            })
            .await
    });
    pinned.await.unwrap();
    let mut shutdown = Box::pin(log.shutdown());
    assert!(shutdown.as_mut().now_or_never().is_none());
    // A queued barrier either drains behind Shutdown or is already refused.
    // Both outcomes prove ingress is closed before the reader is released.
    let barrier = log
        .connection
        .writer()
        .run(|_| Box::pin(async { Ok(()) }))
        .await;
    assert!(barrier.is_ok() || matches!(barrier, Err(StoreError::ConnectionClosed)));
    release.send(()).unwrap();
    assert!(matches!(
        task.await.unwrap(),
        Err(StoreError::ConnectionClosed)
    ));
    shutdown.await.unwrap();
    assert!(matches!(
        log.read_model_context("session", None).await,
        Err(StoreError::ConnectionClosed)
    ));
}

#[tokio::test]
async fn an_unsealed_checkpoint_cannot_certify_a_later_fact_in_its_own_batch() {
    let directory = tempfile::tempdir().unwrap();
    let (log, _, _) = setup(&directory.path().join("events.sqlite")).await;
    closed(&log, "second-main").await;
    let pair = prepared_checkpoint(&log, "batch-summary").await;
    log.connection.run(|connection| Box::pin(async move {
        // The plain sequence is refused by active-Session admission. These
        // actual metadata triggers exercise the supported trigger boundary:
        // hide that execution origin until the new Main has been appended.
        sqlx::query("CREATE TRIGGER hide_pending_compact AFTER INSERT ON event_log
            WHEN NEW.kind='context_checkpoint_recorded' AND NEW.invocation_id='batch-summary'
            BEGIN INSERT INTO imported_invocations(invocation_id,bundle_digest)
            VALUES('batch-summary','sha256:0000000000000000000000000000000000000000000000000000000000000000'); END")
            .execute(&mut *connection).await?;
        sqlx::query("CREATE TRIGGER restore_pending_compact AFTER INSERT ON event_log
            WHEN NEW.kind='model_requested' AND NEW.invocation_id='batch-main'
            BEGIN DELETE FROM imported_invocations WHERE invocation_id='batch-summary'; END")
            .execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let (batch, full) = log
        .connection
        .run(move |connection| {
            Box::pin(async move {
                let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
                let crate::append::AppendResult::Inserted(sequence) =
                    EventLog::append_in_transaction(&mut tx, &pair[0]).await?
                else {
                    panic!("the prepared checkpoint must be new")
                };
                // Obtain the real exact prefix after CP INSERT in this snapshot. The
                // following rollback restores both rowids and trigger side effects.
                let source =
                    context::evidence::selected(&mut tx, &Selection::session("session"), sequence)
                        .await?;
                let opening = event(
                    "batch-main",
                    Fact::InvocationOpened {
                        configuration: None,
                        input: InvocationInput::Message {
                            content: "next input".into(),
                            source_messages: vec![],
                            request_fingerprint: None,
                        },
                    },
                );
                let main = event(
                    "batch-main",
                    Fact::ModelRequested {
                        context_breakdown: None,
                        item_acceptance: false,
                        purpose: ModelPurpose::Main,
                        context: None,
                        step_id: "batch-main-step".into(),
                        model_id: "test".into(),
                        source_scope: source.scope,
                        source_high_water: source.high_water,
                        source_digest: source.digest,
                        input_digest: "input".into(),
                        route_identity: format!("sha256:{}", "a".repeat(64)),
                        checkpoint_event_id: Some(pair[0].event().id.clone()),
                        effective_source_digest: None,
                    },
                );
                let batch = vec![pair[0].clone(), opening, main, pair[1].clone()];
                let mut full = Ok(());
                for write in &batch[1..] {
                    if let Err(error) = EventLog::append_in_transaction(&mut tx, write).await {
                        full = Err(error.to_string());
                        break;
                    }
                }
                if full.is_ok() {
                    full = context::validate_batch(&mut tx, &batch)
                        .await
                        .map_err(|error| error.to_string());
                }
                tx.rollback().await?;
                Ok((batch, full))
            })
        })
        .await
        .unwrap();
    assert!(
        full.as_ref()
            .is_err_and(|reason| reason.contains("missing proof event")),
        "causal replay must require the Compact terminal before Main: {full:?}"
    );
    let ids: Vec<_> = batch.iter().map(|write| write.event().id.clone()).collect();
    let cached = log
        .append_batch(&batch)
        .await
        .map(|_| ())
        .map_err(|error| match error {
            maka_agent_facts::event::CommitError::Rejected(reason) => reason,
            other => other.to_string(),
        });
    let recorded: i64 = log
        .connection
        .run(move |connection| {
            Box::pin(async move {
                Ok(sqlx::query_scalar(
                    "SELECT COUNT(*) FROM event_log WHERE event_id IN (?, ?, ?, ?)",
                )
                .bind(&ids[0])
                .bind(&ids[1])
                .bind(&ids[2])
                .bind(&ids[3])
                .fetch_one(connection)
                .await?)
            })
        })
        .await
        .unwrap();
    assert_eq!(
        cached, full,
        "public cached append persisted {recorded} facts that causal full replay rejects"
    );
    assert_eq!(recorded, 0);
    log.close().await.unwrap();
}
