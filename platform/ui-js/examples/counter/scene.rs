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

use maka_ui_runtime::{ComponentSlot, NativeView};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Row,
    Column,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(rename = "UiNode")]
pub enum Node {
    Stack {
        key: String,
        direction: Direction,
        gap: u16,
        padding: u16,
        background: u32,
        children: Vec<Node>,
    },
    Text {
        key: String,
        text: String,
        color: u32,
    },
    Button {
        key: String,
        label: String,
    },
    Component(ComponentSlot),
}
impl Node {
    pub(crate) fn validate_root(&self) -> Result<(), String> {
        self.validate(0, &mut BTreeSet::new())
    }
    fn validate(&self, depth: usize, keys: &mut BTreeSet<String>) -> Result<(), String> {
        let key = match self {
            Self::Stack { key, .. } | Self::Text { key, .. } | Self::Button { key, .. } => key,
            Self::Component(slot) => &slot.key,
        };
        if depth > 32
            || keys.len() >= 4096
            || key.is_empty()
            || key.len() > 128
            || !keys.insert(key.clone())
        {
            return Err("invalid or duplicate UI node identity".into());
        }
        match self {
            Self::Stack {
                gap,
                padding,
                background,
                children,
                ..
            } => {
                if *gap > 1024 || *padding > 1024 || *background > 0xffffff {
                    return Err("invalid UI style".into());
                }
                for child in children {
                    child.validate(depth + 1, keys)?;
                }
            }
            Self::Text { text, color, .. } => {
                if text.len() > 64 * 1024 || *color > 0xffffff {
                    return Err("invalid text".into());
                }
            }
            Self::Button { label, .. } => {
                if label.len() > 4096 {
                    return Err("invalid button label".into());
                }
            }
            Self::Component(slot) => {
                if slot.binding.is_empty() || slot.binding.len() > 128 {
                    return Err("invalid slot binding".into());
                }
            }
        }
        Ok(())
    }
    fn metadata(
        &self,
        slots: &mut Vec<ComponentSlot>,
        inputs: &mut BTreeMap<String, Value>,
    ) -> Result<(), String> {
        match self {
            Self::Component(slot) => {
                slots.push(slot.clone());
            }
            Self::Button { key, .. } => {
                inputs.insert(key.clone(), json!(true));
            }
            Self::Stack { children, .. } => {
                for child in children {
                    child.metadata(slots, inputs)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub(crate) fn compile<V: Default + 'static>(
    source: &Value,
) -> maka_ui_runtime::Result<NativeView<V>> {
    prepare(source)?.finish::<Node, V>(|_| Rc::new(V::default()))
}
pub(crate) fn finish<V: Default + 'static>(
    prepared: maka_ui_runtime::PreparedScene,
) -> maka_ui_runtime::Result<NativeView<V>> {
    prepared.finish::<Node, V>(|_| Rc::new(V::default()))
}
pub(crate) fn prepare(source: &Value) -> maka_ui_runtime::Result<maka_ui_runtime::PreparedScene> {
    fn scan(source: &Value, depth: usize) -> maka_ui_runtime::Result<()> {
        if depth > 32 {
            return Err(maka_ui_runtime::Error::Invalid(
                "scene source depth limit".into(),
            ));
        }
        if let Some(children) = source.get("children").and_then(Value::as_array) {
            for child in children {
                scan(child, depth + 1)?;
            }
        }
        Ok(())
    }
    scan(source, 0)?;
    let node: Node = serde_json::from_value(source.clone())
        .map_err(|e| maka_ui_runtime::Error::Invalid(e.to_string()))?;
    node.validate_root()
        .map_err(maka_ui_runtime::Error::Invalid)?;
    let mut slots = vec![];
    let mut inputs = BTreeMap::new();
    node.metadata(&mut slots, &mut inputs)
        .map_err(maka_ui_runtime::Error::Invalid)?;
    maka_ui_runtime::PreparedScene::new(node, inputs, slots)
}
pub(crate) fn view<V: Default + 'static>(node: Node) -> maka_ui_runtime::Result<NativeView<V>> {
    compile(&serde_json::to_value(node).unwrap())
}
pub(crate) fn registry<V: Default + 'static>(
    target: maka_plugin_core::composition::PresentationTarget,
) -> maka_ui_runtime::NativeRegistry<V> {
    maka_ui_runtime::NativeRegistry::with_surface(target)
        .with_scene_compiler(language(target), prepare, |prepared| {
            prepared.finish::<Node, V>(|_| Rc::new(V::default()))
        })
        .expect("valid fixture scene language")
}

// This fixture compiles the terminal subset used by lifecycle tests.
pub(crate) fn language(
    target: maka_plugin_core::composition::PresentationTarget,
) -> maka_plugin_core::package::SceneLanguage {
    maka_plugin_core::package::SceneLanguage {
        name: match target {
            maka_plugin_core::composition::PresentationTarget::Tui => "maka.tui.scene",
        }
        .into(),
        version: 1,
    }
}
