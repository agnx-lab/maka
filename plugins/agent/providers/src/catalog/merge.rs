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

use super::{ModelInfo, models_dev::Entry};
use std::collections::BTreeMap;

pub(super) struct Description {
    display_name: Option<String>,
    description: Option<String>,
    knowledge_cutoff: Option<String>,
}
impl Description {
    pub(super) fn fill(&self, model: &mut ModelInfo) {
        model.display_name = model
            .display_name
            .take()
            .or_else(|| self.display_name.clone());
        model.description = model
            .description
            .take()
            .or_else(|| self.description.clone());
        model.knowledge_cutoff = model
            .knowledge_cutoff
            .take()
            .or_else(|| self.knowledge_cutoff.clone());
    }
}

/// Only an exact models.dev OpenRouter entry may connect a live OpenRouter ID
/// to the canonical ID shared by another models.dev route. Slugs are not IDs.
pub(super) fn shared_descriptions(
    entries: &[Entry],
    router: &[ModelInfo],
) -> BTreeMap<String, Description> {
    let bridges: BTreeMap<_, _> = entries
        .iter()
        .filter(|entry| entry.provider == "openrouter")
        .filter_map(|entry| {
            entry
                .canonical
                .as_ref()
                .map(|canonical| (entry.model.id.as_str(), canonical.as_str()))
        })
        .collect();
    let mut groups: BTreeMap<&str, Vec<&ModelInfo>> = BTreeMap::new();
    for model in router {
        if let Some(canonical) = bridges.get(model.id.as_str()) {
            groups.entry(canonical).or_default().push(model);
        }
    }
    groups
        .into_iter()
        .map(|(id, models)| {
            (
                id.to_owned(),
                Description {
                    display_name: unanimous(&models, |model| model.display_name.as_ref()),
                    description: unanimous(&models, |model| model.description.as_ref()),
                    knowledge_cutoff: unanimous(&models, |model| model.knowledge_cutoff.as_ref()),
                },
            )
        })
        .collect()
}

fn unanimous(
    models: &[&ModelInfo],
    field: impl Fn(&ModelInfo) -> Option<&String>,
) -> Option<String> {
    let mut values = models.iter().filter_map(|model| field(model));
    let first = values.next()?;
    values.all(|value| value == first).then(|| first.clone())
}

pub(super) fn reported(mut reported: ModelInfo, public: &ModelInfo) -> ModelInfo {
    let own_efforts = reported.thinking_levels.is_some();
    let reasoning_disabled = reported.capabilities.and_then(|caps| caps.reasoning) == Some(false);
    macro_rules! fill {
        ($($field:ident),* $(,)?) => { $(if reported.$field.is_none() { reported.$field = public.$field.clone(); })* };
    }
    fill!(
        display_name,
        description,
        api_protocol,
        context_window,
        input_limit,
        max_output_tokens,
        supports_reasoning_summary,
        knowledge_cutoff,
        structured_output,
        last_updated,
        modalities
    );
    if !own_efforts && !reasoning_disabled {
        fill!(thinking_levels, default_thinking_level);
    }
    if let Some(defaults) = public.capabilities {
        let caps = reported.capabilities.get_or_insert_default();
        macro_rules! fill_caps {
            ($($field:ident),* $(,)?) => { $(caps.$field = caps.$field.or(defaults.$field);)* };
        }
        fill_caps!(
            chat,
            vision,
            reasoning,
            function_calling,
            parallel_tool_calls,
            image_generation,
            web_search
        );
    }
    reported
}
