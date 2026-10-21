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

# maka-agent-facts

[简体中文](README.zh-CN.md)

Defines the shared Agent, Session and Run facts used by persistence and execution: events, messages, model selections, tool calls, accounting and continuation records. These are data types, without an engine or SQLite dependency. Keep execution policy in the owning service rather than adding it to a persisted fact.

Malformed tool arguments are preserved and returned to the model for correction without executing the tool. Cancellation and recovery retain the same evidence.

Histories and bundles written with the earlier tool-input format cannot be replayed. Initialize a separate empty Root with `maka host init --root NEW_EMPTY_DIRECTORY`; existing data is not automatically migrated or deleted.

Successful tool results retain complete raw evidence and ordered frozen projection parts. Exact size/digest, confirmed operation receipt and Session media ownership remain required. Raw typed JSON must round-trip through the dependency's supported decoder representation; archive canonical encoding walks JSON iteratively and preserves its established key/number encoding. Model token budgets select a request view separately from retained evidence.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
