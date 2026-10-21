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

-- Remove application content quotas without rewriting existing JSON, revisions,
-- generated identities, uniqueness, strict typing or OAuth completion ordering.

CREATE TABLE pricing_overrides_complete (
    model_key TEXT PRIMARY KEY,
    sort_key BLOB NOT NULL,
    record_json TEXT NOT NULL CHECK(json_valid(record_json))
);
INSERT INTO pricing_overrides_complete (model_key, sort_key, record_json) SELECT model_key, sort_key, record_json FROM pricing_overrides;
DROP TABLE pricing_overrides;
ALTER TABLE pricing_overrides_complete RENAME TO pricing_overrides;

CREATE TABLE access_credentials_complete (
    document TEXT NOT NULL CHECK(json_valid(document)),
    credential_id TEXT GENERATED ALWAYS AS (json_extract(document, '$.credentialId')) STORED NOT NULL UNIQUE,
    credential_hash TEXT GENERATED ALWAYS AS (json_extract(document, '$.credentialHash')) STORED NOT NULL UNIQUE CHECK(length(credential_hash) = 64)
);
INSERT INTO access_credentials_complete (document) SELECT document FROM access_credentials;
DROP TABLE access_credentials;
ALTER TABLE access_credentials_complete RENAME TO access_credentials;

CREATE TABLE runtime_policy_complete (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    document TEXT NOT NULL CHECK(json_valid(document))
);
INSERT INTO runtime_policy_complete (singleton, document) SELECT singleton, document FROM runtime_policy;
DROP TABLE runtime_policy;
ALTER TABLE runtime_policy_complete RENAME TO runtime_policy;

CREATE TABLE oauth_login_receipts_complete (
    completion_order INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id TEXT NOT NULL UNIQUE CHECK(length(attempt_id) >= 1),
    target TEXT NOT NULL,
    method TEXT NOT NULL,
    request_fingerprint TEXT NOT NULL,
    phase TEXT NOT NULL CHECK(json_valid(phase)),
    connection TEXT NOT NULL
);
INSERT INTO oauth_login_receipts_complete (completion_order, attempt_id, target, method, request_fingerprint, phase, connection) SELECT completion_order, attempt_id, target, method, request_fingerprint, phase, connection FROM oauth_login_receipts;
-- Preserve the monotonic AUTOINCREMENT high-water mark even if prior rows were deleted.
INSERT INTO sqlite_sequence(name, seq) SELECT 'oauth_login_receipts_complete', seq FROM sqlite_sequence WHERE name = 'oauth_login_receipts' AND NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name = 'oauth_login_receipts_complete');
UPDATE sqlite_sequence SET seq = max(seq, COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'oauth_login_receipts'), 0)) WHERE name = 'oauth_login_receipts_complete';
DROP TABLE oauth_login_receipts;
ALTER TABLE oauth_login_receipts_complete RENAME TO oauth_login_receipts;

CREATE TABLE plugin_credentials_complete (
    package_id TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    key TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 9007199254740991),
    secret TEXT,
    PRIMARY KEY (package_id, scope_id, key)
) STRICT;
INSERT INTO plugin_credentials_complete (package_id, scope_id, key, revision, secret) SELECT package_id, scope_id, key, revision, secret FROM plugin_credentials;
DROP TABLE plugin_credentials;
ALTER TABLE plugin_credentials_complete RENAME TO plugin_credentials;

CREATE TABLE plugin_authorizations_complete (
    id TEXT PRIMARY KEY NOT NULL,
    package_id TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    document TEXT NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0, 1)),
    UNIQUE(package_id, scope_id, operation_id)
) STRICT;
INSERT INTO plugin_authorizations_complete (id, package_id, scope_id, operation_id, document, revoked) SELECT id, package_id, scope_id, operation_id, document, revoked FROM plugin_authorizations;
DROP TABLE plugin_authorizations;
ALTER TABLE plugin_authorizations_complete RENAME TO plugin_authorizations;
