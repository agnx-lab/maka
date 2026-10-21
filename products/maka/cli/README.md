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

# maka-cli

[简体中文](README.zh-CN.md)

Builds the `maka` executable. It starts the terminal product, serves Hosts, exposes ACP and provides application inspection, composition and installation commands. Product startup wires native factories and JavaScript execution adapters into the platform owners.

From the repository root, run `just run` or `just run --help`. Development builds load `../bundle/dist`; `just run` rebuilds that bundle before Cargo starts. Release builds embed it. Desktop runs in a separate sibling executable.

[Source](src/main.rs) · [Cargo manifest](Cargo.toml)

The first installation asks for workspace trust inside the TUI, before starting the Host or enabling project skills and tools. Tab or the arrow keys select an action; Enter confirms and Esc exits. The initial selection is Exit. Confirmation keeps the same terminal screen while the product initializes. Reopening uses the saved installation without asking again.

Startup discovers the local Host and connects to it, or starts one automatically. TUI and Desktop can share that Host. By default, quitting a UI also stops the Host when no other client remains. Use Ctrl+Alt+Q in the TUI to close only the UI and keep accepted background tasks running; reconnect to observe their results.

`host serve` and `application serve` emit a JSON `ready` line with `rootId`, `hostEpoch` and `socketPath`. Use that epoch when connecting and verify the live Host identity. On Unix, sockets live in a private temporary child directory selected by the system; set `TMPDIR` for a different parent. The canonical socket path must fit the portable 100-byte limit; choose a shorter parent if startup reports that limit.

Headless commands (`host`, `application serve`, `code` and `acp`) install process logging before runtime initialization. Warnings and errors, including SQLx slow-query warnings, go to stderr without ANSI styling; stdout remains available for command results and protocol frames. Ordinary DEBUG query logs are disabled. Interactive products keep control of their terminal output.

Dedicated Host processes capture stderr in their Root's private control-cache file `host.stderr.log`: on-demand capture starts before spawning, while foreground serving starts after Root selection. Read it with `maka host logs --root-id ID`, or add `--follow` for live NDJSON and `--after CURSOR` to resume. Pages preserve raw bytes as Base64 and report missing capture, replacement or truncation; clearing the control cache removes these logs. This works without an installed service. The default product's Diagnostics → Host logs view supports reading and following without a conversation.

## Product updates, recovery and URL installation

Composition commands read complete JSON source and remote-export files. The Plugins composition editor loads an exact source revision in UTF-8 pages of at most 64 KiB and saves the confirmed native editor snapshot against that revision. Whole documents have no separate byte ceiling; typed validation, package checks, Host permissions, revision conflicts and actual resource admission still apply. Committed revisions retain the complete source for subsequent reads and reopening the Root.

To update a standalone Host's selected bundled plugins, stop that Host and use its State Root:

```sh
maka application update-defaults --root /path/to/host
maka application update-defaults --root /path/to/host --apply
```

Review the exact package revisions in the preview. Apply replaces explicitly selected packages whose IDs match this executable's bundle, including locally modified packages with those IDs. Other packages, composition settings, permissions and business data stay intact; new default components are not added. Invalid combinations are rejected before updating. After an interruption, rerun `--apply` to finish the same update, then start the Host. This updates application packages; `host upgrade` updates the native executable.

`update-defaults` requires stored package records that the current executable can decode. A Root containing a removed internal format, such as the old `desktop` surface, fails before preview; preserve that Root and initialize a new one.

Close the product UI before recovery; restoration updates the independent Host through its management service. Direct package installation requires the selected Root to be inactive. Both commands default to `~/.maka/application`; select another installation with `--root DIRECTORY`.

```sh
maka application restore-defaults
maka application install-url --url https://example.com/plugin.maka-plugin
```

`restore-defaults` restores the Host, management and UI compositions from the original installation, with their exact packages and instance IDs. It keeps conversations, credentials, plugin data, connection authority and source history. It restores the installation baseline, not a newer executable's defaults. An interrupted restore resumes the same prepared revisions when reopened; later conflicting edits are rejected.

`install-url` accepts a direct HTTP(S) `.maka-plugin` bundle URL. It validates the package digest and bounded downloads, stores an immutable revision, and leaves activation to the Plugins composition editor. Add `--sha256 HEX` to verify a publisher-provided bundle digest, `--expected-revision N` for an explicit source comparison, or `--deployment UUID` for an independently committed deployment. Downloads use the process proxy environment. The live Plugins install page and Agent `UIRequest` also expose URL installation through the bound deployment manager.
