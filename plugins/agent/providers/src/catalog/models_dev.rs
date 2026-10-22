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

use super::{ModelInfo, text, token_limit};
use maka_agent_facts::configuration::ModelCapabilities;
use maka_agent_facts::execution::ThinkingLevel;
use serde_json::Value;

pub(super) struct Entry {
    pub provider: String,
    pub model: ModelInfo,
    pub canonical: Option<String>,
}

pub(super) fn parse(root: &Value) -> Result<Vec<Entry>, String> {
    let providers = root.as_object().ok_or("invalid models.dev catalog")?;
    let mut entries = vec![];
    for (provider, row) in providers {
        if row.get("id").and_then(Value::as_str) != Some(provider.as_str()) {
            return Err("models.dev provider ID differs from its catalog key".into());
        }
        let models = row
            .get("models")
            .and_then(Value::as_object)
            .ok_or("invalid models.dev models")?;
        for (id, row) in models {
            if row.get("id").and_then(Value::as_str) != Some(id.as_str()) {
                return Err("models.dev model ID differs from its catalog key".into());
            }
            let canonical = text(row.get("canonical_model_id"))?;
            if canonical.as_ref().is_some_and(|id| id.trim().is_empty()) {
                return Err("invalid models.dev canonical model ID".into());
            }
            entries.push(Entry {
                provider: provider.clone(),
                model: model(provider, row)?,
                canonical,
            });
        }
    }
    Ok(entries)
}

fn model(provider: &str, row: &Value) -> Result<ModelInfo, String> {
    let id = row["id"].as_str().expect("catalog key already checked");
    let modalities = super::modalities(
        row.pointer("/modalities/input"),
        row.pointer("/modalities/output"),
    )?;
    let mut capabilities = ModelCapabilities {
        reasoning: row.get("reasoning").and_then(Value::as_bool),
        function_calling: row.get("tool_call").and_then(Value::as_bool),
        ..Default::default()
    };
    if let Some(modalities) = &modalities {
        use maka_agent_facts::configuration::ModelModality::{Image, Text};
        if row
            .pointer("/modalities/input")
            .is_some_and(Value::is_array)
        {
            capabilities.vision = Some(modalities.input.contains(&Image));
        }
        if row
            .pointer("/modalities/output")
            .is_some_and(Value::is_array)
        {
            capabilities.image_generation = Some(modalities.output.contains(&Image));
            if !modalities.output.contains(&Text) {
                capabilities.chat = Some(false);
            }
        }
    }
    let mut levels = vec![];
    if let Some(options) = row.get("reasoning_options") {
        for option in options
            .as_array()
            .ok_or("invalid models.dev reasoning controls")?
        {
            match option.get("type").and_then(Value::as_str) {
                Some("effort") => {
                    if let Some(values) = option.get("values") {
                        for value in values
                            .as_array()
                            .ok_or("invalid models.dev effort values")?
                        {
                            let level = match value.as_str() {
                                Some("ultra") => Some(ThinkingLevel::Ultra),
                                Some(value) => super::openrouter::effort(value),
                                None => None,
                            };
                            if let Some(level) = level
                                && !levels.contains(&level)
                            {
                                levels.push(level);
                            }
                        }
                    }
                }
                Some("toggle") if toggle_off(provider) => {
                    if !levels.contains(&ThinkingLevel::Off) {
                        levels.push(ThinkingLevel::Off);
                    }
                }
                _ => {} // Budgets and future controls are not effort levels.
            }
        }
    }
    if capabilities.reasoning == Some(false) {
        levels.clear();
    }
    let model = ModelInfo {
        id: id.into(),
        display_name: text(row.get("name"))?,
        description: text(row.get("description"))?,
        context_window: token_limit(row.pointer("/limit/context")),
        input_limit: token_limit(row.pointer("/limit/input")),
        max_output_tokens: token_limit(row.pointer("/limit/output")),
        thinking_levels: Some(levels),
        knowledge_cutoff: text(row.get("knowledge"))?,
        structured_output: row.get("structured_output").and_then(Value::as_bool),
        last_updated: text(row.get("last_updated"))?,
        capabilities: Some(capabilities),
        modalities,
        ..Default::default()
    };
    model.validate()?;
    Ok(model)
}

fn toggle_off(provider: &str) -> bool {
    // These encoders expose the standardized `none` effort. A native vendor's
    // separate thinking switch is not interchangeable with compatible effort.
    matches!(provider, "openrouter" | "openai")
}
