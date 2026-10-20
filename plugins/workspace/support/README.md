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

# maka-workspace-support

[简体中文](README.zh-CN.md)

Shares directory selection and validation across workspace services. It opens configured roots, excludes private Host paths and checks a selected workspace against those roots. `Root`, `query` and `validate` are the main entry points. A selected path is usable only within the authority supplied by the owning service.

Child queries enumerate the selected directory before sorting and returning pages of up to 32 children within 48 KiB. Ordinary files do not prevent browsing child directories. Continue with the returned `after` cursor; each query revalidates the published root and selected directory identity.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
