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
| Declare a Host service and explicit imports | `plugins/application/gateway/package.ts`, `plugins/application/authoring/package.ts` |
| Inspect exact contracts and immutable package declarations | `references/sdk/contracts/`, `plugins/shared/package.ts` |
| Wire Agent/WorkHub instances, capabilities and native grants | `products/maka/bundle/src/composition.ts` and its imported modules |
| Author a standalone SDK component | `assets/starter/tui.mjs` and the generated SDK types |

In packaged resources, prepend `references/source/` to those paths. Relative imports retain their checkout spelling; an import into `packages/plugin-sdk/src/` is available under `references/sdk/`. This is reference source, not a standalone build tree. The quickstart starter supplies the standalone build.

Read a package declaration before its implementation. Imported names and contracts must match the composition where the package will run. Inspect the current Host through its deployment manager before editing it; installed sources may differ from the bundled examples. Maka's terminal and desktop interfaces are authored by their shells and do not load this SDK's standalone component scenes.

The build uses an explicit source allowlist. It excludes native implementation directories, tests, local files and build output; `.gitignore` is not the publication boundary. Extend the allowlist when another complete example materially helps authoring.
