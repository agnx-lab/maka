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

# Maka NSH terminal shell

An experimental Agent workbench built with NSH, supporting mouse and keyboard interaction and connecting to or starting a Maka Host Root.

## Start

From the workspace root, build the product bundle and sibling native launcher, then start the shell:

```sh
npm run build --workspace @maka-agent/product
cargo build -p maka-cli --bin maka
cargo run -p maka-terminal-shell -- --root /path/to/root
```

The shell discovers and verifies the selected Root's live Host. If none is running, it uses the sibling `maka` native launcher and the existing deployment policy to start one. A separate `host serve` terminal is unnecessary. The launcher is resolved beside the shell executable. `--root` selects the business Host Root; the default product normally uses `~/.maka/application/host`.

Without `--session`, Home shows recent conversations and **New conversation**. Add `--session SESSION_ID` to open an existing conversation directly. The Host must provide `agent`, `settings` and `receipts`. Exiting closes only the shell's own connection; the Host follows its existing lifecycle and idle policy.

From Home, creating a conversation opens **Workspaces**: choose a directory published by the Host. Within a conversation, a new conversation reuses its authorized workspace. Creation then opens **Models** so you can choose a configured connection and model before sending. Use the existing client to configure connections if none are available.

## Keyboard

| Key | Action |
| --- | --- |
| Ctrl+P | Open **Commands** |
| Ctrl+O | Open **Sessions** |
| Ctrl+G | Open **Models** for the current conversation |
| Ctrl+N | Create a conversation |
| Ctrl+L | Jump to the latest messages |
| Alt+↑ | Focus the conversation for scrolling |
| Alt+↓ | Expand and focus the latest turn's activity panel |
| Enter | Send from the composer, or activate a selected palette entry |
| Shift+Enter | Insert a newline in the composer |
| Esc | Close the current overlay; otherwise request Stop for the current run |
| Ctrl+C | Copy selected text through the terminal's OSC 52 support |
| Ctrl+Q | Quit |

Model selection uses **Ctrl+G** because classic terminals encode Ctrl+M as the same carriage return as Enter. Paste through your terminal's paste command.

In a palette, type to search, use ↑/↓ to select, and Enter to activate. Sessions searches by title, workspace or ID; Models searches the Host's model catalog. Both load more results as you scroll or move down. Escape closes the palette without choosing.

## Conversation and requests

Earlier messages load automatically as you scroll up, preserving your reading position. Each turn has its own bounded native activity panel with independent scrolling. It follows new activity near the bottom; scrolling up keeps your reading position. Completed turns collapse by default, with their final answer outside the panel. Activate a turn header to expand or collapse it. Completion and final-answer placement use the Host's recorded turn metadata, so an individual model response finishing does not prematurely close an ongoing turn. Thinking remains expanded when you reopen a turn. Tool details are read on demand; reopening a panel with expanded tool details reloads them from the top.

**Todo** displays the Agent's actual checklist, including its current item and completed count. Token speed reports the **last request**; context figures mark approximate or last-known values. Unavailable metrics are omitted.

Open a pending request to review its original details:

- Tool approval supports **Allow** or **Deny** for that call. Read all original arguments before allowing.
- Additional permissions support **Allow** or **Deny**, with the available call, turn or session scope.
- Questions support the listed choices, **Other answer** and an explicit **Skip**.
- Forms support explicit **Decline** or **Cancel request**. Fill full forms and handle capability requests in the existing client; those editors are not available here.

Use Tab to move between request controls and Enter to activate. For approvals, permissions and questions, Ctrl+Enter allows or submits answers; Ctrl+R refreshes the recorded status. Escape only closes the request view and leaves an unanswered request pending. The displayed recorded outcome is authoritative if another client answered first.

## Saved state and scope

Drafts and operation receipts persist in `<root>/clients/terminal.sqlite`. Each session keeps its own draft, and recovery uses the original Host and operation. Switching conversations saves the current draft first. Recovery starts from the latest acknowledged draft revision; newer unsaved input can be lost on close. A send uses that saved revision. Use the original-operation recovery action for an uncertain result, rather than submitting a duplicate.

The executable verifies the selected Root and live Host epoch. Ctrl+Q, and SIGTERM on Unix, request shutdown independently of JavaScript and terminal output. Shutdown attempts terminal restoration and closes owned Host requests and tasks; incomplete cleanup returns an error.
