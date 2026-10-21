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
async fn escape_stops_each_current_execution_after_its_scoped_binding_is_refreshed() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mut fixture = Fixture::open().await;
        let client = fixture.client().await;
        let (mut view, bindings) = fixture.view(client.clone(), Some(fixture.session.clone()));
        wait_view(&mut view, |screen| screen.contains("Enter sends")).await;
        support::click_button(&mut view, "Choose model and effort").await;
        wait_view(&mut view, |screen| screen.contains("Search models…") && !screen.contains("Loading…")).await;
        support::click_button(&mut view, "Use fixture-model · NSH fixture").await;
        wait_view(&mut view, |screen| screen.contains("fixture-model · Default · Ctrl+G") && screen.contains("Enter sends") && !screen.contains("Close · Esc") && !screen.contains("Back · Esc")).await;
        let mut previous = None;
        for message in ["FIRST_ESC_RUN", "SECOND_ESC_RUN"] {
            tab_to_label(&mut view, "Message composer").await;
            view.handle_event(Event::Paste(message.into()));
            support::wait_draft(&client, &fixture.session, message).await;
            wait_enabled_node(&mut view, "Send message").await;
            view.handle_event(Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE,
            )));
            let mut request = fixture.next_request().await;
            assert!(
                request.body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| {
                        entry["role"] == "user" && entry["content"].to_string().contains(message)
                    })
            );
            let answer = format!("{message}_RESPONSE");
            request.delta(&answer).await;
            wait_view(&mut view, |screen| screen.contains(&answer)).await;
            let observed = client
                .query("agent", "observe", json!({"sessionId":fixture.session}))
                .await
                .unwrap();
            let invocation = observed["run"]["invocationId"].as_str().unwrap().to_owned();
            assert_ne!(previous.as_ref(), Some(&invocation));
            wait_enabled_node(&mut view, "Stop response").await;
            view.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
            tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    let execution = client
                        .query(
                            "agent",
                            "execution",
                            json!({
                                "sessionId":fixture.session,"invocationId":invocation
                            }),
                        )
                        .await
                        .unwrap();
                    if execution["execution"]["state"] == "cancelled" {
                        assert_eq!(execution["kind"], "found");
                        assert_eq!(execution["execution"]["invocationId"], invocation);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("Escape did not stop its current exact execution");
            drop(request);
            fixture.idle(&client, &fixture.session).await;
            wait_view(&mut view, |screen| {
                screen.contains("Stopped") && screen.contains(&answer)
            })
            .await;
            previous = Some(invocation);
        }
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 2);
        drop(view);
        bindings.close().await.unwrap();
        fixture.finish().await;
    })
    .await
    .expect("the current-execution Escape workflow did not finish");
}
