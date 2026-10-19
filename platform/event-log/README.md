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

# maka-event-log

[简体中文](README.zh-CN.md)

Owns the SQLite event store and State Root lease. It persists application sources and packages, operation receipts, sessions, turns and recovery records. Start with `root` for exclusive ownership, `applications` for committed compositions, and `recovery` for interrupted work. Opening a Root establishes the durable owner before services activate; plugins keep their own policy while this crate records canonical facts.

Original operation contracts are stored once by digest. Operation requests and receipt references carry that identity, so journal reads, caller history and UI delivery do not repeatedly copy service schemas. Reads verify the exact original request fingerprint; admission still validates the complete contract and resource requirements.

Message delivery retains prepared content, the editable original and the original submission digest. Its internal records can exceed the public request envelope because they preserve these separate facts; writes and exact message reads do not reapply that envelope to the encoded record. Queue aggregate budgets and caller delivery budgets remain separate. Use `editable_message` to read one owned source by its Session, Turn and message identity.

Checkpoint reads validate every raw source commitment, including predecessors hidden by the latest summary. Prefix digests bind the scope, ordered sequence/byte-length/raw-byte records, then the high-water suffix; identical selections can share a streamed scan across multiple cuts. Lineage selections and archive request fences remain independently checked. This internal encoding has no legacy compatibility path.

An open store can reuse a verified checkpoint chain and extend its authenticated prefix within the same selection. Fresh writer confirmations bind that evidence to the reader's pinned snapshot or the writer's locked transaction; protected history mutations, external commits, schema changes and failed writes revoke stale evidence. Selection, admission, settlement and archive validation remain fresh. Opening-only reads use full validation without warming a certificate. These disposable certificates are never persisted or used as authority.

Transcript preparation selects ordered canonical fact identities and reads each fact within the same transaction. It has no aggregate evidence count or byte cutoff; projection still checks individual content and history pages keep their delivery budgets. Derived rows and progress commit together. Each bounded batch runs on the original SQLite worker while the store owner awaits its result; independent readers can advance, while other writer jobs wait for the batch. Replay memory can grow with the selected step, and accepted preparation work drains under the original store owner.

For item-accepting main model requests, completed text parts, sources, tool calls and provider tool results enter history at their accepted observation sequence. Preparing a later fence or reopening the store must not change an earlier fence. Upgrades invalidate disposable transcript rows, text and progress when their projection rules change; canonical facts remain intact.

Whole-artifact reads use `with_artifact` with Session or execution-turn scope. Metadata and one local read-only BLOB reader share the original writer's snapshot; the synchronous callback owns its destination, and the reader closes after it returns, before the snapshot is released. This avoids reopening and rereading the payload for each chunk. Other writer jobs wait for that artifact copy, and accepted work retains the original owner even if its waiter disappears. Callers retain their resource budgets, digest checks and cancellation policy. `read_artifact_chunk` serves independent windows; repeated windows do not retain a sequential cursor.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
