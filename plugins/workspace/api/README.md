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

# maka-workspace-api

[简体中文](README.zh-CN.md)

Defines the shared contracts for workspace files, processes, terminals, protocol processes and Jev. Native providers and authoring tools use the same declarations. Request paths and workspace identifiers describe a selection; the provider still checks the actual bound directory and call authority.

`Files.readText` reads workspace text and Session attachment or archived tool-result resources under the caller's original invocation authority. It accepts the complete returned `next` object for continuation; an address grants no access to another Session. Agent tool descriptions expose this capability under the selected tool's name. `readImage` does not read tool-result archives.

The Terminal contract exposes `readOutput` and `watchOutput` for complete ordered original PTY bytes (Base64) and native resize events retained by the original live resource. Each data event contains up to 16 KiB; there is no tail eviction. Keep `after` with its original target. A fresh parser subscribes directly with `watchOutput(after: 0)`, resets once to the first retained event’s `initialSize`, then consumes every Data/Resize in order. An intact parser can resume after its known sequence. Existing readers can drain after worker retirement, but a new attachment after retirement or Host restart reports `Unavailable`; a prior `readOutput` query does not reserve a later attachment. Real replay I/O failures fail the call. Plain screen reads are not full parser checkpoints. `Closed` means byte-source closure, not confirmed process cleanup. Output observation needs explicit method grants, not a controller.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
