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

# Source provenance

Third-party source and dependency patch attribution is recorded in [LICENSE](../LICENSE). Apply-patch adaptations have a detailed record in [plugins/workspace/workspace-files/patch/SOURCE.md](../plugins/workspace/workspace-files/patch/SOURCE.md); native Computer Use notices live in [platform/computer-use/THIRD_PARTY_NOTICES](../platform/computer-use/THIRD_PARTY_NOTICES).

### Native model protocol validation

`plugins/agent/provider-protocol` implements Chat Completions compatible and Anthropic Messages with native Rust input types, wire decoders and stream state. Its behavior was checked against the relevant unchanged Vercel AI SDK tests at revision `e623e8c88b55066dde769614a16ae51d424974aa` (openai-compatible 3.0.77 and anthropic 4.0.87). The upstream tests and temporary validation bindings are external validation tools and are not distributed with Maka. The product uses the same Rust protocol core through Host-scoped transport.

### Source archive non-text inventory

- `assets/logo.png`: Product artwork reproduced by `scripts/generate-logo.py` from its vector geometry; Apache-2.0.
- `products/maka/terminal-shell/app/assets/maka.png`: Byte-identical copy of `assets/logo.png`, bundled with the terminal application's composer; Apache-2.0.
- `website/src/assets/incubator.png`: Unmodified Apache Incubator trademark artwork from https://www.apache.org/logos/res/incubator/default.png.
- `website/src/assets/social.en.png`: English website preview rendered by `website/scripts/social-preview.mjs`; includes the product mark and self-hosted OFL-1.1 fonts.
- `website/src/assets/social.zh-CN.png`: Chinese website preview rendered by the same source and toolchain.
