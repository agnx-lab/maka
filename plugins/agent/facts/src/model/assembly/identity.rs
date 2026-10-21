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

use super::StepBuilder;
use super::{ModelError, invalid};
use crate::model::ModelToolCall;

impl StepBuilder {
    /// Validate the durable step identity before consuming its stream.
    /// Provider identities remain unchanged throughout acceptance.
    pub fn for_step(step_id: &str) -> Result<Self, ModelError> {
        if step_id.is_empty() {
            return Err(invalid("invalid model step identity"));
        }
        Ok(Self::default())
    }

    pub(super) fn validate_call(&self, call: &ModelToolCall) -> Result<(), ModelError> {
        self.validate_tool_identity(&call.id, &call.name)
    }

    pub(super) fn validate_tool_identity(&self, id: &str, name: &str) -> Result<(), ModelError> {
        if id.is_empty() {
            return Err(invalid("tool call identity must be nonempty"));
        }
        if name.is_empty() {
            return Err(invalid("tool name must be nonempty"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelEvent, ModelFinishReason, ModelPart, ModelUsage};
    use serde_json::json;

    fn call(id: String, name: String, provider_executed: bool) -> ModelEvent {
        ModelEvent::ToolCall(ModelToolCall {
            id,
            name,
            provider_executed,
            input: json!({}).into(),
            provider_options: None,
        })
    }

    fn finish() -> ModelEvent {
        ModelEvent::Finished {
            reason: ModelFinishReason::ToolCalls,
            usage: ModelUsage::default(),
            provider_options: None,
        }
    }

    #[test]
    fn complete_step_and_tool_identities_and_names_preserve_original_values() {
        assert!(StepBuilder::for_step("").is_err());
        let mut builder = StepBuilder::for_step(&"step".repeat(1025)).unwrap();
        builder
            .push(call("call".repeat(2049), "echo_name".repeat(40), false))
            .unwrap();
        builder.push(finish()).unwrap();
        let step = builder.finish().unwrap();
        let call = step.tool_calls().next().unwrap();
        assert_eq!(call.id, "call".repeat(2049));
        assert_eq!(call.name, "echo_name".repeat(40));
    }

    #[test]
    fn malformed_call_poisoning_prevents_completed_steps() {
        for provider in [false, true] {
            for (id, name) in [(String::new(), "echo".into()), ("id".into(), String::new())] {
                let mut builder = StepBuilder::for_step("step").unwrap();
                assert!(builder.push(call(id, name, provider)).is_err());
                assert!(builder.push(finish()).is_err());
                assert!(builder.finish().is_err());
            }
        }
    }

    #[test]
    fn boundary_ids_remain_raw_and_provider_results_must_match() {
        let id = format!("{}: /é", "x".repeat(4086));
        assert_eq!(id.len(), 4091);
        let name = "é".repeat(128);
        for provider in [false, true] {
            let mut builder = StepBuilder::for_step("step").unwrap();
            builder
                .push(call(id.clone(), name.clone(), provider))
                .unwrap();
            if provider {
                builder
                    .push(ModelEvent::ProviderToolResult {
                        id: id.clone(),
                        name: name.clone(),
                        output: json!({}),
                        is_error: false,
                        provider_options: None,
                    })
                    .unwrap();
            }
            builder.push(finish()).unwrap();
            let step = builder.finish().unwrap();
            assert_eq!(step.tool_calls().next().unwrap().id, id);
            assert_eq!(step.tool_calls().next().unwrap().name, name);
            if provider {
                assert!(
                    matches!(&step.parts[1], ModelPart::ToolResult { id: result, .. } if result == &id)
                );
            }
        }
        let mut builder = StepBuilder::for_step("step").unwrap();
        builder.push(call(id.clone(), name, true)).unwrap();
        assert!(
            builder
                .push(ModelEvent::ProviderToolResult {
                    id,
                    name: "other".into(),
                    output: json!({}),
                    is_error: false,
                    provider_options: None,
                })
                .is_err()
        );
        assert!(builder.finish().is_err());
    }

    #[test]
    fn input_observations_never_accept_items_or_change_final_argument_authority() {
        let mut builder = StepBuilder::for_step("step").unwrap();
        builder
            .push(ModelEvent::PartStarted {
                id: "text".into(),
                text_kind: crate::model::TextKind::Text,
                provider_options: None,
            })
            .unwrap();
        for event in [
            ModelEvent::ToolInputStarted {
                id: "call".into(),
                name: "echo".into(),
                provider_executed: false,
            },
            ModelEvent::ToolInputDelta {
                id: "call".into(),
                text: "{\"path\":\"src/中".into(),
            },
            ModelEvent::ToolInputFinished { id: "call".into() },
        ] {
            builder.push(event).unwrap();
            assert!(builder.accepted_item().is_none());
            assert_eq!(builder.accepted_parts().count(), 0);
        }
        let complete = ModelToolCall {
            id: "call".into(),
            name: "echo".into(),
            provider_executed: false,
            input: json!({"path":"normalized/中文"}).into(),
            provider_options: Some(json!({"provider":{"signature":"final"}})),
        };
        builder
            .push(ModelEvent::ToolCall(complete.clone()))
            .unwrap();
        assert_eq!(
            builder.accepted_item(),
            Some((1, &ModelPart::ToolCall { call: complete }))
        );
        // Adapters that only supply complete calls retain the same admission.
        builder
            .push(call("direct".into(), "echo".into(), false))
            .unwrap();
        assert_eq!(builder.accepted_item().unwrap().0, 2);
        builder
            .push(ModelEvent::PartFinished {
                id: "text".into(),
                provider_options: None,
            })
            .unwrap();
        assert_eq!(builder.accepted_item().unwrap().0, 0);
        builder.push(finish()).unwrap();
        let step = builder.finish().unwrap();
        assert_eq!(step.parts.len(), 3);
        assert_eq!(
            step.tool_calls()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            ["call", "direct"]
        );
    }

    #[test]
    fn incomplete_or_mismatched_input_identity_cannot_complete_a_step() {
        let start = ModelEvent::ToolInputStarted {
            id: "call".into(),
            name: "echo".into(),
            provider_executed: false,
        };
        let end = ModelEvent::ToolInputFinished { id: "call".into() };
        let delta = ModelEvent::ToolInputDelta {
            id: "call".into(),
            text: "{}".into(),
        };
        for events in [
            vec![delta.clone()],
            vec![end.clone()],
            vec![start.clone(), start.clone()],
            vec![start.clone(), end.clone(), end.clone()],
            vec![start.clone(), end.clone(), delta],
            vec![start.clone(), call("call".into(), "other".into(), false)],
            vec![start.clone(), call("call".into(), "echo".into(), true)],
            vec![start.clone(), finish()],
            vec![start.clone(), end, finish()],
            vec![call("call".into(), "echo".into(), false), start.clone()],
        ] {
            let mut builder = StepBuilder::for_step("step").unwrap();
            let mut rejected = false;
            for event in events {
                if builder.push(event).is_err() {
                    rejected = true;
                    break;
                }
            }
            assert!(rejected);
            assert!(builder.finish().is_err());
        }
        // Final acceptance can close an observed input even if no separate
        // display-only end event was supplied by the adapter.
        let mut builder = StepBuilder::for_step("step").unwrap();
        builder.push(start).unwrap();
        builder
            .push(call("call".into(), "echo".into(), false))
            .unwrap();
        assert_eq!(builder.accepted_item().unwrap().0, 0);
        builder.push(finish()).unwrap();
        assert_eq!(builder.finish().unwrap().parts.len(), 1);
    }
}
