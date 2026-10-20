<!--
  Licensed to the Apache Software Foundation (ASF) under one
  or more contributor license agreements.  See the NOTICE file
  distributed with this work for additional information
  regarding copyright ownership.  The ASF licenses this file
  to you under the Apache License, Version 2.0 (the
  "License"); you may not use this file except in compliance
  with the License.  You may obtain a copy of the License at

      http://www.apache.org/licenses/LICENSE-2.0

  Unless required by applicable law or agreed to in writing,
  software distributed under the License is distributed on an
  "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
  KIND, either express or implied.  See the License for the
  specific language governing permissions and limitations
  under the License.
-->

# maka-js-runtime

[简体中文](README.zh-CN.md)

Provides the V8 execution machinery shared by plugin guests and persistent JavaScript REPLs. `plugin` implements bounded guest execution; `repl` and `CodeExecutor` support cells that retain variables between calls. Embeddings supply callable capabilities and own their resources. A JavaScript promise finishing does not prove that the native work it started has settled.

Nested tool calls pass complete JSON inputs and results through the embedding's original executor. A cell can consume a large tool result and publish a short summary; cell result, emitted output and stored-value budgets apply at their own publication boundaries. Admitted tool work remains tracked until cleanup, including when JavaScript stops observing its promise.

The trusted provider SDK isolate uses V8’s default heap policy and queues requests with byte reservations held through native cleanup. Heap-limit notifications mark the shared runtime failed; native out-of-memory failures can still terminate the process. Generated code and plugin guests keep their separately configured limits.

Model SDKs submit shaped JSON through the existing request-scoped HTTP capability. Rust encodes the body into owned bytes, so an aggregate request need not fit one JavaScript string. This bridge accepts the SDK’s JSON objects, not arbitrary JavaScript values or general Fetch bodies. It still buffers the complete body; cancellation is checked during encoding, while synchronous value conversion and individual writes are not preemptible.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
