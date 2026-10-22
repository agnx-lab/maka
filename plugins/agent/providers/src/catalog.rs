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

//! Immutable, route-specific public model facts. Network and publication belong
//! to the product owner; account observations and user policy remain separate.

use maka_agent_facts::configuration::{ModelInfo, validation};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::{RwLock, mpsc, oneshot};

mod merge;
mod models_dev;
pub(crate) mod openrouter;
#[cfg(test)]
mod tests;

type Models = BTreeMap<String, BTreeMap<String, ModelInfo>>;
type RefreshCompletion = oneshot::Sender<Result<(), String>>;

/// The owner holds the write guard across its catalog SQL commit and Arc
/// publication. Readers retain the read guard while observing the SQL revision.
pub struct Catalog {
    pub snapshot: RwLock<Arc<Snapshot>>,
    refresh: Mutex<Option<mpsc::UnboundedSender<RefreshCompletion>>>,
}
impl Catalog {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot: RwLock::new(Arc::new(snapshot)),
            refresh: Mutex::new(None),
        }
    }

    /// Without a runtime owner, this catalogue is a deliberate cached/bundled
    /// embedding. Otherwise await the owner's current/next refresh receipt.
    pub async fn refresh(&self) -> Result<(), String> {
        let sender = self.refresh.lock().expect("catalog refresh lock").clone();
        let Some(sender) = sender else {
            return Ok(());
        };
        let (complete, completed) = oneshot::channel();
        sender
            .send(complete)
            .map_err(|_| "public catalog refresh owner is closed".to_owned())?;
        completed
            .await
            .map_err(|_| "public catalog refresh receipt is closed".to_owned())?
    }

    /// Exactly one product driver owns refreshes. Its network work occurs
    /// outside the snapshot write guard; attached requests share its result.
    pub fn attach_refresh(&self) -> mpsc::UnboundedReceiver<RefreshCompletion> {
        let mut current = self.refresh.lock().expect("catalog refresh lock");
        assert!(
            current
                .as_ref()
                .is_none_or(mpsc::UnboundedSender::is_closed),
            "public catalog already has a refresh owner"
        );
        let (sender, receiver) = mpsc::unbounded_channel();
        *current = Some(sender);
        receiver
    }
}

#[derive(Debug)]
pub struct Snapshot {
    models: Models,
}

impl Snapshot {
    /// Compiled observations are only the floor for models absent from public
    /// sources. They do not establish an otherwise unknown provider default.
    pub fn bundled() -> Result<Self, String> {
        let mut models = Models::new();
        for (provider, facts) in crate::facts::all().map_err(|error| error.to_string())? {
            let entries = models.entry(provider.clone()).or_default();
            for (id, known) in &facts.models {
                let metadata = &known.metadata;
                let mut capabilities = metadata.capabilities.unwrap_or_default();
                capabilities.chat = capabilities
                    .chat
                    .or(Some(known.entry.can_use_as_chat_default));
                capabilities.vision = capabilities.vision.or(Some(known.entry.supports_vision));
                entries.insert(
                    id.clone(),
                    ModelInfo {
                        id: id.clone(),
                        display_name: metadata.display_name.clone(),
                        description: metadata.description.clone(),
                        context_window: metadata.context_window,
                        input_limit: metadata.input_limit,
                        max_output_tokens: metadata.max_output_tokens,
                        thinking_levels: Some(known.entry.thinking_levels.clone()),
                        knowledge_cutoff: metadata.knowledge_cutoff.clone(),
                        structured_output: metadata.structured_output,
                        last_updated: metadata.last_updated.clone(),
                        capabilities: Some(capabilities),
                        modalities: metadata.modalities.clone(),
                        ..Default::default()
                    },
                );
            }
        }
        Ok(Self { models })
    }

    /// Each present source must be a complete valid JSON payload. Cache/fallback
    /// selection occurs before this pure boundary, without changing source IDs.
    pub fn from_sources(
        models_dev: Option<&Value>,
        openrouter: Option<&Value>,
    ) -> Result<Self, String> {
        let mut snapshot = Self::bundled()?;
        let entries = models_dev
            .map(models_dev::parse)
            .transpose()?
            .unwrap_or_default();
        let router = openrouter
            .map(openrouter::parse)
            .transpose()?
            .unwrap_or_default();
        let descriptions = merge::shared_descriptions(&entries, &router);
        for mut entry in entries {
            if entry.provider != "openrouter"
                && let Some(common) = entry.canonical.as_ref().and_then(|id| descriptions.get(id))
            {
                common.fill(&mut entry.model);
            }
            snapshot
                .models
                .entry(entry.provider)
                .or_default()
                .insert(entry.model.id.clone(), entry.model);
        }
        for model in router {
            snapshot
                .models
                .entry("openrouter".into())
                .or_default()
                .insert(model.id.clone(), model);
        }
        Ok(snapshot)
    }

    pub fn get(&self, provider: &str, exact_model_id: &str) -> Option<&ModelInfo> {
        self.models.get(provider)?.get(exact_model_id)
    }

    /// Restore the one public observation captured at admission. A captured
    /// absence must remain absent even if today's bundled/public catalog has it.
    pub fn for_model(
        provider: &str,
        exact_model_id: &str,
        record: Option<ModelInfo>,
    ) -> Result<Self, String> {
        let mut models = Models::new();
        if let Some(record) = record {
            record.validate()?;
            if record.id != exact_model_id {
                return Err("captured public model identity differs from its selection".into());
            }
            models
                .entry(provider.into())
                .or_default()
                .insert(record.id.clone(), record);
        }
        Ok(Self { models })
    }

    /// Only missing account facts are filled. An account's own effort list also
    /// owns its unknown default; catalog defaults must not reinterpret that list.
    pub fn resolve(&self, provider: &str, reported: ModelInfo) -> ModelInfo {
        match self.get(provider, &reported.id) {
            Some(public) => merge::reported(reported, public),
            None => reported,
        }
    }
}

pub(super) fn text(value: Option<&Value>) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err("invalid public model text".into()),
    }
}

pub(super) fn token_limit(value: Option<&Value>) -> Option<u64> {
    let number = value?.as_f64()?;
    (number > 0.0 && number <= validation::MAX_SAFE_INTEGER as f64 && number.fract() == 0.0)
        .then_some(number as u64)
}

pub(super) fn modalities(
    input: Option<&Value>,
    output: Option<&Value>,
) -> Result<Option<maka_agent_facts::configuration::ModelModalities>, String> {
    use maka_agent_facts::configuration::{ModelModalities, ModelModality};
    fn parse(value: Option<&Value>) -> Result<Option<Vec<ModelModality>>, String> {
        let values = match value {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::Array(values)) => values,
            _ => return Err("invalid public model modalities".into()),
        };
        let mut result = vec![];
        for value in values {
            let modality = match value.as_str() {
                Some("text") => ModelModality::Text,
                Some("image") => ModelModality::Image,
                Some("audio") => ModelModality::Audio,
                Some("pdf") => ModelModality::Pdf,
                Some("video") => ModelModality::Video,
                _ => continue,
            };
            if !result.contains(&modality) {
                result.push(modality);
            }
        }
        Ok(Some(result))
    }
    let input = parse(input)?;
    let output = parse(output)?;
    Ok(
        (input.is_some() || output.is_some()).then(|| ModelModalities {
            input: input.unwrap_or_default(),
            output: output.unwrap_or_default(),
        }),
    )
}
