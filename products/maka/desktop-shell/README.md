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

# Maka desktop shell

[简体中文](README.zh-CN.md)

`maka-desktop-shell` runs the authored [app](app/) with the retained GPUI shell and QuickJS runtime. Its native modules expose the public Host client, selected files and system locale; the app owns its views and commands.

Build from the repository root:

```sh
just build
# Or: just build --release
```

This builds the CLI and shell and stages the complete app beside the executable. `--target TRIPLE` selects another Rust target. Native packages carry `bin/maka`, `bin/maka-desktop-shell` and `bin/app/` (plus Windows executable suffixes and its service helper). Bare shell startup resolves the adjacent app, including when launched from another working directory.

Use the owned launcher:

```sh
/absolute/path/to/maka desktop --root /absolute/path/to/business-host
```

`--root` is required and names the actual business Host Root. The existing local connection path initializes a missing Root with the standard read-only business Host or reopens its saved deployment. The launcher verifies discovery against the connected Host endpoint, source and epoch, then supplies `MAKA_HOST_ENDPOINT` and `MAKA_HOST_EPOCH` to the child. It holds its own Host client while the shell runs. Child exit or launch failure settles that client; quitting requests idle shutdown while other connected clients retain the Host. Cancellation stops only this launcher's child.

For direct authoring, keep an existing Host available and supply its endpoint (and, when known, epoch). The shell retains its normal commands:

```sh
export MAKA_HOST_ENDPOINT=/path/to/host.sock
export MAKA_HOST_EPOCH=UUID
target/debug/maka-desktop-shell run --dir products/maka/desktop-shell/app
target/debug/maka-desktop-shell dev --dir products/maka/desktop-shell/app
target/debug/maka-desktop-shell check --dir products/maka/desktop-shell/app
```

`dev` reloads the source app; Host-backed actions require the same connection environment. `check` loads the application description and reports diagnostics. `--help` and `--version` answer without opening a window or starting a Host.
