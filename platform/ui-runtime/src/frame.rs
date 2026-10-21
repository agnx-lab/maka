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

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// One callback's result. Events are finite transaction intents, never retained
/// in a committed Frame or replayed during rendering.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentOutput<V = Value> {
    pub frame: Frame<V>,
    pub events: Vec<Emission>,
    pub actions: Vec<super::actions::Action>,
    /// Request cancellation of this component's original mutation keys.
    /// This does not acknowledge business completion or physical cleanup.
    #[serde(default)]
    pub cancel: Vec<String>,
    /// Complete current subscription demand. Empty removes previous demand.
    #[serde(default)]
    pub subscriptions: Vec<super::actions::Action>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Emission {
    pub name: String,
    pub value: Value,
}
impl<V> ComponentOutput<V> {
    pub fn map_view<T>(self, map: impl FnOnce(V) -> T) -> ComponentOutput<T> {
        ComponentOutput {
            frame: self.frame.map_view(map),
            events: self.events,
            actions: self.actions,
            cancel: self.cancel,
            subscriptions: self.subscriptions,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
// Callback output uses owned JSON by default. The runtime retains immutable
// state snapshots through a shared ownership type without changing the wire ABI.
pub struct Frame<V = Value, S = Value> {
    pub revision: u64,
    pub state: S,
    pub surface: Option<Surface>,
    pub view: V,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Surface {
    Terminal { binding: String },
}
impl Surface {
    pub fn binding(&self) -> &str {
        match self {
            Self::Terminal { binding } => binding,
        }
    }
    pub fn target(&self) -> maka_plugin_core::composition::PresentationTarget {
        use maka_plugin_core::composition::PresentationTarget;
        match self {
            Self::Terminal { .. } => PresentationTarget::Tui,
        }
    }
    fn validate(&self) -> bool {
        !self.binding().is_empty()
    }
}
/// One child of a committed component, owned by a committed native presentation.
/// Binding resolution belongs to the parent activation and the common mount kernel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentSlot {
    pub key: String,
    pub binding: String,
    pub member: u32,
    pub props: Value,
    pub on: BTreeMap<String, String>,
}
impl ComponentSlot {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.key.is_empty()
            || self.binding.is_empty()
            || self
                .on
                .iter()
                .any(|(event, handler)| event.is_empty() || handler.is_empty())
        {
            return Err("invalid component slot identity or binding".into());
        }
        Ok(())
    }
    pub(crate) fn reference(&self) -> ComponentRef<'_> {
        ComponentRef {
            key: &self.key,
            binding: &self.binding,
            member: self.member,
            props: &self.props,
            on: &self.on,
        }
    }
}
impl<V> Frame<V> {
    pub fn output(self, events: Vec<Emission>) -> ComponentOutput<V> {
        ComponentOutput {
            frame: self,
            events,
            actions: Vec::new(),
            cancel: Vec::new(),
            subscriptions: Vec::new(),
        }
    }
}
impl<V, S> Frame<V, S> {
    pub fn validate_metadata(&self) -> Result<(), String> {
        if self.revision > 9_007_199_254_740_991
            || self
                .surface
                .as_ref()
                .is_some_and(|surface| !surface.validate())
        {
            return Err("invalid surface/revision".into());
        }
        Ok(())
    }
    pub fn map_view<T>(self, map: impl FnOnce(V) -> T) -> Frame<T, S> {
        Frame {
            revision: self.revision,
            state: self.state,
            surface: self.surface,
            view: map(self.view),
        }
    }
    pub(crate) fn map_state<T>(self, map: impl FnOnce(S) -> T) -> Frame<V, T> {
        Frame {
            revision: self.revision,
            state: map(self.state),
            surface: self.surface,
            view: self.view,
        }
    }
}
pub(crate) struct ComponentRef<'a> {
    pub key: &'a str,
    pub binding: &'a str,
    pub member: u32,
    pub props: &'a Value,
    pub on: &'a BTreeMap<String, String>,
}
