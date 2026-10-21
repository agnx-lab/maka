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

# maka-ui-runtime

[简体中文](README.zh-CN.md)

Owns component activation, state, input dispatch and atomic frame updates. `Application` runs a checked UI composition; `NativeRegistry` supplies compiled factories and a scene compiler. Products inject a JavaScript execution adapter through the `backend` module when needed.

A callback proposes state, a frame and one-shot actions. Rust owns the committed state and advances one logical transaction at a time. Background preparation does not publish actions or replace native resources. The original renderer owner checks the candidate's mount and frame basis, completes necessary fallible native preparation, then commits the whole batch atomically. Layout and paint consume committed frames; the terminal renderer retains its layout and scene language.

Committed frames and accepted calls share immutable JSON state and props snapshots. Each callback result creates a separate candidate state; rejecting it preserves the original snapshot. Native callbacks borrow these values, while the JavaScript adapter retains them until its accepted worker job completes. The plugin JSON protocol is unchanged.

Native defaults and editing do not await JavaScript. Components declare their supported native behavior in committed props; business input is queued in order. Native observations have bounded publication custody: enqueueing is not publication, and only the original logical commit acknowledges an observation. Rejection retains its exact value for retry after a real business/source change. Only complete replaceable snapshots may coalesce; command receipts and terminal outcomes remain ordered.

Native editors own their buffers and expose immutable captures at an exact owner and revision. Multiline initialization, edits and local text reads carry complete representable UTF-8 text without a separate per-command or per-buffer size ceiling; transport framing and aggregate resource admission still apply. Single-line fields retain their own policies. Opt-in editor checkpoints persist complete text; native capture and restore use the presenter’s shared byte budget, with restore admission preceding the content read in one database snapshot. Small JSON preferences retain their separate value policy. A mount keeps one native recipe implementation type; changing the implementation requires a new child key/remount. A type mismatch rejects the candidate before changing the committed native owner. Asynchronous mutations retain their original receipt and caller identity. Operation feedback includes the confirmed caller commit when available, so a component can checkpoint its exact history position without searching for it; the position alone proves no business outcome or native edit. Replacing a component waits for its accepted work and native resources to settle; dropping a frame or result observer is insufficient. See `actions`, `operations`, `editor`, `checkpoint` and `publication` for these paths.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
