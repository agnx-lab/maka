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

-- Empty source arrays are valid and require no identity reconstruction.
-- Keep reopening such stores independent of their unrelated canonical history.
CREATE INDEX message_source_openings ON event_log(kind)
WHERE invocation_id IS NOT NULL AND event_json IS NOT NULL
    AND kind = 'invocation_opened'
    AND json_extract(event_json, '$.fact.input.kind') = 'message'
    AND json_array_length(event_json, '$.fact.input.source_messages') > 0;
