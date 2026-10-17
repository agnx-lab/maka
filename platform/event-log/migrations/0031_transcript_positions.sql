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

-- Transcript positions are disposable locators derived from the original emitting fact.
DROP TABLE transcript_text;
DROP TABLE transcript_rows;
DROP TABLE transcript_progress;

CREATE TABLE transcript_rows (
    session_id TEXT NOT NULL,
    source_sequence INTEGER NOT NULL CHECK(source_sequence >= 1),
    item_index INTEGER NOT NULL CHECK(item_index >= 0),
    turn_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    payload BLOB NOT NULL,
    digest TEXT NOT NULL,
    total_bytes INTEGER NOT NULL CHECK(total_bytes > 0),
    PRIMARY KEY(session_id, source_sequence, item_index),
    UNIQUE(session_id, message_id)
);
CREATE TABLE transcript_text (
    session_id TEXT NOT NULL,
    source_sequence INTEGER NOT NULL CHECK(source_sequence >= 1),
    item_index INTEGER NOT NULL CHECK(item_index >= 0),
    field TEXT NOT NULL CHECK(field IN ('body', 'thinking')),
    timestamp INTEGER NOT NULL,
    role TEXT NOT NULL,
    body BLOB NOT NULL,
    PRIMARY KEY(session_id, source_sequence, item_index, field)
);
CREATE TABLE transcript_progress (
    session_id TEXT PRIMARY KEY,
    through_sequence INTEGER NOT NULL CHECK(through_sequence >= 0)
);
CREATE INDEX transcript_session_turn
    ON transcript_rows(session_id, turn_id, source_sequence, item_index);
CREATE INDEX transcript_tool_results ON transcript_rows (
    session_id, turn_id, json_extract(payload, '$.toolUseId'), source_sequence, item_index
) WHERE json_extract(payload, '$.type') = 'tool_result';
CREATE INDEX transcript_tool_activity ON transcript_rows (
    session_id, turn_id, json_extract(payload, '$.toolUseId'), source_sequence, item_index
) WHERE json_extract(payload, '$.type') = 'tool_activity';
CREATE INDEX transcript_turn_state
    ON transcript_rows(session_id, turn_id, source_sequence, item_index)
    WHERE json_extract(payload, '$.type') = 'turn_state';
CREATE INDEX transcript_user_prompt ON transcript_rows(session_id, source_sequence, item_index)
    WHERE json_extract(payload, '$.type') = 'user';
