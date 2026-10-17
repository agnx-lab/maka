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

import assert from 'node:assert/strict';
import test from 'node:test';
import {
  compareRowPositions,
  rowPositionKey,
} from '../../products/maka/desktop-shell/app/history-position.js';

test('rows beyond ordinal255 stay before the next fact and have distinct lookup keys', () => {
  const rows = [
    { position: { sourceSequence: 271, itemIndex: 0 }, text: 'next fact' },
    { position: { sourceSequence: 270, itemIndex: 256 }, text: 'same fact later row' },
    { position: { sourceSequence: 270, itemIndex: 255 }, text: 'same fact earlier row' },
  ];
  rows.sort((left, right) => compareRowPositions(left.position, right.position));
  assert.deepEqual(
    rows.map((row) => row.text),
    ['same fact earlier row', 'same fact later row', 'next fact'],
  );
  const lookup = new Map(rows.map((row) => [rowPositionKey(row.position), row.text]));
  assert.deepEqual([...lookup.keys()], ['270/255', '270/256', '271/0']);
  assert.equal(
    lookup.get(rowPositionKey({ sourceSequence: 270, itemIndex: 256 })),
    'same fact later row',
  );
  assert.equal(
    compareRowPositions(
      { sourceSequence: 270, itemIndex: 256 },
      { sourceSequence: 270, itemIndex: 256 },
    ),
    0,
  );
});
