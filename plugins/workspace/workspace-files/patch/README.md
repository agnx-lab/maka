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

# apply-patch

[简体中文](README.zh-CN.md)

Parses complete patch text and applies updates to supplied strings, preserving operation order and original context and line endings. Preparation retains full strings and parsed lines in memory. It performs no filesystem I/O. Callers validate paths and obtain write authority before applying the prepared operations to files. See [SOURCE.md](SOURCE.md) for the upstream source and adaptation record.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
