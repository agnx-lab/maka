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

# maka-processes-native

[简体中文](README.zh-CN.md)

Provides workspace command, terminal and protocol-process services. `protocol` handles structured subprocess communication; `terminal` manages interactive terminal resources. Configuration supplies the installation ceiling, and each call still needs its own authority. Closing an observer does not terminate ownership of a running process.

Live terminal screens and background-process logs come from the original worker handle. Output alone does not update the durable resource revision, `updatedAt`, or `watchList`; start, state, actual resize and final output still commit durably. Detail watches accept new output at the same durable revision. After a Host crash, orphan recovery retains the last durable snapshot, which may predate the last live output.

`health` (the default `BackgroundTaskHealth` model tool) reads the original session-owned process, including live logs when retained. `updatedAt` denotes the last durable resource update, not output activity or endpoint health. Logs require `include_logs: true`. An optional HTTP(S) URL is probed by the bound Web service under separate network permission; the process result and endpoint observation are independent. A responding endpoint proves neither listener ownership nor browser readiness. Health calls and their nested probes retain original operation receipts.

The Terminal [raw output contract](../../api/README.md) uses the original PTY owner and complete replay in two resource-owned anonymous temporary files. Temporary storage grows with output and is released when the original resource and all existing readers are dropped. Closing an output observer leaves the process and controller unchanged; existing sequenced control and mutation receipts retain their original authority.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
