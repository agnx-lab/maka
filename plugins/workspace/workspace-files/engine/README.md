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

# maka-fs-tools

[简体中文](README.zh-CN.md)

Implements file reads, search, edits, writes, patches and worktree operations. Callers provide the authorized filesystem scope. `ReadExecutor` handles bounded reads; `MutationExecutor` and `WriteCoordinator` prepare and serialize writes. Patch parsing is delegated to the adjacent `patch` crate.

`workspace` prepares intrinsic identity through the original captured directory. For a usable Git repository, it appends `.maka-workspace.json` to the common `info/exclude` only when absent, preserving existing bytes. This fixed preparation covers selected subdirectories and linked worktrees; it requires the real private Root/control boundaries and an inspected binding for managed worktrees. An unusable nearest `.git` entry does not redirect preparation to an ancestor repository; failures writing a valid exclusion remain errors. Existing tracked files and user ignore rules retain their normal Git precedence.

`git` provides native, read-only repository status and per-file comparisons. Callers supply the original directory identity, private State Root and any validated managed-worktree binding, and retain the blocking worker until it settles. Git metadata and ignore/attribute control files are checked before library traversal; external commands and alternate object stores are not used. Cancellation is cooperative, and response limits do not promise a fixed execution time or an atomic repository snapshot.

`files` lists direct children through held directory capabilities and reports native invalidation hints. The caller supplies the original workspace identity, private Root/control paths and validated managed-worktree binding; caller paths cannot create that exception. `WorkspacePath` is shared with Git and preserves exact relative pathname bytes. Listing is live and bounded, with no snapshot cache. A watch registers the workspace-to-target directory chain before readiness, ignores read/access notifications, coalesces changes and revalidates names before delivery. The original worker must await `Watch::close` through normal native watch/callback cleanup, even after client cancellation. Native filesystem notifications are not a durable event history.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
