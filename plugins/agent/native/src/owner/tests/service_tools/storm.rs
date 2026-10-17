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

//! One real service turn per process; sequential effects and query fanout are distinct workloads.
use super::*;
use futures_util::FutureExt;
use maka_agent_facts::{
    context::ModelPurpose,
    event::InvocationOutcome,
    model::{ModelFinishReason, ModelPart, TextKind},
    tool_output::ToolOutput,
};
use maka_plugin_core::operation::{OperationCleanup, OperationOutcome, Receipt};
use std::{panic::AssertUnwindSafe, time::Instant};

const FINAL: &str = "K40_ALL_SERVICE_RESULTS_CONFIRMED";

#[derive(Clone, Copy)]
struct Case {
    calls: usize,
    bytes: usize,
    javascript: bool,
    query: bool,
}

impl Case {
    fn environment(query: bool) -> Self {
        let number =
            |name, fallback| std::env::var(name).map_or(fallback, |v| v.parse::<usize>().unwrap());
        let backend = std::env::var("MAKA_TOOL_STORM_BACKEND").unwrap_or("native".into());
        assert!(matches!(backend.as_str(), "native" | "javascript"));
        let case = Self {
            calls: number("MAKA_TOOL_STORM_CALLS", 8),
            bytes: number("MAKA_TOOL_STORM_PAYLOAD_BYTES", 128),
            javascript: backend == "javascript",
            query,
        };
        assert!(case.calls > 0);
        case
    }
    fn business(self, index: usize) -> Value {
        json!({"effects":if self.query {0} else {index+1},"payload":payload(index, self.bytes)})
    }
}

fn payload(index: usize, bytes: usize) -> String {
    (0..bytes)
        .map(|offset| char::from(b'a' + ((index + offset) % 26) as u8))
        .collect()
}
fn call_id(index: usize) -> String {
    format!("storm-{index:04}")
}

fn usage() -> (f64, u64) {
    let mut value = std::mem::MaybeUninit::<libc::rusage>::uninit();
    assert_eq!(
        unsafe { libc::getrusage(libc::RUSAGE_SELF, value.as_mut_ptr()) },
        0
    );
    let value = unsafe { value.assume_init() };
    let seconds = |v: libc::timeval| v.tv_sec as f64 + v.tv_usec as f64 / 1_000_000.0;
    let peak = value.ru_maxrss as u64;
    #[cfg(not(target_os = "macos"))]
    let peak = peak * 1024;
    (seconds(value.ru_utime) + seconds(value.ru_stime), peak)
}

async fn reply(
    socket: &mut TcpStream,
    case: Case,
    indices: std::ops::Range<usize>,
    final_text: bool,
    progress: &Arc<StdMutex<Value>>,
) -> usize {
    let mut body = String::new();
    for (slot, index) in indices.enumerate() {
        let delta = json!({"id":"storm-model","choices":[{"index":0,"delta":{"tool_calls":[{
            "index":slot,"id":call_id(index),"type":"function","function":{"name":"Counter",
            "arguments":json!({"amount":1,"note":payload(index,case.bytes)}).to_string()}
        }]},"finish_reason":null}]});
        // One event per call, including large query fanout; never one giant arguments event.
        body.push_str(&format!("data: {delta}\n\n"));
    }
    if final_text {
        body.push_str(&format!("data: {}\n\n", json!({"id":"storm-model","choices":[{"index":0,"delta":{"content":FINAL},"finish_reason":null}]})));
    }
    body.push_str(&format!("data: {}\n\ndata: [DONE]\n\n", json!({"id":"storm-model","choices":[{"index":0,"delta":{},"finish_reason":if final_text {"stop"} else {"tool_calls"}}]})));
    let wire = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut written = 0;
    while written < wire.len() {
        let count = socket.write(&wire.as_bytes()[written..]).await.unwrap();
        assert_ne!(count, 0, "EOF while writing controlled SSE");
        written += count;
        let mut progress = progress.lock().unwrap();
        progress["response_wire_bytes"] =
            json!(progress["response_wire_bytes"].as_u64().unwrap() + count as u64);
    }
    wire.len()
}

async fn serve(
    listener: TcpListener,
    store: Arc<maka_event_log::EventLog>,
    session: String,
    case: Case,
    progress: Arc<StdMutex<Value>>,
) -> Value {
    let mut after = 0;
    let mut mains = 0;
    let mut request_bytes = 0;
    let mut response_bytes = 0;
    let mut archives = BTreeMap::new();
    let mut budgets = Vec::new();
    let mut latencies = Vec::new();
    let mut emitted_at = None;
    let mut confirmed = BTreeSet::new();
    loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        let body = request(&mut socket).await; // Original auth/EOF/content-length checks.
        request_bytes += serde_json::to_vec(&body).unwrap().len();
        {
            let mut progress = progress.lock().unwrap();
            progress["http_requests"] = json!(progress["http_requests"].as_u64().unwrap() + 1);
            progress["normalized_request_json_bytes"] = json!(request_bytes);
        }
        let through = *store.subscribe_commits().borrow();
        let mut purpose = None;
        loop {
            let page = store
                .session_events(&session, after, through, 512, usize::MAX)
                .await
                .unwrap();
            for stored in page.events {
                match stored.event.fact {
                    Fact::ModelRequested {
                        purpose: role,
                        context,
                        context_breakdown,
                        ..
                    } => {
                        purpose = Some(role);
                        budgets.push(json!({"purpose":role,"context":context,"context_breakdown":context_breakdown,
                            "http_max_tokens":body.get("max_tokens"),"http_max_completion_tokens":body.get("max_completion_tokens")}));
                    }
                    Fact::ToolResultArchived { placeholder } => {
                        let DurableToolProjection::Json { value } =
                            placeholder.to_model_projection().unwrap()
                        else {
                            panic!("archive projection")
                        };
                        archives.insert(placeholder.identity.tool_call_id, value);
                    }
                    _ => {}
                }
            }
            if let Some(next) = page.next_after {
                after = next;
            } else {
                after = through;
                break;
            }
        }
        progress.lock().unwrap()["last_canonical_purpose"] = json!(purpose);
        if purpose == Some(ModelPurpose::Summary) {
            return json!({"status":"reached_maintenance","main_requests":mains,"summary_requests":1,
                "http_requests":mains+1,"normalized_request_json_bytes":request_bytes,"response_wire_bytes":response_bytes,
                "confirmed_results":confirmed.len(),"step_confirmation_ms":latencies,"requests":budgets});
        }
        assert_eq!(
            purpose,
            Some(ModelPurpose::Main),
            "HTTP request must have a real canonical role"
        );
        let messages = body["messages"].as_array().unwrap();
        assert!(has_tool(&body, "Counter"));
        let expected_indices = if mains == 0 {
            0..0
        } else if case.query {
            0..case.calls
        } else {
            mains - 1..mains
        };
        for index in expected_indices {
            let id = call_id(index);
            let result = messages
                .iter()
                .rev()
                .find(|m| m["role"] == "tool" && m["tool_call_id"] == id)
                .expect("the actual result reached the next Main");
            let actual: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
            let expected = archives
                .get(&id)
                .cloned()
                .unwrap_or_else(|| case.business(index));
            assert_eq!(actual, expected, "model projection for {id}");
            assert!(actual.get("receipt").is_none() && actual.get("stateDomain").is_none());
            assert!(confirmed.insert(index));
            progress.lock().unwrap()["confirmed_results"] = json!(confirmed.len());
        }
        if let Some(at) = emitted_at.take() {
            latencies.push(Instant::now().duration_since(at).as_secs_f64() * 1000.0);
        }
        mains += 1;
        let finished = confirmed.len() == case.calls;
        let indices = if finished {
            0..0
        } else if case.query {
            0..case.calls
        } else {
            mains - 1..mains
        };
        emitted_at = Some(Instant::now());
        response_bytes += reply(&mut socket, case, indices, finished, &progress).await;
        if finished {
            return json!({"status":"completed","main_requests":mains,"summary_requests":0,"http_requests":mains,
                "normalized_request_json_bytes":request_bytes,"response_wire_bytes":response_bytes,
                "confirmed_results":confirmed.len(),"step_confirmation_ms":latencies,"requests":budgets,
                "archived_projections":archives.len()});
        }
    }
}

async fn business_read(fixture: &Fixture, binding: Binding) -> Result<Value, String> {
    fixture
        .app
        .client()
        .start(
            binding,
            "read".into(),
            json!({"amount":1,"note":""}),
            scope(&fixture.issuer, true),
            Attachments::new().into(),
        )
        .await?
        .result()
        .await
}

async fn verify(fixture: &Fixture, session: &str, invocation: &str, case: Case) -> Value {
    let through = *fixture.store.subscribe_commits().borrow();
    let mut after = 0;
    let mut dispatched = BTreeMap::new();
    let mut settled = BTreeSet::new();
    let mut raw_bytes = 0;
    let mut projected_bytes = 0;
    let mut final_completions = 0;
    let mut completed = 0;
    let mut maintenance = 0;
    let mut purposes = Vec::new();
    loop {
        let page = fixture
            .store
            .session_events(session, after, through, 512, usize::MAX)
            .await
            .unwrap();
        for stored in page.events {
            assert_eq!(stored.event.invocation.invocation_id, invocation);
            match &stored.event.fact {
                Fact::ToolDispatched {
                    operation_id,
                    call,
                    name,
                    input,
                    ..
                } => {
                    assert_eq!(name, "Counter");
                    let index = (0..case.calls)
                        .find(|index| call.tool_call_id == call_id(*index))
                        .expect("unique real call id");
                    assert_eq!(input, &json!({"amount":1,"note":payload(index,case.bytes)}));
                    assert!(dispatched.insert(operation_id.clone(), index).is_none());
                }
                Fact::ToolSettled {
                    operation_id,
                    outcome,
                } => {
                    let index = dispatched[operation_id];
                    assert!(settled.insert(index));
                    let ToolOutcome::Succeeded { .. } = outcome else {
                        panic!("unexpected tool settlement: {outcome:?}")
                    };
                    assert_eq!(
                        outcome_projection(outcome).as_ref(),
                        &DurableToolProjection::Json {
                            value: case.business(index)
                        }
                    );
                    projected_bytes += serde_json::to_vec(&case.business(index)).unwrap().len();
                    let raw = fixture
                        .store
                        .resolve_tool_result(session, &stored.event.id)
                        .await
                        .unwrap();
                    raw_bytes += serde_json::to_vec(&raw).unwrap().len();
                    match raw {
                        ToolOutput::Operation(reply) if !case.query => {
                            assert_eq!(reply.reference.method, "write");
                            assert_eq!(reply.reference.export, "Counter");
                            assert_eq!(reply.reference.namespace, id(21));
                            let Receipt::Recorded {
                                outcome: OperationOutcome::Returned { value },
                                cleanup: OperationCleanup::Confirmed,
                            } = reply.receipt
                            else {
                                panic!("original returned receipt and cleanup")
                            };
                            assert_eq!(value, case.business(index));
                        }
                        ToolOutput::Json(value) if case.query => {
                            assert_eq!(value, case.business(index))
                        }
                        other => panic!("wrong original service result: {other:?}"),
                    }
                }
                Fact::ModelRequested { purpose, .. } => purposes.push(*purpose),
                Fact::ModelCompleted { output, .. } => {
                    let text: String = output
                        .parts
                        .iter()
                        .filter_map(|p| match p {
                            ModelPart::Text {
                                text,
                                text_kind: TextKind::Text,
                                ..
                            } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect();
                    if !text.is_empty() {
                        assert_eq!(text, FINAL);
                        assert_eq!(output.finish_reason, ModelFinishReason::Stop);
                        final_completions += 1;
                    }
                }
                Fact::ToolResultArchived { .. } | Fact::ContextCheckpointRecorded { .. } => {
                    maintenance += 1
                }
                Fact::InvocationEnded { outcome } => {
                    assert!(matches!(outcome, InvocationOutcome::Completed));
                    completed += 1;
                }
                _ => {}
            }
        }
        if let Some(next) = page.next_after {
            assert!(next > after);
            after = next;
        } else {
            break;
        }
    }
    assert_eq!(dispatched.len(), case.calls);
    assert_eq!(settled.len(), case.calls);
    assert_eq!(completed, 1);
    assert_eq!(final_completions, 1);
    assert!(purposes.iter().all(|p| *p == ModelPurpose::Main));
    assert_eq!(purposes.len(), if case.query { 2 } else { case.calls + 1 });
    // Exact original assistant text through the public, fixed-fence paged history path.
    let mut input = json!({"sessionId":session,"through":through,"position":{"kind":"start"}});
    let mut final_text = String::new();
    loop {
        let page = fixture.query("history", input.clone()).await.unwrap();
        assert_eq!(page["through"], through);
        if page["kind"] == "preparing" {
            tokio::task::yield_now().await;
            continue;
        }
        for chunk in page["chunks"].as_array().unwrap() {
            if chunk["role"] == "assistant" && chunk["field"] == "body" {
                assert_eq!(chunk["offset"], final_text.len());
                assert_eq!(chunk["totalBytes"], FINAL.len());
                final_text.push_str(chunk["text"].as_str().unwrap());
            }
        }
        if page["after"].is_null() {
            break;
        }
        input["position"] = json!({"kind":"after","cursor":page["after"]});
    }
    assert_eq!(final_text, FINAL);
    let execution = fixture
        .query(
            "execution",
            json!({"sessionId":session,"invocationId":invocation}),
        )
        .await
        .unwrap();
    assert_eq!(execution["execution"]["state"], "completed");
    let observed: contract::Observation = serde_json::from_value(
        fixture
            .query("observe", json!({"sessionId":session}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        observed.session.activity,
        contract::SessionActivity::Idle
    ));
    assert!(!observed.compacting);
    json!({"tool_dispatches":dispatched.len(),"tool_settlements":settled.len(),"raw_result_json_bytes":raw_bytes,
        "business_result_json_bytes":projected_bytes,"original_payload_bytes":case.calls*case.bytes,
        "maintenance_facts":maintenance,"model_requests":purposes.len(),"completed_invocations":completed,
        "final_assistant_bytes":final_text.len(),"through":through})
}

async fn run(case: Case) -> Value {
    let whole = Instant::now();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let counter = Counter::new(Behavior::Echo);
    let mut setup = if case.javascript {
        Setup::javascript()
    } else {
        Setup::native(counter.clone(), true)
    };
    setup.echo = true;
    setup.query = case.query;
    let fixture = Fixture::with_services(
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        None,
        None,
        Some(setup),
    )
    .await;
    let prepared = AssertUnwindSafe(async {
        let session = fixture.create("K40 actual service tool storm").await;
        let client = fixture.app.client();
        let binding = client
            .bind(client.activation(&id(1)).unwrap(), "service".into())
            .await
            .unwrap()
            .remove(0);
        let before = business_read(&fixture, binding.clone()).await.unwrap();
        assert_eq!(before, json!({"effects":0,"payload":""}));
        (session, binding, before)
    })
    .catch_unwind()
    .await;
    let (session, binding, business_before) = match prepared {
        Ok(prepared) => prepared,
        Err(panic) => {
            let app = fixture.app.shutdown().await;
            let config = fixture.config.shutdown().await;
            println!(
                "MAKA_AGENT_TOOL_STORM {}",
                json!({"status":"setup_failed","calls_requested":case.calls,
                "payload_bytes":case.bytes,"backend":if case.javascript {"javascript"} else {"native"},
                "cleanup":{"app":app.err(),"config":config.as_ref().err().map(ToString::to_string)}})
            );
            std::panic::resume_unwind(panic);
        }
    };
    let entered_before = counter.entered.available_permits();
    let store = fixture.store.clone();
    let server_session = session.clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    let progress = Arc::new(StdMutex::new(
        json!({"http_requests":0,"normalized_request_json_bytes":0,
        "response_wire_bytes":0,"confirmed_results":0}),
    ));
    let server_progress = progress.clone();
    let server = tokio::spawn(async move {
        let result = AssertUnwindSafe(serve(
            listener,
            store,
            server_session,
            case,
            server_progress,
        ))
        .catch_unwind()
        .await;
        let _ = send.send(result);
    });
    let cpu = usage().0;
    let start = Instant::now();
    let mut invocation = None;
    let checked = tokio::time::timeout(
        Duration::from_secs(600),
        AssertUnwindSafe(async {
            let sent = fixture.send(&session).await.unwrap();
            assert_eq!(sent["kind"], "started");
            invocation = Some(sent["invocationId"].as_str().unwrap().to_owned());
            let admission = start.elapsed().as_secs_f64() * 1000.0;
            // This isolated fixture has no competing closer; avoid the shared busy drainer.
            assert!(fixture.owner.workers.close());
            let mut metrics = receive
                .await
                .unwrap()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            if metrics["status"] != "completed" {
                fixture
                    .mutation(
                        2,
                        "stop",
                        json!({"sessionId":session,"invocationId":invocation}),
                        Uuid::new_v4(),
                    )
                    .await
                    .unwrap();
            }
            fixture.owner.workers.wait().await;
            assert!(fixture.owner.workers.reopen());
            metrics["admission_ms"] = json!(admission);
            metrics["turn_wall_ms"] = json!(start.elapsed().as_secs_f64() * 1000.0);
            metrics["turn_cpu_seconds"] = json!(usage().0 - cpu);
            metrics["turn_peak_rss_bytes"] = json!(usage().1);
            if metrics["status"] == "completed" {
                if !case.javascript {
                    assert_eq!(
                        counter.entered.available_permits() - entered_before,
                        case.calls
                    );
                    assert_eq!(
                        counter.effects.load(Ordering::SeqCst),
                        if case.query { 0 } else { case.calls }
                    );
                    metrics["native_tool_handler_entries"] = json!(case.calls);
                }
                metrics["canonical"] =
                    verify(&fixture, &session, invocation.as_deref().unwrap(), case).await;
            }
            metrics
        })
        .catch_unwind(),
    )
    .await;
    if !matches!(&checked, Ok(Ok(_))) {
        if let Some(invocation) = &invocation {
            let _ = fixture
                .mutation(
                    2,
                    "stop",
                    json!({"sessionId":session,"invocationId":invocation}),
                    Uuid::new_v4(),
                )
                .await;
        }
        server.abort();
    }
    let served = server.await;
    let business_after = business_read(&fixture, binding).await;
    let app = fixture.app.shutdown().await;
    let config = fixture.config.shutdown().await;
    // Retain actual partial counts on timeout, panic, or maintenance boundary.
    let through = *fixture.store.subscribe_commits().borrow();
    let mut counts =
        json!({"dispatches":0,"settlements":0,"summary_requests":0,"checkpoints":0,"archives":0});
    let mut after = 0;
    loop {
        let page = match fixture
            .store
            .session_events(&session, after, through, 512, usize::MAX)
            .await
        {
            Ok(page) => page,
            Err(error) => {
                counts["read_error"] = json!(error.to_string());
                break;
            }
        };
        for event in page.events {
            let failure = match &event.event.fact {
                Fact::ToolSettled { outcome, .. } => {
                    !matches!(outcome, ToolOutcome::Succeeded { .. })
                }
                Fact::InvocationEnded { outcome } => {
                    matches!(outcome, InvocationOutcome::Failed { .. })
                }
                Fact::ModelInterrupted { .. } => true,
                _ => false,
            };
            if failure && counts["first_failure"].is_null() {
                counts["first_failure"] = serde_json::to_value(&event.event.fact).unwrap();
            }
            let key = match event.event.fact {
                Fact::ToolDispatched { .. } => Some("dispatches"),
                Fact::ToolSettled { .. } => Some("settlements"),
                Fact::ModelRequested {
                    purpose: ModelPurpose::Summary,
                    ..
                } => Some("summary_requests"),
                Fact::ContextCheckpointRecorded { .. } => Some("checkpoints"),
                Fact::ToolResultArchived { .. } => Some("archives"),
                _ => None,
            };
            if let Some(key) = key {
                counts[key] = json!(counts[key].as_u64().unwrap() + 1);
            }
        }
        if let Some(next) = page.next_after {
            after = next;
        } else {
            counts["complete"] = json!(true);
            break;
        }
    }
    let controls_empty = fixture.owner.runs.lock().await.controls.is_empty();
    let mut result = match &checked {
        Ok(Ok(value)) => value.clone(),
        Ok(Err(panic)) => {
            json!({"status":"failed_assertion","error":panic.downcast_ref::<String>().map(String::as_str).or_else(||panic.downcast_ref::<&str>().copied())})
        }
        Err(error) => json!({"status":"fixture_timeout","error":error.to_string()}),
    };
    result["scenario"] = json!(if case.query {
        "query_parallel_fanout"
    } else {
        "mutation_sequential"
    });
    result["backend"] = json!(if case.javascript {
        "javascript"
    } else {
        "native"
    });
    result["calls_requested"] = json!(case.calls);
    result["payload_bytes"] = json!(case.bytes);
    result["actual_partial_counts"] = counts;
    result["http_progress"] = progress.lock().unwrap().clone();
    result["partial_counts_snapshot"] = json!("after_stop_and_application_shutdown");
    result["business_after_snapshot"] = json!("after_stop_before_application_shutdown");
    result["business_before"] = business_before;
    result["business_after"] = json!(business_after.as_ref().ok());
    result["business_after_error"] = json!(business_after.as_ref().err());
    result["native_effects"] = if case.javascript {
        Value::Null
    } else {
        json!(counter.effects.load(Ordering::SeqCst))
    };
    result["first_effect_observed_ms"] = if case.javascript || case.query {
        Value::Null
    } else {
        json!(
            counter
                .first_effect
                .get()
                .map(|at| at.duration_since(start).as_secs_f64() * 1000.0)
        )
    };
    if let Some(ms) = result["turn_wall_ms"].as_f64() {
        result["settlements_per_second"] = json!(
            result["actual_partial_counts"]["settlements"]
                .as_u64()
                .unwrap() as f64
                / (ms / 1000.0)
        );
    }
    result["cleanup"] = json!({"app":app.as_ref().err(),"config":config.as_ref().err().map(ToString::to_string),"controls_empty":controls_empty,
        "server_joined":true,"server_result_ok":served.is_ok()});
    result["whole_scenario_wall_ms"] = json!(whole.elapsed().as_secs_f64() * 1000.0);
    result["whole_process_peak_rss_bytes"] = json!(usage().1);
    result["driver_included"] = json!(true);
    result["query_fanout_does_not_assert_handler_overlap"] = json!(case.query);
    println!("MAKA_AGENT_TOOL_STORM {result}");
    app.unwrap();
    config.unwrap();
    assert!(controls_empty);
    if result["status"] == "completed" {
        assert_eq!(
            business_after.unwrap(),
            json!({"effects":if case.query {0} else {case.calls},"payload":""})
        );
    }
    match checked {
        Ok(Ok(_)) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(error) => panic!("tool storm did not settle: {error}"),
    }
    assert!(served.is_ok());
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit real service sequential mutation workload, one case per process"]
async fn real_service_tool_storm() {
    assert_eq!(run(Case::environment(false)).await["status"], "completed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit real service query fanout workload, one case per process"]
async fn real_service_query_fanout() {
    assert_eq!(run(Case::environment(true)).await["status"], "completed");
}
