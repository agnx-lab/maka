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

Without `--session`, the centered composer offers **Choose workspace** before editing. Add `--session SESSION_ID` to open an existing conversation directly. The Host must provide `agent`, `settings` and `receipts`. Exiting closes only the shell's own connection; the Host follows its existing lifecycle and idle policy.

Choose **Choose workspace** or press Ctrl+N to open **Workspaces**, then choose a directory published by the Host. Within a conversation, a new conversation reuses its authorized workspace. Creation then opens **Models** so you can choose a configured connection and model before sending. Use the existing client to configure connections if none are available.

**Sessions** uses a collapsible sidebar when the terminal has room for both it and the conversation; smaller windows use a dialog. Open either with the header's browse button or Ctrl+O. Drag the divider or use **Narrower** / **Wider** to adjust the sidebar. Choose **All workspaces**, the current session's exact workspace, or a project. A project's **+** creates in its Host-authorized directory; **New in current workspace** follows the open conversation. An isolated worktree is not necessarily part of its parent project.

## Keyboard

| Key | Action |
| --- | --- |
| Ctrl+P | Open **Commands** |
| Ctrl+O | Open **Sessions** |
| Ctrl+G | Open model and effort choices in **Models** |
| Ctrl+N | Create a conversation |
| Ctrl+L | Jump to the latest messages |
| Alt+↑ | Focus the conversation for scrolling |
| Alt+↓ | Open and focus the latest process group (or its flat rows) |
| Tab | Move between controls |
| Enter | Send from the composer, select a search result, or activate the focused control |
| Shift+Enter | Insert a newline in the composer |
| Esc | Go back within **Models**, then close it; otherwise close the overlay or focused browser, or request Stop from the conversation |
| Ctrl+C | Copy selected text through the terminal's OSC 52 support |
| Ctrl+Q | Quit |

Model selection uses **Ctrl+G** because classic terminals encode Ctrl+M as the same carriage return as Enter. Paste through your terminal's paste command.

In a search field, type to search, use ↑/↓ to select, and Enter to activate. Sessions searches titles within the chosen scope, using a case-sensitive literal substring of at most 128 Unicode characters. The Host applies this search before paging; it does not search conversation contents, paths or IDs. Model search uses the Host's catalog. Results load automatically as you scroll or move down.

Click the composer's model label or press Ctrl+G to open **Models**, then choose **Model** or **Effort**. Effort choices come from the selected model's published policy; **Off** appears only when supported. **Default · Use connection default** clears an explicit effort override and follows the connection's default. Re-selecting the same model on the same connection preserves an explicit effort choice. Escape or **Back** returns from either list to the menu; another Escape or **Close** closes it and restores focus to the opener, preserving the draft and caret.

## Conversation and requests

Earlier messages load automatically as you scroll up, preserving your reading position. Continuous thinking and tool activity form process groups; an intermediate reply or incoming message starts a new group. Replies appear outside these groups, and the final answer appears once. Completed turns fold their work by default. Stopped, failed and interrupted turns, and turns with interleaved input, keep their work available. Completion and final-answer placement use the Host's recorded turn metadata.

**Details** selects **Compact**, **Standard** (the default), **Detailed** or **Verbose**, independently of **Chat** / **Trajectory**. Compact and Standard show group headers with closed contents; Compact omits live header detail and settled thinking previews. Detailed lays out current work as ordinary conversation rows and groups historical work. Verbose keeps all work flat and never folds completed turns. **Fold** chooses completion or the next input; your explicit turn disclosure choice takes precedence. These settings persist locally for the selected Root.

Grouped activity has independent native scrolling, capped at half the reading viewport. A newly opened live group starts at the bottom, while a closed group starts at the top. An already open reader stays in place as the group closes. Scrolling up keeps your source reader in place when later replies arrive. Thinking disclosure and Source choices remain attached to their original rows across updates and policy changes. Tool details load on demand from the Host; a layout change can reload their original source.

Assistant responses format headings, emphasis, lists, quotes and code. **Source** switches to the exact selectable text. Partial history stays literal until its opening is loaded. Responses larger than 256 KiB remain complete selectable source to bound formatting work; this does not limit messages or content retained by the Host.

**Todo** sits above the composer and displays the Agent's actual checklist, including its current item and completed count. Token speed reports the **last request**; context figures mark approximate or last-known values. Unavailable metrics are omitted.

The activity header follows the latest thinking, streamed tool arguments or running tool. A request waiting for your response takes priority. Long titles shorten at complete Unicode characters, and title updates preserve your position in the activity reader.

Expand a tool result containing an image to see its thumbnail, then activate **Open image** for a larger preview. Escape or **Close** returns to the conversation and restores focus without stopping the run. Both views use the original image retained by the Host, even if its workspace file changes. The shell detects Sixel support and cell pixel size at startup; otherwise, images automatically use colored Unicode half and quadrant blocks. After resizing the terminal, images use Unicode until the shell is restarted. Unsupported image formats show an error in the result.

Open a pending request to review its original details:

- Tool approval supports **Allow** or **Deny** for that call. Read all original arguments before allowing.
- Additional permissions support **Allow** or **Deny**, with the available call, turn or session scope.
- Questions support the listed choices, **Other answer** and an explicit **Skip**.
- Forms support explicit **Decline** or **Cancel request**. Fill full forms and handle capability requests in the existing client; those editors are not available here.

Use Tab to move between request controls and Enter to activate. For approvals, permissions and questions, Ctrl+Enter allows or submits answers; Ctrl+R refreshes the recorded status. Escape only closes the request view and leaves an unanswered request pending. The displayed recorded outcome is authoritative if another client answered first.

## Saved state and scope

Drafts and operation receipts persist in `<root>/clients/terminal.sqlite`. Each session keeps its own draft, and recovery uses the original Host and operation. Switching conversations saves the current draft first. Recovery starts from the latest acknowledged draft revision; newer unsaved input can be lost on close. A send uses that saved revision. Use the original-operation recovery action for an uncertain result, rather than submitting a duplicate.

Presentation preferences persist separately in `<root>/clients/org.apache.maka.terminal/storage.json`, shared by this Root's terminal conversations. They contain local view choices; the Host remains authoritative for sessions, messages and execution.

The executable verifies the selected Root and live Host epoch. Ctrl+Q, and SIGTERM on Unix, request shutdown independently of JavaScript and terminal output. Shutdown attempts terminal restoration and closes owned Host requests and tasks; incomplete cleanup returns an error.
