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

use super::Snapshot;
use maka_agent_facts::configuration::{
    ModelCapabilities, ModelInfo, ModelModalities, ModelModality,
};
use maka_agent_facts::execution::ThinkingLevel::{High, Low, Max, Off};
use serde_json::json;

#[test]
fn newly_published_openrouter_model_is_available_without_a_bundled_entry() {
    let bundled = Snapshot::bundled().unwrap();
    let id = "deepseek/deepseek-v4.1-flash-1017";
    assert_eq!(bundled.get("openrouter", id), None);
    // Public /api/v1/models row retained in the source audit; unrelated pricing
    // and presentation fields are deliberately absent from this metadata input.
    let router = json!({"data":[{
        "id":"deepseek/deepseek-v4.1-flash-1017",
        "name":"DeepSeek: DeepSeek V4.1 Flash 1017","context_length":1048576,
        "architecture":{"input_modalities":["text","image"],"output_modalities":["text"]},
        "top_provider":{"context_length":1048576,"max_completion_tokens":393216},
        "supported_parameters":["tools","structured_outputs","reasoning","reasoning_effort"],
        "reasoning":{"mandatory":false,"default_enabled":true,
            "supported_efforts":["max","high","low","none"],"default_effort":"high"},
        "knowledge_cutoff":null
    }]});
    let snapshot = Snapshot::from_sources(None, Some(&router)).unwrap();
    let model = snapshot.resolve("openrouter", ModelInfo::new(id));
    assert_eq!(
        model.display_name.as_deref(),
        Some("DeepSeek: DeepSeek V4.1 Flash 1017")
    );
    assert_eq!(model.context_window, Some(1048576));
    assert_eq!(model.max_output_tokens, Some(393216));
    assert_eq!(model.knowledge_cutoff, None);
    assert_eq!(model.thinking_levels, Some(vec![Max, High, Low, Off]));
    assert_eq!(model.default_thinking_level, Some(High));
    assert_eq!(model.capabilities.unwrap().vision, Some(true));
    assert_eq!(model.capabilities.unwrap().function_calling, Some(true));
    assert_eq!(model.structured_output, Some(true));
}

#[test]
fn new_public_model_keeps_each_routes_own_controls_and_limits() {
    let models_dev = json!({
        "deepseek":{"id":"deepseek","models":{
            "brand-new-native":{"id":"brand-new-native","name":"Native name",
                "canonical_model_id":"vendor/new-model","reasoning":true,"tool_call":false,
                "reasoning_options":[{"type":"effort","values":["low","high"]}],
                "limit":{"context":8192,"input":4096,"output":2048}}
        }},
        "openrouter":{"id":"openrouter","models":{
            "vendor/new-model:free":{"id":"vendor/new-model:free","canonical_model_id":"vendor/new-model",
                "reasoning_options":[{"type":"effort","values":["low"]}]}
        }}
    });
    let openrouter = json!({"data":[{
        "id":"vendor/new-model:free","canonical_slug":"this-is-not-the-bridge",
        "name":"Gateway name","description":"Shared model description","knowledge_cutoff":"2026-01",
        "context_length":131072,"top_provider":{"max_completion_tokens":32768},
        "architecture":{"input_modalities":["text","image"],"output_modalities":["text"]},
        "supported_parameters":["tools","structured_outputs","reasoning"],
        "reasoning":{"mandatory":true,"supported_efforts":["max","high"],"default_effort":"max"}
    }]});
    let snapshot = Snapshot::from_sources(Some(&models_dev), Some(&openrouter)).unwrap();
    let native = snapshot.get("deepseek", "brand-new-native").unwrap();
    assert_eq!(native.display_name.as_deref(), Some("Native name"));
    assert_eq!(
        native.description.as_deref(),
        Some("Shared model description")
    );
    assert_eq!(native.knowledge_cutoff.as_deref(), Some("2026-01"));
    assert_eq!(native.context_window, Some(8192));
    assert_eq!(native.input_limit, Some(4096));
    assert_eq!(native.max_output_tokens, Some(2048));
    assert_eq!(native.capabilities.unwrap().function_calling, Some(false));
    assert_eq!(native.capabilities.unwrap().vision, None);
    assert_eq!(native.thinking_levels, Some(vec![Low, High]));
    assert_eq!(native.default_thinking_level, None);
    let gateway = snapshot.get("openrouter", "vendor/new-model:free").unwrap();
    assert_eq!(gateway.display_name.as_deref(), Some("Gateway name"));
    assert_eq!(gateway.context_window, Some(131072));
    assert_eq!(gateway.max_output_tokens, Some(32768));
    assert_eq!(gateway.input_limit, None);
    assert_eq!(gateway.thinking_levels, Some(vec![Max, High]));
    assert_eq!(gateway.default_thinking_level, Some(Max));
    assert_eq!(gateway.capabilities.unwrap().vision, Some(true));
    assert_eq!(gateway.capabilities.unwrap().function_calling, Some(true));
    assert_eq!(gateway.structured_output, Some(true));
    assert_eq!(snapshot.get("openrouter", "brand-new-native"), None);
    assert_eq!(snapshot.get("deepseek", "vendor/new-model:free"), None);
}

#[test]
fn conflicting_gateway_descriptions_never_cross_routes_or_use_slugs_as_ids() {
    let models_dev = json!({
        "private-relay":{"id":"private-relay","models":{
            "exact-local":{"id":"exact-local","canonical_model_id":"vendor/base"},
            "unrelated":{"id":"unrelated","canonical_model_id":"slug-only"}
        }},
        "openrouter":{"id":"openrouter","models":{
            "vendor/base":{"id":"vendor/base","canonical_model_id":"vendor/base"},
            "vendor/base:free":{"id":"vendor/base:free","canonical_model_id":"vendor/base"}
        }}
    });
    let router = json!({"data":[
        {"id":"vendor/base","name":"Paid name","description":"Same description","knowledge_cutoff":"2026-01",
            "canonical_slug":"slug-only","context_length":1000},
        {"id":"vendor/base:free","name":"Free name","description":"Same description","knowledge_cutoff":"2025-01",
            "context_length":2000}
    ]});
    let snapshot = Snapshot::from_sources(Some(&models_dev), Some(&router)).unwrap();
    let local = snapshot.get("private-relay", "exact-local").unwrap();
    assert_eq!(local.display_name, None);
    assert_eq!(local.description.as_deref(), Some("Same description"));
    assert_eq!(local.knowledge_cutoff, None);
    assert_eq!(local.context_window, None);
    assert_eq!(local.thinking_levels, Some(vec![]));
    assert_eq!(
        snapshot
            .get("private-relay", "unrelated")
            .unwrap()
            .description,
        None
    );
}

#[test]
fn a_complete_public_record_replaces_bundled_guesses_and_has_no_invented_default() {
    let bundled = Snapshot::bundled().unwrap();
    assert!(
        !bundled
            .get("openai", "gpt-5.2")
            .unwrap()
            .thinking_levels
            .as_ref()
            .unwrap()
            .is_empty()
    );
    let models_dev = json!({"openai":{"id":"openai","models":{
        "gpt-5.2":{"id":"gpt-5.2","name":"Current record","reasoning":true,
            "tool_call":false,"structured_output":false,
            "modalities":{"input":["text"],"output":["text"]}}
    }}});
    let snapshot = Snapshot::from_sources(Some(&models_dev), None).unwrap();
    let current = snapshot.get("openai", "gpt-5.2").unwrap();
    assert_eq!(current.display_name.as_deref(), Some("Current record"));
    assert_eq!(current.context_window, None);
    assert_eq!(current.thinking_levels, Some(vec![]));
    assert_eq!(current.default_thinking_level, None);
    assert_eq!(current.structured_output, Some(false));
    assert_eq!(current.capabilities.unwrap().function_calling, Some(false));
    assert_eq!(current.capabilities.unwrap().vision, Some(false));
    // An absent source/model retains the compiled floor, not a stale public-only row.
    assert_eq!(
        snapshot.get("anthropic", "claude-opus-4-8"),
        bundled.get("anthropic", "claude-opus-4-8")
    );
    let empty = Snapshot::from_sources(Some(&json!({})), Some(&json!({"data":[]}))).unwrap();
    assert_eq!(
        empty.get("openai", "gpt-5.2"),
        bundled.get("openai", "gpt-5.2")
    );
}

#[test]
fn account_false_empty_and_unknown_defaults_override_public_facts() {
    let router = json!({"data":[{"id":"new/account-model","name":"Public label","context_length":8192,
        "supported_parameters":["tools","reasoning"],
        "reasoning":{"mandatory":false,"default_enabled":true,
            "supported_efforts":["high","low","none"],"default_effort":"high"}}]});
    let snapshot = Snapshot::from_sources(None, Some(&router)).unwrap();
    let reported = ModelInfo {
        thinking_levels: Some(vec![]),
        default_thinking_level: None,
        structured_output: Some(false),
        context_window: Some(123),
        capabilities: Some(ModelCapabilities {
            reasoning: Some(false),
            function_calling: Some(false),
            ..Default::default()
        }),
        modalities: Some(ModelModalities {
            input: vec![],
            output: vec![],
        }),
        ..ModelInfo::new("new/account-model")
    };
    let effective = snapshot.resolve("openrouter", reported.clone());
    assert_eq!(effective.display_name.as_deref(), Some("Public label"));
    assert_eq!(effective.thinking_levels, Some(vec![]));
    assert_eq!(effective.default_thinking_level, None);
    assert_eq!(effective.context_window, Some(123));
    assert_eq!(effective.structured_output, Some(false));
    assert_eq!(effective.capabilities, reported.capabilities);
    assert_eq!(effective.modalities, reported.modalities);
    let own_choices = ModelInfo {
        thinking_levels: Some(vec![Low, High]),
        ..ModelInfo::new("new/account-model")
    };
    let effective = snapshot.resolve("openrouter", own_choices);
    assert_eq!(effective.thinking_levels, Some(vec![Low, High]));
    assert_eq!(effective.default_thinking_level, None);
    let disabled = snapshot.resolve(
        "openrouter",
        ModelInfo {
            capabilities: Some(ModelCapabilities {
                reasoning: Some(false),
                ..Default::default()
            }),
            ..ModelInfo::new("new/account-model")
        },
    );
    assert_eq!(disabled.thinking_levels, None);
    assert_eq!(disabled.default_thinking_level, None);
    let inherited = snapshot.resolve("openrouter", ModelInfo::new("new/account-model"));
    assert_eq!(inherited.thinking_levels, Some(vec![High, Low, Off]));
    assert_eq!(inherited.default_thinking_level, Some(High));
}

#[test]
fn models_dev_efforts_are_explicit_and_native_toggle_is_not_a_gateway_effort() {
    let models_dev = json!({
        "deepseek":{"id":"deepseek","models":{"new-model":{
            "id":"new-model","reasoning":true,
            "reasoning_options":[{"type":"toggle"},{"type":"effort","values":["low","high","future",null]}]}}},
        "openrouter":{"id":"openrouter","models":{"vendor/toggle":{
            "id":"vendor/toggle","reasoning":true,"reasoning_options":[{"type":"toggle"}]}}},
        "openai":{"id":"openai","models":{"new-effort":{
            "id":"new-effort","reasoning":true,
            "reasoning_options":[{"type":"effort","values":["none","high"]},{"type":"budget_tokens","min":2048}]}}}
    });
    let snapshot = Snapshot::from_sources(Some(&models_dev), None).unwrap();
    assert_eq!(
        snapshot
            .get("deepseek", "new-model")
            .unwrap()
            .thinking_levels,
        Some(vec![Low, High])
    );
    assert_eq!(
        snapshot
            .get("openrouter", "vendor/toggle")
            .unwrap()
            .thinking_levels,
        Some(vec![Off])
    );
    assert_eq!(
        snapshot
            .get("openai", "new-effort")
            .unwrap()
            .thinking_levels,
        Some(vec![Off, High])
    );
    assert_eq!(
        snapshot
            .get("openai", "new-effort")
            .unwrap()
            .default_thinking_level,
        None
    );
}

#[test]
fn malformed_sources_and_conflicting_exact_identities_are_not_empty_successes() {
    for models_dev in [
        json!(null),
        json!([]),
        json!({"x":{"id":"other","models":{}}}),
        json!({"x":{"id":"x","models":{"key":{"id":"other"}}}}),
    ] {
        assert!(Snapshot::from_sources(Some(&models_dev), None).is_err());
    }
    for router in [
        json!(null),
        json!({}),
        json!({"data":[{}]}),
        json!({"data":[{"id":"same"},{"id":"same"}]}),
    ] {
        assert!(Snapshot::from_sources(None, Some(&router)).is_err());
    }
    let snapshot = Snapshot::from_sources(None, Some(&json!({"data":[{
        "id":"image/new","architecture":{"input_modalities":["text","file","image"],"output_modalities":["image"]}
    }]}))).unwrap();
    let model = snapshot.get("openrouter", "image/new").unwrap();
    assert_eq!(model.capabilities.unwrap().chat, Some(false));
    assert_eq!(model.capabilities.unwrap().image_generation, Some(true));
    assert_eq!(
        model.modalities.as_ref().unwrap().input,
        [ModelModality::Text, ModelModality::Image]
    );
}

#[test]
fn captured_public_fact_or_absence_never_uses_todays_fallback() {
    let record = ModelInfo {
        display_name: Some("Accepted public name".into()),
        context_window: Some(1234),
        ..ModelInfo::new("gpt-5.2")
    };
    let captured = Snapshot::for_model("openai", "gpt-5.2", Some(record.clone())).unwrap();
    assert_eq!(captured.get("openai", "gpt-5.2"), Some(&record));
    assert_eq!(captured.get("openrouter", "gpt-5.2"), None);
    let absent = Snapshot::for_model("openai", "gpt-5.2", None).unwrap();
    assert_eq!(absent.get("openai", "gpt-5.2"), None);
    assert_eq!(
        absent.resolve("openai", ModelInfo::new("gpt-5.2")),
        ModelInfo::new("gpt-5.2")
    );
    assert!(Snapshot::for_model("openai", "a-different-model", Some(record)).is_err());
}

#[tokio::test]
async fn explicit_refresh_enqueues_synchronously_and_reports_the_owner_receipt() {
    let catalog = super::Catalog::new(Snapshot::bundled().unwrap());
    // A standalone embedding returns an already completed cached receipt.
    catalog.request_refresh().unwrap().await.unwrap().unwrap();
    let mut requests = catalog.attach_refresh();
    let completed = catalog.request_refresh().unwrap();
    requests
        .recv()
        .await
        .unwrap()
        .send(Err("refresh kept the previous valid source".into()))
        .unwrap();
    assert_eq!(
        completed.await.unwrap(),
        Err("refresh kept the previous valid source".into())
    );
    drop(requests);
    assert!(catalog.request_refresh().is_err());
    let mut replacement = catalog.attach_refresh();
    let completed = catalog.request_refresh().unwrap();
    replacement.recv().await.unwrap().send(Ok(())).unwrap();
    completed.await.unwrap().unwrap();
}
