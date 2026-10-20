/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

-- Preserve copied-history provenance while replacing its referenced artifact table.
CREATE TABLE session_history_artifacts_saved AS SELECT * FROM session_history_artifacts;
DROP TABLE session_history_artifacts;

CREATE TABLE artifacts_new (
    session_id TEXT NOT NULL REFERENCES session_control(id),
    id TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK(created_at >= 0),
    record_json TEXT NOT NULL CHECK(length(CAST(record_json AS BLOB)) <= 16384),
    content_sha256 TEXT NOT NULL,
    payload BLOB NOT NULL,
    canonical_at INTEGER CHECK(canonical_at IS NULL OR canonical_at > 0),
    PRIMARY KEY (session_id, id),
    CHECK(json_extract(record_json, '$.sessionId') = session_id),
    CHECK(json_extract(record_json, '$.id') = id),
    CHECK(json_extract(record_json, '$.createdAt') = created_at),
    CHECK(json_extract(record_json, '$.sizeBytes') = length(payload))
);

INSERT INTO artifacts_new
    (session_id, id, created_at, record_json, content_sha256, payload, canonical_at)
SELECT session_id, id, created_at, record_json, content_sha256, payload, canonical_at
FROM artifacts;
DROP TABLE artifacts;
ALTER TABLE artifacts_new RENAME TO artifacts;
CREATE INDEX artifacts_session_order ON artifacts(session_id, created_at DESC, id);

CREATE TABLE session_history_artifacts (
    session_id TEXT NOT NULL REFERENCES session_history_copies(session_id),
    source_session_id TEXT NOT NULL,
    source_artifact_id TEXT NOT NULL,
    artifact_id TEXT NOT NULL,
    PRIMARY KEY(session_id, source_session_id, source_artifact_id),
    FOREIGN KEY(session_id, artifact_id) REFERENCES artifacts(session_id, id)
);

INSERT INTO session_history_artifacts
    (session_id, source_session_id, source_artifact_id, artifact_id)
SELECT session_id, source_session_id, source_artifact_id, artifact_id
FROM session_history_artifacts_saved;
DROP TABLE session_history_artifacts_saved;
