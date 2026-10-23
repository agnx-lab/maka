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

use maka_agent_facts::{execution::CodeLanguage, tools::ToolExecutor};
use maka_code_runtime::{CellAbort, CellContext, CellLimits, CellResult};
use std::{path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

/// Code Mode policy selects one concrete interpreter for each accepted Run.
/// Construction is cheap: neither backend starts a VM or child process here.
#[derive(Clone)]
pub struct CodeExecutor {
    javascript: maka_js_runtime::CodeExecutor,
    python: maka_python_runtime::CodeExecutor,
    language: CodeLanguage,
}
impl CodeExecutor {
    pub fn new(limits: CellLimits) -> Result<Self, CellAbort> {
        Ok(Self {
            javascript: maka_js_runtime::CodeExecutor::new(limits.clone())?,
            python: maka_python_runtime::CodeExecutor::new(limits)?,
            language: CodeLanguage::default(),
        })
    }
    pub fn with_language(mut self, language: CodeLanguage) -> Self {
        self.language = language;
        self
    }
    /// Explicit packaged worker location for an embedding or integration fixture.
    pub fn with_python_worker(mut self, path: PathBuf) -> Result<Self, CellAbort> {
        self.python = maka_python_runtime::CodeExecutor::with_worker(self.limits().clone(), path)?;
        Ok(self)
    }
    pub fn language(&self) -> CodeLanguage {
        self.language
    }
    pub fn limits(&self) -> &CellLimits {
        self.javascript.limits()
    }
    pub fn tool_identifier(&self, name: &str) -> String {
        match self.language {
            CodeLanguage::JavaScript => maka_js_runtime::tool_identifier(name),
            CodeLanguage::Python => maka_python_runtime::tool_identifier(name),
        }
    }
    pub async fn execute_module(
        &self,
        source: String,
        tools: Arc<dyn ToolExecutor>,
        cancellation: CancellationToken,
        context: CellContext,
    ) -> Result<CellResult, CellAbort> {
        match self.language {
            CodeLanguage::JavaScript => {
                self.javascript
                    .execute_module(source, tools, cancellation, context)
                    .await
            }
            CodeLanguage::Python => {
                self.python
                    .execute_module(source, tools, cancellation, context)
                    .await
            }
        }
    }
}
