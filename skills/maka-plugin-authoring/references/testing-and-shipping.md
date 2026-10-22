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

# Validate the actual artifact

Use `maka application inspect-package --package PATH` after changing package bytes.
Use the returned immutable reference in the composition, then check every selected
package and presentation:

```sh
maka application check --input composition.json --package plugin --package surface
maka application export-package --package plugin --output plugin.maka-plugin
```

Export creates a new file. A checked package can still have wrong handler behavior:
exercise service validation and actual calls, Component lifecycle/action replies,
and native rendering/input for the intended presenter. Include unknown mutation and
cleanup behavior when the feature owns effects. Test receipt recovery against the
original reference, rather than asserting that retrying succeeds.

On macOS/Linux, copy `assets/starter/verify-host.py` beside `create.mjs`, `host.mjs` and `tui.mjs`
and run `python3 verify-host.py --maka /path/to/maka`. It generates a fresh Host
package and an explicit consumer, creates a temporary Root, commits the graph,
launches the real Host with only the Echo query grant, verifies ASCII/Unicode and
invalid inputs, then closes the Host and removes the temporary directory. This is
a launcher test fixture: plugin business code uses bound capabilities, not the raw
IPC framing shown in the fixture.

The starter generator checks Host or TUI graphs using the actual CLI. Its
Echo service is a query, and its counter is local UI state. It does not demonstrate
durable effects, cross-process grants or model execution; add those through their
own explicit contracts when the requested feature needs them.

Installation uses existing owned Roots. For a new isolated test Root, initialize it
with `maka host init --root PATH`, then commit a deployment using
`maka application commit --root PATH --deployment UUID --expected-revision 0
--input composition.json --package plugin --package surface`. Read back the source
with `maka application read`. Updates use the observed revision and preserve instance
identities. Commit stores desired input and bytes; it is distinct from successful
activation and cleanup.

Verify native rendering and input with the intended presenter's existing component
fixtures. To exercise an installed business Host through Maka's terminal workbench,
run `maka --root HOST_ROOT`; the Host's selected deployment and explicit grants
remain authoritative.

In a checkout, run the relevant crate tests and SDK/product type checks. Product
tests build once before running. To build another product artifact concurrently,
use `node products/maka/bundle/scripts/build.mjs --output PATH` with a separate
directory; two writers must not share `dist`.

For an Agent changing its connected foreground, use [Runtime UI authoring](runtime-ui.md). Its text package and document commands reuse the deployment manager’s validation and revision checks; the original operation receipt remains the evidence for the edit.
