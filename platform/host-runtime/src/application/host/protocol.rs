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

use super::super::Result;
use super::super::transport::{Catalogue, HostEndpoint, NativeGrant};
use maka_plugin_core::{composition::InstanceId, deployment::DeploymentRef};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Persisted trusted-local Host publication and native invocation authority.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Configuration {
    PublicHost {
        endpoints: BTreeMap<String, HostEndpoint>,
        native: Vec<NativeGrant>,
    },
}
impl Configuration {
    pub fn validate(&self) -> Result<()> {
        let Self::PublicHost { endpoints, native } = self;
        if endpoints.is_empty() || endpoints.keys().any(|name| name.is_empty()) {
            return Err("invalid public Host endpoint configuration".into());
        }
        for endpoint in endpoints.values() {
            endpoint.validate()?;
        }
        for grant in native {
            grant.validate()?;
        }
        Ok(())
    }
}

pub const VERSION: u32 = 1;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Opening {
    pub version: u32,
    pub epoch: Option<Uuid>,
    #[serde(default)]
    pub client_instance: Option<String>,
    pub request: Request,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Connect,
    /// Trusted current-account launcher approval; remote principals cannot use it.
    ConnectPeer {
        source: DeploymentRef,
        consumers: BTreeSet<InstanceId>,
        endpoints: BTreeMap<String, HostEndpoint>,
    },
    Call {
        session: Uuid,
    },
    Status,
    Quit {
        session: Uuid,
    },
    /// Current-account owner only. Source/packages remain a separate CAS operation.
    Configure {
        source: DeploymentRef,
        previous: Configuration,
        configuration: Configuration,
    },
    Retire {
        #[serde(default)]
        interrupt: bool,
        #[serde(default)]
        allow_idle_connections: bool,
    },
    PrepareAccess {
        principal: String,
    },
    RevokeAccess {
        credential: String,
    },
    FinalizeAccess,
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Response {
    Connected(Connected),
    Accepted,
    Epochs(Result<BTreeMap<String, Option<Uuid>>>),
    Captured(Result<Catalogue>),
    Approved {
        source: DeploymentRef,
        result: Result<()>,
    },
    Status(Status),
    Configured(Result<()>),
    Retired(Result<bool>),
    Closed,
    Pairing(Result<super::Pairing>),
    Revoked(Result<bool>),
    Finalized(Result<()>),
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionControl {
    Close,
    Epochs,
    Capture,
    Approve {
        source: DeploymentRef,
        consumers: BTreeSet<InstanceId>,
        retain: BTreeSet<u64>,
        captured: Option<Catalogue>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connected {
    pub version: u32,
    pub epoch: Uuid,
    pub session: Uuid,
    pub source: DeploymentRef,
    pub consumers: BTreeMap<String, InstanceId>,
    pub catalogue: Catalogue,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub root_id: String,
    pub version: u32,
    pub pid: std::num::NonZeroU32,
    pub generation: Option<String>,
    pub lifecycle_mode: LifecycleMode,
    pub epoch: Uuid,
    pub source: DeploymentRef,
    pub closing: bool,
    pub closed: bool,
    pub cleanup_error: Option<String>,
    pub sessions: usize,
    /// Observation only; idle retirement also fences native background producers.
    pub active_calls: usize,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleMode {
    Service,
    Ephemeral,
}
