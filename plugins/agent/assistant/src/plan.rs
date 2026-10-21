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

//! Plan intent and model-reported progress. Neither is Host execution authority.
mod artifact;
pub mod owner;
pub mod repository;
mod transition;
pub use artifact::{Artifact, Complexity, Step};

use maka_agent_facts::event::Invocation;
use maka_agent_facts::execution::BehaviorId;
use maka_agent_facts::language::Language;
use maka_agent_ports::execution::Receipt;
use maka_agent_ports::execution::Submit;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid Plan input: {0}")]
    Invalid(String),
    #[error("Plan changed or operation identity was reused")]
    Conflict,
    #[error("Plan storage is invalid: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Storage(#[from] maka_capabilities::storage::StoreError),
    #[error(transparent)]
    Execution(#[from] maka_capabilities::authorization::CommandError),
    #[error(transparent)]
    Capability(#[from] maka_capabilities::Error),
    #[error(transparent)]
    Tool(#[from] maka_agent_facts::tools::ToolError),
}

/// An exact operation is replayable even after subsequent state changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request<A = maka_capabilities::authorization::Id> {
    pub operation_id: String,
    pub expected_revision: u64,
    pub command: Command<A>,
}

/// The caller must obtain the appropriate public Remote or invocation capability
/// before invoking these domain operations. Deserializing a command grants none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Command<A = maka_capabilities::authorization::Id> {
    Propose {
        turn_id: String,
        artifact: Artifact,
    },
    Revise {
        proposal_id: String,
    },
    Abandon {
        proposal_id: String,
    },
    Approve {
        #[serde(default, skip_serializing_if = "Language::is_english")]
        language: Language,
        proposal_id: String,
        proposal_revision: u64,
        behavior: BehaviorId,
        grant: A,
    },
    Dispatch {
        execution_id: String,
    },
    Defer {
        execution_id: String,
    },
    Reconcile {
        execution_id: String,
        grant: A,
    },
    Accept {
        execution_id: String,
        receipt: Receipt,
    },
    /// Only a definitive Host rejection; an ambiguous reply leaves admission pending.
    Reject {
        execution_id: String,
        request_digest: String,
        reason: String,
    },
    Progress {
        execution_id: String,
        invocation: Invocation,
        steps: Vec<Progress>,
    },
    Interrupt {
        execution_id: String,
        invocation: Invocation,
        reason: String,
    },
    Resume {
        #[serde(default, skip_serializing_if = "Language::is_english")]
        language: Language,
        execution_id: String,
        grant: A,
    },
    Cancel {
        execution_id: String,
        reason: String,
        grant: Option<A>,
    },
    Settle {
        execution_id: String,
        invocation: Invocation,
        outcome: Settlement,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Settlement {
    Completed,
    Interrupted { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot<A = maka_capabilities::authorization::Id> {
    pub revision: u64,
    pub proposal: Option<Proposal>,
    pub execution: Option<Execution<A>>,
}

impl<A> Default for Snapshot<A> {
    fn default() -> Self {
        Self {
            revision: 0,
            proposal: None,
            execution: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "PlanProposal")]
pub struct Proposal {
    pub id: String,
    pub plan_id: String,
    pub revision: u64,
    pub turn_id: String,
    pub artifact: Artifact,
    pub status: ProposalStatus,
    pub supersedes: Option<String>,
    pub source_execution_id: Option<String>,
    pub submitted_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "PlanProposalStatus")]
pub enum ProposalStatus {
    PendingApproval,
    RevisionRequested,
    Abandoned,
    Approved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Execution<A = maka_capabilities::authorization::Id> {
    pub id: String,
    pub proposal_id: String,
    pub artifact: Artifact,
    pub steps: Vec<Progress>,
    /// Frozen before calling Host. Recover the exact operation, never a new attempt.
    pub request: Submit,
    pub grant: A,
    pub dispatched: bool,
    pub cancellation: Option<String>,
    pub phase: Phase,
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Phase {
    AwaitingAdmission,
    Active {
        receipt: Receipt,
    },
    Interrupted {
        receipt: Option<Receipt>,
        reason: String,
    },
    Completed {
        receipt: Receipt,
    },
    Cancelled {
        receipt: Option<Receipt>,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PlanProgress")]
pub struct Progress {
    pub id: String,
    pub status: StepStatus,
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "PlanStepStatus")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
}

fn invalid(message: impl ToString) -> Error {
    Error::Invalid(message.to_string())
}

fn identifier(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(invalid(
            "identity must contain ASCII letters, digits, '_' or '-'",
        ));
    }
    Ok(())
}

fn text(value: &str) -> Result<(), Error> {
    if value.trim().is_empty() || value.contains('\0') {
        return Err(invalid("text is empty or contains NUL"));
    }
    Ok(())
}
