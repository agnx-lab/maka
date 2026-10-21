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
use maka_app_client::Client;
use maka_capabilities::call::Attachments;

async fn activate(view: &mut AppView, label: &str) {
    tab_to_label(view, label).await;
    view.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
}

async fn fill(view: &mut AppView, label: &str, value: &str) {
    tab_to_label(view, label).await;
    shortcut(view, 'a');
    view.handle_event(Event::Paste(value.into()));
    let (done, committed) = tokio::sync::oneshot::channel();
    view.control()
        .unwrap()
        .advance_clock(Duration::ZERO, move || {
            let _ = done.send(());
        });
    tokio::time::timeout(Duration::from_secs(20), committed)
        .await
        .unwrap()
        .unwrap();
}

async fn mutate(client: &Client, method: &str, input: Value) -> Value {
    let mutation = client
        .settings_mutation(
            Uuid::new_v4(),
            "management",
            method,
            input,
            Attachments::new(),
        )
        .await
        .unwrap();
    mutation.prepare().await.unwrap();
    let reply = mutation.confirm().await.unwrap();
    let Receipt::Recorded {
        outcome: OperationOutcome::Returned { value },
        ..
    } = reply.receipt
    else {
        panic!("management result is not recorded: {reply:?}");
    };
    value
}

async fn target(client: &Client) -> (Value, Value) {
    let first = client
        .query("management", "instances", json!({"kind":"start"}))
        .await
        .unwrap();
    assert_eq!(first["instances"].as_array().unwrap().len(), 8);
    assert!(
        !first["instances"]
            .as_array()
            .unwrap()
            .iter()
            .any(|instance| instance["path"] == json!(["optional-agent-2"]))
    );
    let second = client
        .query(
            "management",
            "instances",
            json!({"kind":"continue","source":first["source"],"after":first["next"]}),
        )
        .await
        .unwrap();
    assert_eq!(second["source"], first["source"]);
    let target = second["instances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|instance| instance["path"] == json!(["optional-agent-2"]))
        .unwrap()
        .clone();
    (first["source"].clone(), target)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn plugin_settings_use_real_host_inventory_cas_configuration_and_enablement_without_mixing_the_draft()
 {
    tokio::time::timeout(Duration::from_secs(90), async {
        let fixture = Fixture::plugin_settings().await;
        let client = fixture.client().await;
        client.save_draft("agent", &fixture.session, EDITOR, None, "PLUGIN_SETTINGS_ORIGINAL_DRAFT".into()).await.unwrap();
        let (mut view, bindings) = fixture.view(client.clone(), Some(fixture.session.clone()));
        wait_view(&mut view, |screen| screen.contains("PLUGIN_SETTINGS_ORIGINAL_DRAFT") && screen.contains("Enter sends")).await;
        support::click_button(&mut view, "Connections & models").await;
        activate(&mut view, "Settings section Plugins").await;
        wait_enabled_node(&mut view, "Search Host plugins").await;
        fill(&mut view, "Search Host plugins", "optional-agent-2").await;
        // This real instance is on Host page two. The UI must retrieve that
        // page itself, without a user-facing load-more control.
        support::click_button(&mut view, "Open plugin optional-agent-2").await;
        activate(&mut view, "Edit plugin setting locale").await;
        fill(&mut view, "Plugin value locale", "zh-CN").await;
        let (source, original) = target(&client).await;
        let external = mutate(&client, "configure", json!({
            "expectedRevision":source["revision"],"instance":original["instance"],
            "changes":[{"path":["locale"],"value":{"kind":"text","text":"en-GB"}}]
        })).await;
        assert_eq!(external["status"], "committed");
        activate(&mut view, "Save Host plugin setting").await;
        wait_view(&mut view, |screen| screen.contains("Plugin configuration changed elsewhere.")).await;
        tab_to_label(&mut view, "Plugin value locale").await;
        assert_eq!(view.surface().input_context().unwrap().text.as_deref(), Some("zh-CN"), "CAS conflict must retain the unsaved editor");
        support::click_button(&mut view, "Refresh plugin configuration").await;
        activate(&mut view, "Edit plugin setting locale").await;
        fill(&mut view, "Plugin value locale", "zh-CN").await;
        activate(&mut view, "Save Host plugin setting").await;
        wait_enabled_node(&mut view, "Enable Host plugin").await;
        let (source, configured) = target(&client).await;
        let mut after = Value::Null;
        let saved_locale = loop {
            let page = client.query("management", "readSettings", json!({"source":source,"instance":configured["instance"],"path":[],"after":after})).await.unwrap();
            if let Some(locale) = page["fields"].as_array().unwrap().iter().find(|field| field["key"] == "locale") { break locale["value"].clone(); }
            after = page["next"].clone();
            assert!(!after.is_null(), "the actual Agent locale field was not returned");
        };
        assert_eq!(saved_locale, json!({"kind":"text","text":"zh-CN"}));
        activate(&mut view, "Enable Host plugin").await;
        wait_enabled_node(&mut view, "Disable Host plugin").await;
        assert_eq!(target(&client).await.1["disabled"], false);
        activate(&mut view, "Disable Host plugin").await;
        wait_enabled_node(&mut view, "Enable Host plugin").await;
        let (final_source, disabled) = target(&client).await;
        assert_eq!(disabled["disabled"], true);
        assert_eq!(final_source["revision"].as_u64().unwrap(), source["revision"].as_u64().unwrap() + 2);
        assert_eq!(disabled["package"], original["package"], "enablement must not manufacture another package identity");
        activate(&mut view, "Back from plugins settings").await;
        tab_to_label(&mut view, "Search Host plugins").await;
        assert_eq!(view.surface().input_context().unwrap().text.as_deref(), Some("optional-agent-2"), "returning to the plugin list must retain its search filter");
        activate(&mut view, "Back from plugins settings").await;
        wait_view(&mut view, |screen| screen.contains("PLUGIN_SETTINGS_ORIGINAL_DRAFT") && screen.contains("Enter sends")).await;
        tab_to_label(&mut view, "Message composer").await;
        assert_eq!(view.surface().input_context().unwrap().text.as_deref(), Some("PLUGIN_SETTINGS_ORIGINAL_DRAFT"));
        support::wait_draft(&client, &fixture.session, "PLUGIN_SETTINGS_ORIGINAL_DRAFT").await;
        assert!(client.pending_settings().await.unwrap().is_empty());
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 0, "plugin configuration must not send any model request");
        evidence("plugins-real-cas-and-owner", &view.snapshot());
        drop(view); bindings.close().await.unwrap(); fixture.finish().await;
    }).await.expect("real Host plugin settings flow timed out");
}
