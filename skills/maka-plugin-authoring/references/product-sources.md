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

# Product source examples

The bundled skill includes selected TypeScript source from the same build as its SDK. Read `references/source-index.md` for exact resource paths and sizes, then request only the files needed. In a checkout, read the original paths below.

| Task | Start here |
| --- | --- |
| Implement a small component or composer footer | `plugins/agent/composer-footer/package.ts`, `tui.ts` |
| Native input, editor snapshots and callbacks | `plugins/agent/composer/index.ts` |
| Read and mutate a deployment with original receipts | `plugins/application/deployment/index.ts` and its local modules |
| Process Agent UI requests and recover an interrupted mutation | `plugins/application/authoring/index.ts` |
| Create a complete Agent main screen with slots and one state owner | `plugins/agent/agent-screen/package.ts`, `index.ts`, then its local modules |
| Find exact component props, contracts and shared view helpers | `plugins/shared/contracts.ts`, `package.ts`, `view.ts` |
| Wire instances, capabilities and grants | `products/maka/bundle/src/composition.ts` and its imported modules |

In packaged resources, prepend `references/source/` to those paths. Relative imports retain their checkout spelling; an import into `packages/plugin-sdk/src/` is available under `references/sdk/`. This is reference source, not a standalone build tree. The quickstart starter supplies the standalone build.

Read a package declaration before its implementation. A component's imported slot names and contract props must match the composition where it will run. For runtime edits, inspect the installed source using `UIRequest`; it may differ from the bundled examples after customization.

The build uses an explicit source allowlist and file/byte budgets. It excludes native implementation directories, tests, local files and build output; `.gitignore` is not the publication boundary. Extend the allowlist when another complete example materially helps authoring.
