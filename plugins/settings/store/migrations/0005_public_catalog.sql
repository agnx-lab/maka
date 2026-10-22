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

CREATE TABLE public_catalog (
    source TEXT PRIMARY KEY NOT NULL CHECK(source IN ('models_dev', 'openrouter')),
    body TEXT NOT NULL CHECK(json_valid(body)),
    etag TEXT,
    last_modified TEXT,
    checked_at INTEGER NOT NULL
) STRICT;

-- Previous discovery merged bundled defaults into these observations. Retire
-- that cache once; selections, overrides, connection revisions and secrets stay.
-- Optional fields use absence, not JSON null, in ConnectionCatalogEntry.
UPDATE connections
SET document = json_remove(
    json_set(document, '$.models', json('[]')),
    '$.modelSource', '$.modelsFetchedAt'
)
WHERE json_array_length(document, '$.models') > 0
   OR json_type(document, '$.modelSource') IS NOT NULL
   OR json_type(document, '$.modelsFetchedAt') IS NOT NULL;

-- Match catalog::next_revision's integer domain and fail the migration rather
-- than publishing changed observations under an exhausted catalog revision.
UPDATE connection_catalog
SET revision = CASE WHEN revision < 9007199254740991 THEN revision + 1 END
WHERE changes() > 0;
