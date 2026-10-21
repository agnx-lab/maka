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

# maka-config

[简体中文](README.zh-CN.md)

Persists the Root's connection catalog, credentials, model catalog and preferences. `ConfigurationStore` uses an existing owned Root. Connection observations and credential revisions are separate, so clients can inspect configuration without receiving secrets. These records control execution and do not belong in model conversation history.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)

Connection, model and credential records preserve complete supported values without application byte or item quotas. Physical admission and operation settlement belong to the Host. Queries return continuation slices; a complete first item may exceed the page packing target. Revisions, exact identities, canonical structure and durable OAuth completion receipts remain authoritative.
