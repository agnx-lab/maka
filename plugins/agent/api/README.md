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

# maka-agent-api

[简体中文](README.zh-CN.md)

Contains Agent, Session, Executor and WorkHub wire declarations, plus the Agent configuration schema. Native services and SDK generation share these definitions. Use this crate when a consumer needs the public contract without pulling in the Agent execution engine.

Independent Desktop clients consume the same persisted Agent service through the Root-bound [public Host client](../../../platform/host-runtime/src/application/host/client.rs); they do not open Agent storage or embed its execution engine. [Agent contracts](src/contract.rs) and [interaction contracts](src/contract_interactions.rs) are the source of field definitions and schemas.

Read `history` pages under one fixed `through` fence and resume `watch` from its last `nextAfter`; watch also publishes queue and pending-interaction updates. `send` admits work and returns its identity; `stop` names that exact invocation. Use `pendingInteractions`, `interaction` and `respond` to reconstruct and answer the original request after reconnecting. Mutations retain their original operation receipts: a lost response does not authorize another send. See [native acceptance](../native/README.md) and [transport ownership and exit modes](../../../platform/transport/README.md).

History row references use `position: { sourceSequence, itemIndex }`, ordered by the original emitting event and then its row ordinal. Cursors, source reads, prompt anchors and tool-image references share this position. `through` and `{ kind: "event", sequence }` still address raw events; a model completion publishes all of its rows at that completion’s event.

`search` scans the selected Session’s history with a literal query and explicit `caseSensitive` and `wholeWord` options. Case-insensitive matching uses Unicode simple case folding without normalization; whole-word matches require no adjacent Unicode word character (letters, marks, decimal digits, connector punctuation or join controls). Start with `after: null`. Keep the query, options and `through` fence fixed while advancing `nextAfter`, including pages with no hits; null ends the scan. Each hit identifies the exact original field for `source` reads.

`fork` starts a side conversation from a complete Turn or all current history, guarded by the source revision. Included work must be settled; an earlier complete Turn can still be forked while a later Turn runs. The new Session retains that history and its attachments, shares the verified workspace, and starts with narrowed permissions and no active work. Its `branch` identifies the source. Recover a lost response with the original operation reference; `forkReceipt` can locate a published fork even when the Host outcome remains unknown. A null result does not prove that nothing happened or authorize a replacement fork. See the [fork declarations](src/contract_fork.rs).

`diagnostics` reads bounded, newest-first pages of execution, model-step and automatic-title failures under a fixed `through` fence. `watchDiagnostics` emits the current diagnostic revision on subscription and after changes; reconnect by reading the latest page again. These session-scoped operational records retain original invocation/event identities and safe typed causes. They exclude provider bodies and failures belonging to copied source sessions. Their revision is independent of conversation history, so a late title failure remains discoverable after the answer finishes.

`context` reports the latest Main request prepared by the selected Session, including requests still running or interrupted. It groups the original Host input by message role and tool schemas; token counts are estimates, while provider-reported input usage is separate. Pin `through` to reread the same result after reconnecting. Missing requests or measurements remain null, and copied history does not count as a request executed by the new Session. See the [context declarations](src/contract_context.rs).

`gitStatus` and `gitDiff` read the selected Session's repository through the same Agent service, including WorkHub coordinator sessions. Status separates staged, worktree and untracked changes; a path can appear in both staged and worktree areas. Diff compares HEAD to the index, the index to the working file, or an empty file to an untracked file, respectively. An unborn repository has a null `head`; a workspace without a repository returns null. Paths use exact Git pathname bytes encoded as `bytesBase64`, and renames appear as deletion plus addition.

These are fresh, read-only observations, not repository snapshots. Status marks an incomplete result with `truncated`; per-file diff distinguishes text, binary, unchanged, unsupported and oversized input/output. Responses fit within 512 KiB and diff inputs are limited to 1 MiB per side. The workspace must be the repository root. Unsupported repository features fail explicitly, including external filters, sparse checkout and submodules; configured commands are never executed. A changed workspace or comparison base requires a fresh query. [Git declarations](src/contract_git.rs) define the wire types; immutable completed-work patches remain available through `snapshotPatch` and `readPatch`.

`listFiles` lists direct children of the selected Session workspace; `watchFiles` reports when that directory needs rereading. Both also work for WorkHub coordinator sessions. Use null `path` for the workspace root, or a `WorkspacePath` carrying exact relative pathname bytes as `bytesBase64`. Symlinks are leaf entries; `.git`, the workspace marker and private Host state/control directories are excluded, including in unrestricted model workspaces. Only the Session’s validated managed worktree is readable inside private state. Pages contain at most 128 entries and 512 KiB, scanning at most 100,000 children; paths are at most 4,096 bytes and 64 components deep. Unsupported platform filenames fail explicitly instead of being converted lossily.

Start the watch, wait for `ready`, then read the first list page and compare its `directoryIdentity` with `ready`. Follow `next` for further pages; a `changed` notification means discard the cursor and reread from the beginning. Pages are live observations, so concurrent edits can change pagination. Notifications can coalesce and are not a replayable event history: reconnect with a new watch and fresh listing. Detected deletion or replacement of a selected subdirectory ends the watch with `unavailable`; invalid Session/workspace authority or native observation failures end it with an error. On macOS an idle workspace-root move may only be detected on the next notification, read or reconnect. Close the subscription when leaving the directory.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
