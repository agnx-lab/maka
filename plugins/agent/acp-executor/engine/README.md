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

# maka-external-agent

[简体中文](README.zh-CN.md)

Implements outbound ACP agent conversations, installation and setup. It translates between Maka execution ports and an external agent process. The native wrapper supplies explicitly bound process, file and interaction services. This is the outbound executor; the ACP server exposed to editors lives under `products/maka/acp`.

ACP prompt, attachment, file-read and output carriers preserve complete supported content through their original process and callback owners. Lossless text/write chunks remain; a prompt has no independent elapsed-work cutoff. Connect/configuration observation and cleanup deadlines retain their lifecycle meaning. Installer verification preserves exact release-owned filenames, lengths and hashes, with staged publication and cancellation; the public installer API does not imply a demonstrated production UI invocation.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
