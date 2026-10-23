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
use maka_agent_facts::event::Fact;

fn server(
    record: &std::path::Path,
    preapproved: bool,
) -> maka_agent_facts::configuration::mcp::ServerConfiguration {
    serde_json::from_value(json!({"name":"fixture","transport":{"type":"stdio","command":"node","args":[std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mcp/tests/server.mjs"),"normal",record],"cwd":record.parent().unwrap()},"tools":["echo"],"preapproved":preapproved})).unwrap()
}
fn calls(record: &std::path::Path) -> usize {
    std::fs::read_to_string(record)
        .unwrap()
        .lines()
        .filter(|line| serde_json::from_str::<Value>(line).unwrap()["method"] == "tools/call")
        .count()
}
async fn approval(
    fixture: &Fixture,
    session: &str,
) -> maka_agent_facts::interaction::InteractionRecord {
    let mut changes = fixture.store.subscribe_commits();
    loop {
        if let Some(record) = fixture
            .store
            .pending_interactions(session)
            .await
            .unwrap()
            .into_iter()
            .next()
        {
            return record;
        }
        changes.changed().await.unwrap();
    }
}
#[tokio::test]
async fn mcp_native_direct_and_code_mode_calls_share_approval_permissions_and_journal() {
    tokio::time::timeout(Duration::from_secs(60), exercise_mcp_permissions())
        .await
        .unwrap();
}
async fn exercise_mcp_permissions() {
    for (code, decision, read_only, preapproved, trusted_readonly, revoke) in [
        (false, "allow", false, false, false, false),
        (false, "deny", false, false, false, false),
        (true, "allow", false, true, false, false),
        (false, "allow", true, true, false, false),
        (false, "allow", true, true, true, false),
        (false, "allow", false, false, false, true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fixture = Fixture::configured(
            &format!("http://{}/v1", listener.local_addr().unwrap()),
            None,
            None,
            None,
            |settings| {
                let mut config = server(&record, preapproved);
                if trusted_readonly {
                    config.read_only_tools = vec!["echo".into()];
                }
                settings.mcp_servers = vec![config];
            },
        )
        .await;
        selection::edit(&fixture, |changes| {
            changes.model_overrides = Patch::Set(BTreeMap::from([(
                "fixture-model".into(),
                ModelOverride {
                    code_mode: Some(code),
                    ..Default::default()
                },
            )]))
        })
        .await;
        let session = fixture.create("MCP permissions").await;
        if read_only {
            files::change(&fixture, &session, "read_only", false, true).await;
        }
        fixture.send(&session).await.unwrap();
        let (mut first, _) = listener.accept().await.unwrap();
        let prompt = request(&mut first).await;
        if code {
            assert!(
                prompt["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == "exec")
            );
            service_tools::named_tool_response(&mut first,"exec",json!({"code":"const r = await tools.mcp__fixture__echo({marker:'MCP_RESULT'}); if (r.isError || r.structuredContent.marker !== 'MCP_RESULT') throw Error('MCP result lost'); text(r.structuredContent);"})).await;
        } else {
            assert!(
                prompt["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == "mcp__fixture__echo")
            );
            service_tools::named_tool_response(
                &mut first,
                "mcp__fixture__echo",
                json!({"marker":"MCP_RESULT"}),
            )
            .await;
        }
        if !preapproved {
            let pending = approval(&fixture, &session).await;
            assert_eq!(calls(&record), 0, "confirmation must precede MCP effect");
            assert!(
                matches!(pending.request,maka_agent_facts::interaction::InteractionRequest::ServiceApproval{ref tool_name,..} if tool_name=="mcp__fixture__echo")
            );
            if revoke {
                files::change(&fixture, &session, "read_only", false, true).await;
            }
            let result=fixture.mutation(2,"respond",json!({"sessionId":session,"requestId":pending.request_id,"answer":{"kind":"service_approval","decision":decision}}),Uuid::new_v4()).await.unwrap();
            assert_eq!(result["record"]["outcome"]["decision"], decision);
        }
        let (mut next, _) = listener.accept().await.unwrap();
        let result = request(&mut next).await;
        let should_call = decision == "allow" && (!read_only || trusted_readonly) && !revoke;
        assert_eq!(calls(&record), usize::from(should_call));
        if should_call {
            assert!(
                result["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| message["role"] == "tool"
                        && message.to_string().contains("MCP_RESULT"))
            );
        }
        response(&mut next).await;
        fixture.drained().await;
        let facts = service_tools::facts(&fixture, &session).await;
        if should_call {
            assert!(facts.iter().any(|fact|matches!(fact,Fact::ToolDispatched{name,call,..} if name=="mcp__fixture__echo" && (!code || matches!(call.origin,maka_agent_facts::tool_call::ToolOrigin::CodeMode{..})))));
        }
        if read_only && !trusted_readonly {
            assert!(facts.iter().any(
                |fact| matches!(fact,Fact::ToolRejected{name,..} if name=="mcp__fixture__echo")
            ));
        }
        fixture.app.shutdown().await.unwrap();
        fixture.config.shutdown().await.unwrap();
    }
}
