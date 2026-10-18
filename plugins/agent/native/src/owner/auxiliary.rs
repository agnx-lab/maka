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

//! Auxiliary requests retain their physical worker and original usage journal.
use super::*;
use maka_agent_facts::{
    model::{
        ModelEvent, ModelFinishReason, ModelGeneration, ModelPart, TextKind, error::ModelError,
    },
    tools::ToolError,
};
use maka_capabilities::call::Resources;
use maka_event_log::usage::{AuxiliarySource, Outcome};
use maka_model::{ModelRequest, ModelStream, StepBuilder};
use tracing::Instrument;

pub(super) enum Error {
    Model(ModelError),
    Admission(ToolError),
    Tool(ToolError),
}
impl From<ToolError> for Error {
    fn from(error: ToolError) -> Self {
        Self::Tool(error)
    }
}
impl From<Error> for ToolError {
    fn from(error: Error) -> Self {
        match error {
            Error::Model(error) => failed(error),
            Error::Admission(error) | Error::Tool(error) => error,
        }
    }
}

pub(super) enum Admission {
    Source(AuxiliarySource),
    Operation {
        reference: Box<maka_plugin_core::operation::RecoveryRef>,
        binding: ModelBinding,
        session_id: String,
    },
}
pub(super) struct Auxiliary {
    models: ModelExecutor,
    pub resources: Arc<Resources>,
}
impl Auxiliary {
    pub fn new(revision: SourceRevision) -> Result<Self, String> {
        Ok(Self {
            models: ModelExecutor::for_source(Duration::from_secs(30), revision)
                .map_err(|_| "agent_auxiliary_unavailable")?,
            resources: Arc::default(),
        })
    }
    pub async fn generate(
        &self,
        request: ModelRequest,
        cancellation: CancellationToken,
        log: &EventLog,
        admission: Admission,
        quote: maka_agent_facts::pricing::Quote,
    ) -> Result<ModelGeneration, Error> {
        if cancellation.is_cancelled() {
            return Err(Error::Model(ModelError::Cancelled));
        }
        let id = match admission {
            Admission::Operation {
                reference,
                binding,
                session_id,
            } => {
                let span = tracing::error_span!("agent_auxiliary_operation", session_id = %session_id, operation_id = %reference.operation);
                async {
                    log.begin_auxiliary_operation_model(
                        *reference,
                        binding,
                        session_id,
                        request.language,
                        Some(quote),
                    )
                    .await
                    .map_err(|error| match error {
                        maka_event_log::StoreError::SessionNotFound
                        | maka_event_log::StoreError::SessionRetired
                        | maka_event_log::StoreError::SessionBusy => {
                            Error::Admission(failed(error))
                        }
                        other => {
                            tracing::error!(
                                stage = "auxiliary_model_admission_failed",
                                cause = crate::diagnostics::store_cause(&other),
                                sql_code = crate::diagnostics::store_sql_code(&other),
                                "Auxiliary model admission failed"
                            );
                            Error::Tool(persistence(other))
                        }
                    })
                }
                .instrument(span)
                .await?
            }
            Admission::Source(source) => log
                .begin_auxiliary_model(source, request.language, Some(quote))
                .await
                .map_err(|error| match error {
                    maka_event_log::StoreError::InvalidTransition(_)
                    | maka_event_log::StoreError::SessionNotFound
                    | maka_event_log::StoreError::SessionRetired
                    | maka_event_log::StoreError::SessionBusy
                    | maka_event_log::StoreError::PrefixTooLarge => Error::Admission(failed(error)),
                    _ => {
                        tracing::error!(
                            stage = "auxiliary_model_admission_failed",
                            cause = crate::diagnostics::store_cause(&error),
                            sql_code = crate::diagnostics::store_sql_code(&error),
                            "Auxiliary model admission failed"
                        );
                        Error::Tool(persistence(error))
                    }
                })?,
        };
        let model_id = request.provider.model.clone();
        let result = async {
            let stream = self
                .models
                .stream(request, cancellation.clone())
                .await
                .map_err(Error::Model)?;
            let step = receive(stream, log, id).await?;
            if step.finish_reason == ModelFinishReason::ToolCalls {
                return Err(failed("Auxiliary generation cannot finish with tool calls").into());
            }
            Ok(ModelGeneration {
                text: step
                    .parts
                    .into_iter()
                    .filter_map(|part| match part {
                        ModelPart::Text {
                            text_kind: TextKind::Text,
                            text,
                            ..
                        } => Some(text),
                        _ => None,
                    })
                    .collect(),
                model_id,
                finish_reason: step.finish_reason,
                usage: step.usage,
            })
        }
        .await;
        // Neither an uncertain commit nor lost physical custody permits a success/failure replacement.
        if matches!(
            result,
            Err(Error::Tool(
                ToolError::Persistence(_) | ToolError::CleanupUnconfirmed(_)
            ))
        ) {
            return result;
        }
        let outcome = match &result {
            Ok(_) => Outcome::Success,
            Err(_) if cancellation.is_cancelled() => Outcome::Aborted,
            Err(_) => Outcome::Error,
        };
        log.settle_auxiliary_model(id, outcome)
            .await
            .map_err(|error| {
                tracing::error!(stage = "auxiliary_model_settlement_failed", cause = crate::diagnostics::store_cause(&error),
                    sql_code = crate::diagnostics::store_sql_code(&error), request_id = %id, "Auxiliary model settlement failed");
                persistence(error)
            })?;
        result
    }
}
async fn receive(
    mut stream: ModelStream,
    log: &EventLog,
    id: Uuid,
) -> Result<maka_agent_facts::model::ModelStep, Error> {
    let result = async {
        let mut builder = StepBuilder::default();
        let mut bytes = 0usize;
        let mut observed_usage = false;
        while let Some(event) = stream.next().await {
            let maka_model::Emission {
                event,
                reservation: _reservation,
            } = event.map_err(Error::Model)?;
            if let ModelEvent::Finished { usage, .. } = &event
                && !observed_usage
            {
                log.observe_auxiliary_model(id, usage.clone())
                    .await
                    .map_err(|error| {
                        tracing::error!(stage = "auxiliary_model_observation_failed", cause = crate::diagnostics::store_cause(&error),
                            sql_code = crate::diagnostics::store_sql_code(&error), request_id = %id, "Auxiliary model usage observation failed");
                        persistence(error)
                    })?;
                observed_usage = true;
            }
            bytes = bytes.saturating_add(
                maka_agent_facts::model::budget::reservation(&event, 2 * 1024 * 1024 + 1)
                    .map_err(failed)? as usize,
            );
            if bytes > 2 * 1024 * 1024 {
                return Err(failed("Auxiliary generation exceeds 2 MiB stream limit").into());
            }
            if matches!(
                event,
                ModelEvent::ToolCall(_) | ModelEvent::ProviderToolResult { .. }
            ) {
                return Err(failed("Auxiliary generation cannot call tools").into());
            }
            builder.push(event).map_err(Error::Model)?;
        }
        builder.finish().map_err(Error::Model)
    }
    .await;
    stream
        .cancel_and_wait()
        .await
        .map_err(|error| {
            tracing::error!(stage = "auxiliary_model_cleanup_unconfirmed", cause = "cleanup_unconfirmed",
                request_id = %id, "Auxiliary model stream cleanup is unconfirmed");
            ToolError::CleanupUnconfirmed(error.to_string())
        })?;
    result
}
pub(super) fn persistence(error: impl std::fmt::Display) -> ToolError {
    ToolError::Persistence(error.to_string())
}
pub(super) fn failed(error: impl std::fmt::Display) -> ToolError {
    ToolError::Failed(error.to_string())
}
