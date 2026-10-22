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

Caller intents, operation dispatch and settlement, pending messages, and eligible canonical appends share one typed writer queue. At most 32 already queued adjacent requests share a FULL commit, without a collection delay or crossing an ordinary writer/control command. Each request keeps its own validation; groups with multiple requests isolate each request with a savepoint, and execution claims and commit notices are released only after successful commit. Dependent producer stages still await the preceding durable receipt. Capability, checkpoint, summary and lineage appends retain their original transaction path.

A statement failure rolls back the entire group unless it is a domain rejection isolated by its own savepoint. Former ordinary writer requests can continue recovery on the same owner after acknowledged rollback and fresh authority validation; pure observation batches retain their stricter retirement policy. Failed commits always return unknown outcomes without execution claims. Reusing a former ordinary writer group after a failed commit additionally requires acknowledged cleanup and zero transaction depth. Savepoint, authority and unconfirmed cleanup failures retire the writer. Single-key and paged plugin-data reads use the independent reader owner and see committed snapshots; dependent reads must still await their preceding write receipt.

Saved application inspection uses `RootOwner::open_existing` and `EventLog::for_application_inspection`. It requires existing private coordination state and the original exclusive Root/database leases, reads committed application facts including retained WAL content, and closes without changing stored members or bytes. It performs no migration or derived initialization; missing state fails without repair. Only application queries on the owned connection belong to this opening. CLI application read and default-update preview share it; applying an update opens a fresh ordinary writer and recomputes the change.

Native UI checkpoints retain complete content under the original persisted deployment and instance, without separate content or accumulated-namespace byte quotas. Reads obtain the content byte length and caller-owned admission before materializing the same record within one SQLite snapshot; the reservation travels with the returned record. Writes preserve source/format identity, revision comparison and empty CAS tombstones. Live memory admission belongs to the caller; filesystem and commit failures remain explicit storage failures.

Original operation contracts are stored once by digest. Operation requests and receipt references carry that identity, so journal reads, caller history and UI delivery do not repeatedly copy service schemas. Reads verify the exact original request fingerprint; admission still validates the complete contract and resource requirements.

The operation journal retains complete admitted requests and returned values without separate request, result or receipt-reference size quotas. Exact retries and reopened readers recover the original outcome and cleanup status.

Message delivery retains prepared content, the editable original and the original submission digest. Its internal records can exceed the public request envelope because they preserve these separate facts; writes and exact message reads do not reapply that envelope to the encoded record. Pending queues have no fixed message-count or aggregate-byte quota. Stop and whole-queue retraction retain complete admitted entries in their transactional receipts, without a separate receipt-size quota. Exact retries and reopened readers return the same original receipt. Use `editable_message` to read one owned source by its Session, Turn and message identity.

Source messages, executor tool calls and generated tool results share the Session's visible-message identity namespace. Admission checks both claim orders against canonical facts, including results synthesized for pending tools at executor completion or invocation termination. Only actual result identities are occupied; a matching suffix alone does not reserve an identity.

Checkpoint reads validate every raw source commitment, including predecessors hidden by the latest summary. Prefix digests bind the scope, ordered sequence/byte-length/raw-byte records, then the high-water suffix; identical selections can share a streamed scan across multiple cuts. Lineage selections and archive request fences remain independently checked. This internal encoding has no legacy compatibility path.

`run_source` authenticates every raw event in one physical Run and retains only its opening, final event and exact boundary. Its digest binds the Run-local event count and original ledger positions and bytes. Raw stream fragmentation does not consume the selected replay context budget; handoff still reserves room for its successor and immediate cancellation before sealing. Continuation validation follows every authenticated source edge to the original Message; owner reads follow explicit successors to the current execution. Both reject cycles and preserve the frozen lineage regardless of its depth.

An open store can reuse a verified checkpoint chain and extend its authenticated prefix within the same selection. Fresh writer confirmations bind that evidence to the reader's pinned snapshot or the writer's locked transaction; protected history mutations, external commits, schema changes and failed writes revoke stale evidence. Selection, admission, settlement and archive validation remain fresh. Opening-only reads use full validation without warming a certificate. These disposable certificates are never persisted or used as authority.

A complete session-visible row metadata directory is committed with canonical facts on the original SQLite writer at FULL durability. History windows seek this directory and reconstruct only selected source bodies; missing body/text caches do not require replaying the Session prefix. Original emitting sequences and row ordinals remain unchanged. Projection failures and global message-ID collisions are recorded without rejecting otherwise accepted execution facts; an indexed failure at or below a requested fence makes that Session history unavailable at that fence. Storage failures still roll back the canonical transaction.

For item-accepting main model requests, completed text parts, sources, tool calls and provider tool results enter history at their accepted observation sequence. Reading later fences or reopening cannot change an earlier fence. The first directory installation performs one blocking, source-ordered rebuild from retained canonical facts and frozen membership, before public Ready. It preserves canonical bytes and records invalid historical sources explicitly. Ordinary reopening uses the installed directory without replaying history; recovering lost metadata is explicit maintenance, unlike an ordinary selected-body cache miss.

First upgrade requires maintenance-first startup with the existing admitted long-lived role, rather than relying on the ordinary launcher's 30-second startup observation window. An uninstalled Root, or a Supervised deployment using its exact admitted executable, can run `host serve --root …`; this starts the real service and its normal recovery, not a metadata-only command. An OnDemand deployment must use its matching admitted executable and existing direct `host candidate --root … --expected-root-id … --startup-attempt-id …` role without launcher-owned stdin. Release the prior service through normal lifecycle coordination; retain deployment mode/executable admission and Root leases. Observe durable initialization and normal service continuation or actual close before ordinary launch. Initialization publishes no Ready before completion and remains transactionally recoverable after interruption. No timeout/progress bypass or automatic foreground prefix fallback is added.

Whole-artifact reads use `with_artifact` with Session or execution-turn scope. Metadata and one local read-only BLOB reader share the original writer's snapshot; the synchronous callback owns its destination, and the reader closes after it returns, before the snapshot is released. This avoids reopening and rereading the payload for each chunk. Other writer jobs wait for that artifact copy, and accepted work retains the original owner even if its waiter disappears. Callers retain their resource budgets, digest checks and cancellation policy. `read_artifact_chunk` serves independent windows; repeated windows do not retain a sequential cursor.

Generated tool images and user uploads retain their complete original artifact bytes through model reconstruction, immutable copies and bundle transfer. Payload length, digest, canonical retention and copied-history mappings remain authoritative. Bundle transfer streams blobs in chunks and retains physical frame-length representation and stalled-I/O cleanup, without an independent whole-bundle content quota.

History, delivery and resource pages retain their byte targets for packing subsequent items, while one complete first item always advances with its original metadata and cursor. History body windows remain bounded UTF-8 transfers. `history_source` takes a body-byte target and returns the complete original metadata for that single chunk; it has no separate JSON-envelope quota. Session scope and frozen source fences remain mandatory.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)
