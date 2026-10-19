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

-- Complete metadata, independent of optional payload/text caches.
DELETE FROM transcript_text;
DELETE FROM transcript_rows;
DROP TABLE transcript_progress;
DROP INDEX transcript_session_turn;
DROP INDEX transcript_tool_results;
DROP INDEX transcript_tool_activity;
DROP INDEX transcript_turn_state;
DROP INDEX transcript_user_prompt;
CREATE TABLE transcript_directory (
 session_id TEXT NOT NULL, source_sequence INTEGER NOT NULL CHECK(source_sequence >= 1),
 item_index INTEGER NOT NULL CHECK(item_index >= 0), turn_id TEXT NOT NULL, message_id TEXT NOT NULL,
 timestamp INTEGER NOT NULL, role TEXT NOT NULL, has_thinking INTEGER NOT NULL CHECK(has_thinking IN (0,1)),
 user_origin TEXT, state_status TEXT, tool_use_id TEXT, tool_name TEXT, tool_origin TEXT,
 parent_tool_call_id TEXT, parent_operation_id TEXT, has_args INTEGER, result_valid INTEGER,
 settlement TEXT, is_error INTEGER, content_kind TEXT, tool_title TEXT, presentation TEXT, duration_ms INTEGER,
 PRIMARY KEY(session_id,source_sequence,item_index), UNIQUE(session_id,message_id)
);
CREATE INDEX directory_turn ON transcript_directory(session_id,turn_id,source_sequence,item_index);
CREATE INDEX directory_results ON transcript_directory(session_id,turn_id,tool_use_id,source_sequence,item_index) WHERE role='tool_result';
CREATE INDEX directory_activity ON transcript_directory(session_id,turn_id,tool_use_id,source_sequence,item_index) WHERE role='tool_activity';
CREATE INDEX directory_state ON transcript_directory(session_id,turn_id,source_sequence,item_index) WHERE role='turn_state';
CREATE INDEX directory_user ON transcript_directory(session_id,source_sequence,item_index) WHERE role='user';
CREATE TABLE transcript_failures (
 session_id TEXT NOT NULL, source_sequence INTEGER NOT NULL, category TEXT NOT NULL,
 message_id TEXT, claimant_source_sequence INTEGER, claimant_item_index INTEGER, attempted_item_index INTEGER, reason TEXT,
 PRIMARY KEY(session_id,source_sequence)
);
-- Generic schema consumers do not certify public directory completeness.
CREATE TABLE transcript_directory_installed (singleton INTEGER PRIMARY KEY CHECK(singleton=1));
