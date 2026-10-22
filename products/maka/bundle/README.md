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

# Product bundle

[简体中文](README.zh-CN.md)

This directory selects the packages shipped with Maka and binds them into Agent, WorkHub or Studio compositions. Plugin declarations and implementations live under [plugins](../../../plugins/README.md).

- [package-inputs.json](package-inputs.json) selects authored packages.
- [src/composition.ts](src/composition.ts) builds instances, bindings and installation identities.
- [scripts/build.mjs](scripts/build.mjs) emits package bytes and authoring code into `dist`.

## Run and develop

From the repository root:

```sh
just run
just run --root /path/to/business-host
```

`just run` opens the [NSH terminal workbench](../terminal-shell/README.md), using `~/.maka/application/host` by default. It connects to an existing Host or starts one automatically. Authoring commands below generate and manage plugin compositions independently of this terminal interface.

For a customized UI product, close Maka and run `maka application update-defaults` to preview a three-way merge of the previous defaults, local composition and new defaults. Use `--apply` to synchronize when the preview has no conflicts; `--root PATH` selects another installation. Independent changes merge by field; conflicting values, deletions and ordered lists report JSON Pointer paths and prevent the entire update. The preview validates the merged composition and actual packages. Updates preserve installed packages that are not selected by the composition, within their original Host or UI. Completed synchronization records the new default baseline, and interrupted writes resume on reopening. UI installations without a saved baseline require explicit composition updates. Standalone Hosts preserve their composition and support [selected package updates](../cli/README.md#product-updates-recovery-and-url-installation) without a UI or default baseline.

`just product` rebuilds the TypeScript bundle. Development binaries load `dist`, or the directory named by `MAKA_PRODUCT_BUNDLE`; TypeScript changes do not require Rust recompilation. Release binaries embed the bundle. The build generates `product-version.txt` from exact authoring and named package bytes. An unchanged launch reads this identifier without scanning package contents; installation and updates still validate real package bytes against it. Treat `dist` and `MAKA_PRODUCT_BUNDLE` as generated artifacts: rebuild after source changes instead of editing output files. Missing, malformed or mismatched identifiers require a rebuild. Use a separate output directory when building an artifact alongside a running development build:

```sh
node products/maka/bundle/scripts/build.mjs --output /tmp/maka-product-bundle
```

## Author an installation

The CLI can produce checked sources and packages without Node at runtime:

```sh
maka application compose --configuration installation.json --output product-source
```

The output directory must be new. The command writes Host, management and UI sources, connection grants, resolved installation identities and `.maka-plugin` packages. It does not start or install them.

`installation.json` selects `presentation` (`tui` or `desktop`), `locale`, `scheme`, `hostRoot` and service configuration. Agent uses the generated `AgentConfiguration`; WorkHub adds a separate `coordinator`. Optional services and ACP executors are selected explicitly. See [the configuration parser](src/composition.ts) and [SDK types](../../../packages/plugin-sdk/src/generated/runtime.ts) for accepted fields.

Preserve the emitted `keys`, `hostDeployment` and `managementDeployment` when reauthoring an installation. They identify existing owners and data. Package revisions identify bytes and can change independently.

For explicit installation, initialize the target Host Root with `maka host init`, then commit `host.json` using `maka application commit`. Supply the selected deployment ID, expected revision and exported packages. Composition and source commands also support independently managed UI deployments. Each command's `--help` lists its required arguments.

Committing stores desired source and bytes; it does not activate or select that deployment. Use `maka application serve --help` for explicit deployment serving, endpoints and native grants. `maka --root HOST_ROOT` and `maka desktop --root HOST_ROOT` use the Host Root's saved selection.

## Verify changes

```sh
npm --workspace @maka-agent/product run typecheck
npm --workspace @maka-agent/product test
MAKA_PRODUCT_CLI_CHECK=1 npm --workspace @maka-agent/product test
```

Tests build once, then exercise the retained component behavior. Setting `MAKA_PRODUCT_CLI_CHECK` also checks packages and compositions against the actual CLI. Native rendering, focus and input changes need verification in their intended presenter.
