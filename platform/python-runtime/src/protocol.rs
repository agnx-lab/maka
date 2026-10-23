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

use maka_code_runtime::{CellDiagnostic, ToolMetadata};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Serialize, Deserialize)]
pub(crate) struct Start {
    pub source: String,
    pub names: Vec<String>,
    pub metadata: Vec<ToolMetadata>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Request {
    Call {
        id: u32,
        name: String,
        args: Vec<Value>,
        kwargs: serde_json::Map<String, Value>,
    },
    Sleep {
        id: u32,
        duration: Duration,
        asynchronous: bool,
    },
    Resolve {
        ids: Vec<u32>,
    },
    Complete {
        error: Option<CellDiagnostic>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Reply {
    Return {
        value: Value,
    },
    Error {
        error: CellDiagnostic,
    },
    Pending,
    Resolved {
        results: Vec<(u32, Result<Value, CellDiagnostic>)>,
    },
}
