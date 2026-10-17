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

Checkpoint reads validate every raw source commitment, including predecessors hidden by the latest summary. Prefix digests bind the scope, ordered sequence/byte-length/raw-byte records, then the high-water suffix; identical selections can share a streamed scan across multiple cuts. Lineage selections and archive request fences remain independently checked. This internal encoding has no legacy compatibility path.

An open store can reuse a verified checkpoint chain and extend its authenticated prefix within the same selection. Fresh writer confirmations bind that evidence to the reader's pinned snapshot or the writer's locked transaction; protected history mutations, external commits, schema changes and failed writes revoke stale evidence. Selection, admission, settlement and archive validation remain fresh. Opening-only reads use full validation without warming a certificate. These disposable certificates are never persisted or used as authority.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
