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

use crate::{ProviderKind, native};
use maka_agent_facts::composition::SourceKind;
use maka_agent_facts::composition::SourceRevision;
use maka_agent_ports::model::Binding;
use maka_agent_ports::model::ProviderAdapter;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) fn builtin(owner: SourceRevision) -> BTreeMap<String, Binding> {
    let native = Arc::new(native::Adapter::default());
    let providers: [(&str, Arc<dyn ProviderAdapter>); 4] = [
        ("responses", Arc::new(maka_responses::Adapter::default())),
        ("chat-completions", native.clone()),
        ("anthropic-messages", native),
        (
            maka_providers::codex::ADAPTER,
            Arc::new(maka_providers::codex::Codex::default()),
        ),
    ];
    providers
        .into_iter()
        .map(|(name, provider)| {
            let mut source = owner.clone();
            source.kind = SourceKind::ModelAdapter;
            source.name = name.into();
            (name.into(), Binding::new(provider, source))
        })
        .collect()
}

pub fn name(kind: &ProviderKind) -> &str {
    match kind {
        ProviderKind::OpenaiResponses | ProviderKind::OpenResponses(_) => "responses",
        ProviderKind::Anthropic => "anthropic-messages",
        ProviderKind::OpenaiChatCompletions | ProviderKind::OpenaiCompatible { .. } => {
            "chat-completions"
        }
    }
}
