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

# maka-ui-js

[简体中文](README.zh-CN.md)

Executes JavaScript component callbacks for `maka-ui-runtime`. `execution` configures one sleeping background worker with bounded admission. Activation, mount, update, input, capture and close all use asynchronous jobs. Reducers remain bounded synchronous computations; they propose candidates from the original committed state and do not await I/O.

The Rust UI owner validates and commits each resulting batch. A late result cannot revive a retired mount. Native input resources, exact mutation receipts and accepted execution cleanup stay with their original owners; moving execution to the worker does not acknowledge their completion.

Invocation jobs retain shared immutable state and props instead of copying their JSON trees on the UI owner. Argument validation and encoding run on the worker and read those snapshots directly. JavaScript still receives independent JSON values, so guest mutations cannot modify the retained Rust snapshots.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
