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

# maka-host-runtime

[简体中文](README.zh-CN.md)

Runs checked service compositions and owns call admission, activation and retirement. Products supply native factories and, when needed, a JavaScript execution adapter. See `application` for the owner and client API; `application/native_context.rs` is the starting point for native services.

A committed source is desired configuration. It becomes effective only after its instances converge. Updates preserve unchanged activations and their captured bindings; replacements wait for accepted calls and native resources to settle. Mutation receipts distinguish business results from cleanup, so a lost reply must be resolved through its original receipt.

Retiring either party of a subscription requests cancellation of its original invocation. The Host still waits for the handler and its native resources to settle before closing the provider. Accepted queries and mutations retain their original completion and cleanup semantics. Subscription and mutation-progress delivery use queue backpressure and consumer demand without a separate item-size quota.

Service inputs, outputs, configuration and original receipts have no separate JSON envelope quota in the Host or JavaScript request bridge. Accepted inputs still reserve shared capacity until their work settles; this does not bound output memory. Schema validation, JavaScript numeric precision and IPC framing requirements apply at their respective boundaries. A receipt that JavaScript cannot represent exactly remains available in its original journal.

Complete source documents, including frozen remote contracts and package references, retain all original bytes through admission, persistence and management. Read them in pages under one source revision; replace large documents through immutable resources. Concurrent Host input and resource custody uses packing windows: a larger call owns the whole window until actual settlement, rather than being rejected solely for its extent.

Persistence comes from the owned EventLog. Stable plugin data belongs to the Root, deployment and instance; an activation epoch identifies execution, not a new data owner.

The built-in `maka.host.logs` provider derives its fixed log source from that same Root owner. Its `HostLogs` export (`maka.host.Logs`) offers bounded raw-byte `read` queries and cancellable `watch` subscriptions. Compositions must bind and explicitly grant these methods; registration grants no access, and requests cannot select another Root or file. See the generated SDK contract for page and cursor fields.

Host runtime logs technical initialization, source adoption, operation persistence and physical cleanup failures with safe categories and original ownership IDs. Arbitrary plugin exceptions, SQL messages and request payloads are omitted. Logs do not change readiness, operation receipts or cleanup custody; resolve uncertain mutations through their original receipts.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
