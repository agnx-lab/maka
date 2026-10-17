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

# Build a plugin

## In this repository

Choose the owning family under `plugins/`. Each plugin's `package.ts` exports a `PackageDeclaration`; JavaScript entries point to local modules built from matching `.ts` files. Native entries use `sdk.nativeImplementations` so identifiers, contracts and configuration schemas come from the Rust definition.

From the repository root, build an existing leaf as a reference:

```sh
node scripts/plugins/build.mjs   --plugin plugins/workspace/workspace-web --output /tmp/maka-web-bundle
maka application inspect-package --package /tmp/maka-web-bundle/packages/workspace-web
```

The builder replaces the output directory. It builds the selected package, without assembling the product or recompiling Rust.

For a product feature, add the plugin to `products/maka/bundle/package-inputs.json` and bind its instances in `src/composition.ts`. Use `just product` to rebuild the development bundle, or `just run` to build and launch. Existing installations keep their saved source; commit an explicit source update to change one.

## Standalone starter

Copy the three files from this skill's `assets/starter/` directory together: `create.mjs`, `host.mjs` and `tui.mjs`. Then run:

```sh
node create.mjs --output /tmp/my-plugin --target tui --maka /path/to/maka
```

The output directory must be new. `--target host` creates an Echo service graph; `--target tui` creates a native terminal counter graph. The helper reads the executable's schemas, computes exact package references and checks the composition. It does not initialize a Root or start execution. Use the emitted input and package paths for later commands.

The generated `maka.extension.json` uses `schemaVersion: 1` and `application.entries`. A JavaScript entry declares its bundled module and realm; component entries also declare the target's scene language. The composition selects pinned package references, instance UUIDs, entry names and bindings.

Keep the composition outside the package so its digest cannot refer to itself. Recompute references after changing package bytes, and retain instance identities when updating saved data. See [Testing and shipping](testing-and-shipping.md) to check and install the result.
