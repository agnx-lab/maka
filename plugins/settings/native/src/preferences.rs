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

use maka_agent_facts::configuration::policy::EnabledPolicy;
use maka_agent_facts::configuration::policy::Personalization;
use maka_agent_facts::configuration::policy::PrivacyPolicy;
use maka_agent_facts::configuration::policy::RuntimePolicyMutationResult;
use maka_agent_facts::language::{Language, LanguagePreference};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SettingsPreferencesRead")]
pub struct Read {}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "SettingsAssistantPreferences")]
pub struct Preferences {
    pub language: Option<LanguagePreference>,
    pub resolved_language: Language,
    pub compaction_prompt: String,
    pub default_compaction_prompt: String,
    pub revision: u64,
    #[schemars(length(max = 256))]
    pub display_name: String,
    #[schemars(length(max = 4096))]
    pub assistant_tone: String,
    pub workspace_instructions: bool,
    pub incognito_active: bool,
    pub shell_titles: bool,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "SettingsPreferencesUpdate")]
pub struct Update {
    #[schemars(range(max = 9_007_199_254_740_991u64))]
    pub expected_revision: u64,
    pub mutation: Mutation,
}
#[derive(Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[schemars(rename = "SettingsPreferencesMutation")]
pub enum Mutation {
    Language {
        preference: LanguagePreference,
    },
    CompactionPrompt {
        prompt: Option<String>,
    },
    Personalization {
        #[schemars(length(max = 256))]
        display_name: String,
        #[schemars(length(max = 4096))]
        assistant_tone: String,
    },
    WorkspaceInstructions {
        enabled: bool,
    },
    Privacy {
        incognito_active: bool,
    },
    ShellTitles {
        enabled: bool,
    },
}
#[derive(Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[schemars(rename = "SettingsPreferencesUpdated")]
pub enum Updated {
    Committed {
        revision: u64,
    },
    RevisionConflict {
        expected_revision: u64,
        actual_revision: u64,
    },
    Rejected,
}

pub(crate) async fn read(
    store: &maka_config::ConfigurationStore,
    configured_locale: LanguagePreference,
    native_language: Language,
    input: Value,
) -> Result<Value, String> {
    let _: Read = serde_json::from_value(input).map_err(|_| "settings_input_invalid")?;
    let snapshot = store
        .runtime_policy()
        .await
        .map_err(|_| "settings_store_unavailable")?;
    let language = snapshot
        .policy
        .language
        .unwrap_or(configured_locale)
        .resolve(native_language);
    super::connections::encode(Preferences {
        language: snapshot.policy.language,
        resolved_language: language,
        compaction_prompt: snapshot.policy.compaction_prompt.unwrap_or_else(|| {
            maka_agent_facts::context::default_compaction_prompt(language).into()
        }),
        default_compaction_prompt: maka_agent_facts::context::default_compaction_prompt(language)
            .into(),
        revision: snapshot.revision,
        display_name: snapshot.policy.personalization.display_name,
        assistant_tone: snapshot.policy.personalization.assistant_tone,
        workspace_instructions: snapshot.policy.workspace_instructions.enabled,
        incognito_active: snapshot.policy.privacy.incognito_active,
        shell_titles: snapshot.policy.shell.titles,
    })
}
pub(crate) async fn update(
    store: &maka_config::ConfigurationStore,
    input: Value,
) -> Result<Value, String> {
    let Ok(input) = serde_json::from_value::<Update>(input) else {
        return super::connections::encode(Updated::Rejected);
    };
    let result = match input.mutation {
        Mutation::Language { preference } => {
            store
                .set_language(input.expected_revision, preference)
                .await
        }
        Mutation::CompactionPrompt { prompt } => {
            store
                .set_compaction_prompt(input.expected_revision, prompt)
                .await
        }
        Mutation::ShellTitles { enabled } => {
            store
                .set_shell_titles(input.expected_revision, enabled)
                .await
        }
        Mutation::Privacy { incognito_active } => {
            store
                .set_privacy(input.expected_revision, PrivacyPolicy { incognito_active })
                .await
        }
        Mutation::Personalization {
            display_name,
            assistant_tone,
        } => {
            store
                .set_personalization(
                    input.expected_revision,
                    Personalization {
                        display_name,
                        assistant_tone,
                    },
                )
                .await
        }
        Mutation::WorkspaceInstructions { enabled } => {
            store
                .set_workspace_instructions(input.expected_revision, EnabledPolicy { enabled })
                .await
        }
    };
    let result = match result {
        Ok(RuntimePolicyMutationResult::Committed { revision }) => Updated::Committed { revision },
        Ok(RuntimePolicyMutationResult::RevisionConflict {
            expected_revision,
            actual_revision,
        }) => Updated::RevisionConflict {
            expected_revision,
            actual_revision,
        },
        Err(maka_config::ConfigError::Invalid(_)) => Updated::Rejected,
        Err(_) => return Err("settings_effect_unknown".into()),
    };
    super::connections::encode(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use maka_event_log::root::{RootNamespaces, RootOwner};
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn preference_update_observes_cas_and_preserves_unsaved_auto_and_english() {
        let temp = tempfile::tempdir().unwrap();
        let owner = Arc::new(
            RootOwner::create(
                &temp.path().join("root"),
                &RootNamespaces {
                    ownership: temp.path().join("owners"),
                    control: temp.path().join("control"),
                },
            )
            .unwrap(),
        );
        let store = maka_config::ConfigurationStore::for_root(owner.clone())
            .await
            .unwrap();
        let preferences = read(&store, LanguagePreference::ZhCn, Language::ZhTw, json!({}))
            .await
            .unwrap();
        assert_eq!(preferences["language"], Value::Null);
        assert_eq!(preferences["resolvedLanguage"], "zh-CN");
        assert_eq!(
            update(
                &store,
                json!({"expectedRevision":0,"mutation":{"kind":"language","preference":"auto"}})
            )
            .await
            .unwrap(),
            json!({"kind":"committed","revision":1})
        );
        let preferences = read(&store, LanguagePreference::ZhCn, Language::ZhTw, json!({}))
            .await
            .unwrap();
        assert_eq!(preferences["language"], "auto");
        assert_eq!(preferences["resolvedLanguage"], "zh-TW");
        assert_eq!(
            update(
                &store,
                json!({"expectedRevision":0,"mutation":{"kind":"language","preference":"en"}})
            )
            .await
            .unwrap(),
            json!({"kind":"revision_conflict","expectedRevision":0,"actualRevision":1})
        );
        assert_eq!(
            update(
                &store,
                json!({"expectedRevision":1,"mutation":{"kind":"language","preference":"en"}})
            )
            .await
            .unwrap(),
            json!({"kind":"committed","revision":2})
        );
        store.close().await.unwrap();
        let store = maka_config::ConfigurationStore::for_root(owner)
            .await
            .unwrap();
        let preferences = read(&store, LanguagePreference::ZhCn, Language::ZhTw, json!({}))
            .await
            .unwrap();
        assert_eq!(preferences["revision"], 2);
        assert_eq!(preferences["language"], "en");
        assert_eq!(preferences["resolvedLanguage"], "en");
        assert_eq!(
            update(
                &store,
                json!({"expectedRevision":2,"mutation":{"kind":"language","preference":"zh"}})
            )
            .await
            .unwrap(),
            json!({"kind":"rejected"})
        );
        assert_eq!(store.runtime_policy().await.unwrap().revision, 2);
        store.close().await.unwrap();
    }
}
