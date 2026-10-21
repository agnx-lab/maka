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

//! Actual immutable request payloads, never JSON bearer handles.

use maka_plugin_core::contract::ResourceType;
use serde_json::Value;
use std::{collections::BTreeMap, ops::Range, sync::Arc};

/// Byte interpretation preserved by immutable transport snapshots.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ResourceEncoding {
    #[default]
    Bytes,
    Utf8,
}

/// Trusted immutable payload. Its owner must not retain a native UI entity,
/// live input, mount lease or a writable buffer. Descriptors remain immutable.
/// Reads use in-bounds byte ranges; text implementations check UTF-8 boundaries.
/// Transport adapters apply their own framing limits.
pub trait ReadResource: Send + Sync + 'static {
    fn encoding(&self) -> ResourceEncoding {
        ResourceEncoding::Bytes
    }
    fn resource_type(&self) -> ResourceType;
    fn bytes(&self) -> usize;
    fn metadata(&self) -> Value;
    fn read(&self, range: Range<usize>) -> std::result::Result<Vec<u8>, String>;
}
/// Actual request-local payloads. Slot names and metadata cannot construct them;
/// the execution adapter owns capacity and retains these values through cleanup.
pub type Attachments = BTreeMap<String, Arc<dyn ReadResource>>;
