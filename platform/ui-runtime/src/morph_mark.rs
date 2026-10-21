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

//! Decorative morphing strokes sampled on a bounded native grid.
use crate::scene::terminal::Color;
use maka_plugin_core::contract::{Contract, Interface, Transport};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MorphShape {
    #[schemars(length(min = 1))]
    pub paths: Vec<Vec<[f32; 2]>>,
    #[schemars(range(min = 0.01, max = 0.2))]
    pub radius: f32,
    pub miters: Vec<usize>,
    #[schemars(range(min = 32, max = 256))]
    pub columns: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MorphMarkConfiguration {
    pub shapes: [MorphShape; 2],
    pub color: Color,
    #[schemars(range(min = 1000, max = 60000))]
    pub period: u32,
}
impl MorphMarkConfiguration {
    pub fn validate(&self) -> crate::Result<()> {
        if !self.color.valid()
            || !(1000..=60000).contains(&self.period)
            || self.shapes.iter().any(|s| {
                s.paths.is_empty()
                    || !(0.01..=0.2).contains(&s.radius)
                    || !(32..=256).contains(&s.columns)
                    || s.paths.iter().any(|p| {
                        p.len() < 2
                            || p.iter().flatten().any(|n| !n.is_finite() || n.abs() > 2.0)
                            || p.windows(2).any(|pair| pair[0] == pair[1])
                    })
                    || s.miters
                        .iter()
                        .any(|i| s.paths.get(*i).is_none_or(|p| p.len() != 3))
            })
        {
            return Err(crate::Error::Invalid(
                "invalid morphing mark geometry".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MorphMarkProps {
    pub background: Color,
    pub foreground: Color,
    pub motion: bool,
    pub ascii: bool,
}
pub fn contract() -> Contract {
    Contract {
        name: "maka.ui.MorphMark".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: serde_json::to_value(schemars::schema_for!(MorphMarkProps)).unwrap(),
            events: BTreeMap::new(),
            captures: BTreeMap::new(),
        },
    }
}

pub const IMPLEMENTATION: &str = "maka.ui.morph-mark";

pub fn native() -> maka_plugin_core::package::NativeDefinition {
    maka_plugin_core::package::NativeDefinition::new(
        IMPLEMENTATION,
        schemars::schema_for!(MorphMarkConfiguration).into(),
        [("MorphMark", contract())],
    )
}
