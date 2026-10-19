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

# maka-session-import

[简体中文](README.zh-CN.md)

Discovers and converts external conversation formats from Claude, Codex and OpenCode. `Transcript` is the normalized import representation; `source` and `catalog` handle discovery. The native Agent service validates the selected source and publishes the complete transcript and new Session in one transaction. Retries verify the original canonical records. Adapters give reused foreign tool IDs distinct normalized occurrence keys; a new imported Session rejects duplicate ToolCall keys. It does not execute imported instructions or replace the Host's canonical event writer.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
