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

# maka-jev

[简体中文](README.zh-CN.md)

Implements structured Jev evaluations and endpoint settings. `decision` validates questions and responses; `settings` owns configuration and endpoint-bound credentials. Calls use scoped HTTP resources. Returned probabilities are provider answers, and a timeout can leave the remote outcome unknown.

Evaluations, complete responses and endpoint-bound credentials have no additional local count or byte ceilings. The caller’s positive configured timeout remains an explicit execution deadline; cancellation, network failure and unconfirmed settlement preserve uncertain remote outcomes. Questions and answers retain exact original IDs, declared choice/score structure, probability checks and JavaScript-exact usage counters. The engine buffers complete JSON and does not promise bounded memory or vendor acceptance of every locally valid evaluation.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
