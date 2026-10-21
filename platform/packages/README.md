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

# maka-packages

[简体中文](README.zh-CN.md)

Loads immutable application packages from declarations and files. It validates package paths, bytes and manifests before the composition planner sees them. A package reference identifies content; loading or installing that content neither starts an instance nor creates a binding. Start with `Package` and the `manifest` module.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)

Directory loading and bundles retain every regular file and its original bytes, including binary assets and localized presentation. Content digests, portable paths, nonfollowing directory handles and nonoverwriting export publication bind the complete package; allocation and filesystem failures remain explicit.
