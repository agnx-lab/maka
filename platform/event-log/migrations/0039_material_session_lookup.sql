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

-- Share the live-material key without changing inherited history ownership.
DROP VIEW session_history_events;
CREATE VIEW session_history_events AS
    SELECT e.event_session AS owner_session_id,
           0 AS inherited, NULL AS archive_sequence, e.* FROM runtime_events e
    UNION ALL
    SELECT h.session_id, 1, h.archive_sequence, e.*
    FROM session_history_members h JOIN runtime_events e ON e.sequence = h.sequence;

DROP INDEX session_event_sequence;
