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

# maka-python-runtime

[简体中文](README.zh-CN.md)

Optional Python Code Mode backend, implemented with pinned Monty 1.3.0 Rust crates. No system Python, pip packages or native extensions are needed. Enable Code Mode and select Python in connection model settings (`codeModeLanguage: "python"`). JavaScript remains the default.

Each cell starts a fresh `maka-python-worker` process beside the Host executable. Build it with `cargo build -p maka-python-runtime`; the development runner and native release packages include it automatically. Embeddings can supply an explicit worker path with `CodeExecutor::with_worker`.

The worker supports top-level await and JSON tool calls through `await tools.name(input)` or keyword arguments. `asyncio.gather` supports concurrent tools; catch errors inside each coroutine for partial results. The interpreter does not provide the full CPython standard library, `asyncio.create_task`, `TaskGroup`, or `gather(return_exceptions=True)`. Filesystem, network and process operations go through authorized Host tools.

The shared cell runtime owns tool effects, output and the session JSON store. Both backends wait for physical cleanup before reporting completion, including cancellation and dropped observers. Python globals never persist; `store`/`load` values do and can cross language boundaries. Only lossless JSON values cross the worker boundary, with a 128-level nesting bound.

The default limits are 64 MiB interpreter allocation and 30 seconds cumulative execution. The process allocator adds bounded headroom for protocol and interpreter overhead. Worker startup and Host waits do not consume execution time; compilation does. Cancellation kills and reaps the worker and settles admitted tool work. The allocator is installed only in the worker binary, never in the Host.

[Source](src/lib.rs) · [Cell instructions](../../plugins/agent/tools/src/cell/python.md)
