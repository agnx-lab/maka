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

use super::{Error, Progress, StepStatus, identifier, invalid, text};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PlanArtifact")]
pub struct Artifact {
    pub title: String,
    pub overview: Option<String>,
    #[schemars(length(min = 1))]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub risks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PlanStep")]
pub struct Step {
    #[schemars(length(min = 1), regex(pattern = "^[A-Za-z0-9_-]+$"))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub files: Vec<String>,
    pub complexity: Option<Complexity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "PlanComplexity")]
pub enum Complexity {
    Low,
    Medium,
    High,
}

impl Artifact {
    pub fn validate(&self) -> Result<(), Error> {
        text(&self.title)?;
        if let Some(overview) = &self.overview {
            text(overview)?;
        }
        if self.steps.is_empty() {
            return Err(invalid("Plan requires at least one step"));
        }
        let mut ids = BTreeSet::new();
        for step in &self.steps {
            identifier(&step.id)?;
            text(&step.title)?;
            text(&step.description)?;
            if !ids.insert(&step.id) {
                return Err(invalid("duplicate Plan step id"));
            }
            for file in &step.files {
                text(file)?;
            }
        }
        for risk in &self.risks {
            text(risk)?;
        }
        Ok(())
    }

    pub(super) fn progress(&self, steps: &[Progress]) -> Result<(), Error> {
        let expected: BTreeSet<_> = self.steps.iter().map(|s| &s.id).collect();
        let actual: BTreeSet<_> = steps.iter().map(|s| &s.id).collect();
        if steps.len() != self.steps.len()
            || expected != actual
            || steps
                .iter()
                .filter(|s| s.status == StepStatus::InProgress)
                .count()
                > 1
        {
            return Err(invalid(
                "include every Plan step exactly once, at most one in progress",
            ));
        }
        for step in steps {
            if let Some(note) = &step.note {
                text(note)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Command, Phase, Snapshot};

    #[test]
    fn large_artifacts_survive_request_freezing_and_reject_ambiguous_progress() {
        let mut artifact = Artifact {
            title: "界".repeat(30),
            overview: None,
            steps: (0..64)
                .map(|i| Step {
                    id: format!("step_{i}"),
                    title: "界".repeat(40),
                    description: "\"\\\n".repeat(600),
                    files: vec![],
                    complexity: None,
                })
                .collect(),
            risks: vec!["Check dependencies".into(); 25],
        };
        artifact.steps[0].files = (0..64).map(|i| format!("src/file_{i}.rs")).collect();
        let mut snapshot = Snapshot::default();
        snapshot
            .apply(
                &Command::Propose {
                    turn_id: "turn".into(),
                    artifact: artifact.clone(),
                },
                "propose",
                "session",
                1,
            )
            .unwrap();
        snapshot
            .apply(
                &Command::Approve {
                    language: maka_agent_facts::language::Language::En,
                    grant: maka_capabilities::authorization::Id(uuid::Uuid::from_u128(1)),
                    proposal_id: snapshot.proposal.as_ref().unwrap().id.clone(),
                    proposal_revision: 1,
                    behavior: Default::default(),
                },
                "approve",
                "session",
                2,
            )
            .unwrap();
        let execution = snapshot.execution.as_ref().unwrap();
        assert!(matches!(execution.phase, Phase::AwaitingAdmission));
        execution.request.validate().unwrap();
        let mut progress = execution.steps.clone();
        progress[1].id = progress[0].id.clone();
        assert!(artifact.progress(&progress).is_err());
        progress = execution.steps.clone();
        progress[0].status = StepStatus::InProgress;
        progress[1].status = StepStatus::InProgress;
        assert!(artifact.progress(&progress).is_err());
    }
}
