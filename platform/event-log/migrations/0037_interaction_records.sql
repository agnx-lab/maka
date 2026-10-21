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

-- Preserve original requests, receipts and grants while removing logical JSON quotas.
CREATE TABLE interaction_requests_new (
    request_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    record_json TEXT NOT NULL
);
INSERT INTO interaction_requests_new SELECT request_id, session_id, created_at, record_json FROM interaction_requests;

CREATE TABLE interaction_outcomes_new (
    request_id TEXT PRIMARY KEY REFERENCES interaction_requests_new(request_id),
    outcome_json TEXT NOT NULL
);
INSERT INTO interaction_outcomes_new SELECT request_id, outcome_json FROM interaction_outcomes;

DROP TABLE interaction_outcomes;
DROP TABLE interaction_requests;
ALTER TABLE interaction_requests_new RENAME TO interaction_requests;
ALTER TABLE interaction_outcomes_new RENAME TO interaction_outcomes;
CREATE INDEX interaction_session_pending ON interaction_requests(session_id, created_at, request_id);
CREATE INDEX interaction_permission_basis ON interaction_requests(
    session_id, json_extract(record_json, '$.request.baseRevision'), created_at, request_id
) WHERE json_extract(record_json, '$.request.kind') = 'permissions';

CREATE TABLE client_capability_session_grants_new (
    authority_key TEXT PRIMARY KEY,
    record_json TEXT NOT NULL
);
INSERT INTO client_capability_session_grants_new SELECT authority_key, record_json FROM client_capability_session_grants;
DROP TABLE client_capability_session_grants;
ALTER TABLE client_capability_session_grants_new RENAME TO client_capability_session_grants;

-- Preserve both child relations while replacing their referenced project table.
CREATE TABLE projects_new (
    id TEXT PRIMARY KEY NOT NULL,
    identity TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL CHECK(length(CAST(name AS BLOB)) >= 1),
    last_used_at INTEGER NOT NULL CHECK(last_used_at BETWEEN 0 AND 9007199254740991),
    archived_at INTEGER CHECK(archived_at BETWEEN 0 AND 9007199254740991)
) STRICT;

INSERT INTO projects_new (id, identity, name, last_used_at, archived_at)
SELECT id, identity, name, last_used_at, archived_at FROM projects;

CREATE TABLE project_identities_new (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects_new(id) ON DELETE CASCADE
) STRICT;

INSERT INTO project_identities_new (id, project_id)
SELECT id, project_id FROM project_identities;

CREATE TABLE project_locations_new (
    project_id TEXT NOT NULL REFERENCES projects_new(id) ON DELETE CASCADE,
    path TEXT NOT NULL CHECK(length(CAST(path AS BLOB)) >= 1),
    is_worktree INTEGER NOT NULL CHECK(is_worktree IN (0, 1)),
    last_used_at INTEGER NOT NULL CHECK(last_used_at BETWEEN 0 AND 9007199254740991),
    PRIMARY KEY(project_id, path)
) STRICT;

INSERT INTO project_locations_new (project_id, path, is_worktree, last_used_at)
SELECT project_id, path, is_worktree, last_used_at FROM project_locations;

DROP TABLE project_locations;
DROP TABLE project_identities;
DROP TABLE projects;
ALTER TABLE projects_new RENAME TO projects;
ALTER TABLE project_identities_new RENAME TO project_identities;
ALTER TABLE project_locations_new RENAME TO project_locations;
CREATE INDEX project_identity_owner ON project_identities(project_id, id);

-- Preserve copied-history provenance while allowing complete artifact metadata.
CREATE TABLE artifacts_new (
    session_id TEXT NOT NULL REFERENCES session_control(id),
    id TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK(created_at >= 0),
    record_json TEXT NOT NULL,
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

CREATE TABLE session_history_artifacts_new (
    session_id TEXT NOT NULL REFERENCES session_history_copies(session_id),
    source_session_id TEXT NOT NULL,
    source_artifact_id TEXT NOT NULL,
    artifact_id TEXT NOT NULL,
    PRIMARY KEY(session_id, source_session_id, source_artifact_id),
    FOREIGN KEY(session_id, artifact_id) REFERENCES artifacts_new(session_id, id)
);

INSERT INTO session_history_artifacts_new
    (session_id, source_session_id, source_artifact_id, artifact_id)
SELECT session_id, source_session_id, source_artifact_id, artifact_id
FROM session_history_artifacts;

DROP TABLE session_history_artifacts;
DROP TABLE artifacts;
ALTER TABLE artifacts_new RENAME TO artifacts;
ALTER TABLE session_history_artifacts_new RENAME TO session_history_artifacts;
CREATE INDEX artifacts_session_order ON artifacts(session_id, created_at DESC, id);
