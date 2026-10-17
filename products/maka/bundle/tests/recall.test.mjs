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
import { test } from 'node:test';
import { contracts } from '../../../../packages/plugin-sdk/src/contracts.ts';
import Ajv from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'light' }).Appearance.value;
const ajv = new Ajv({ strict: false, formats: { uint64: true } });
addFormats(ajv);
const validate = (method) => ajv.compile(contracts.agent.interface.methods[method].input);
for (const target of ['tui'])
  test(`Recall ${target} uses exact editor snapshots, validates date bounds, browses archived sessions and emits original anchors`, async () => {
    const { component } = await import(`../dist/packages/recall/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: { kind: 'submit' } });
    const child = (child, name, value) => event({ kind: 'component', child, name, event: value });
    const finish = (value) =>
      event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
    const form = { query: 'café 中😀', since: '2026-10-01', until: '2026-10-12' };
    const observations = Object.fromEntries(
      Object.entries(form).map(([key, value], index) => [
        key,
        {
          revision: 1,
          snapshot: {
            marked: false,
            text: {
              owner: `00000000-0000-0000-0000-${String(index + 1).padStart(12, '0')}`,
              revision: 1,
              bytes: Buffer.byteLength(value),
            },
          },
          receipt: null,
        },
      ]),
    );
    for (const [key, observation] of Object.entries(observations))
      child('recall-' + key, 'changed', observation);
    raw('find');
    for (const [key, value] of Object.entries(form)) {
      assert.equal(out.actions[0].method, 'read');
      assert.deepEqual(out.actions[0].resources.text.input, {
        kind: 'at_revision',
        owner: observations[key].snapshot.text.owner,
        revision: 1,
      });
      finish({ text: value, metadata: observations[key].snapshot.text });
    }
    const query = out.actions[0];
    assert.equal(query.method, 'recall');
    assert.ok(validate('recall')(query.input));
    assert.deepEqual(query.input.terms, ['café', '中😀']);
    assert.equal(query.input.since, Date.parse('2026-10-01T00:00:00Z'));
    assert.equal(query.input.until, Date.parse('2026-10-13T00:00:00Z') - 1);
    const session = { id: 'original-session', title: 'Original conversation' };
    finish({
      titles: [],
      passages: [
        {
          session,
          anchorMessageId: 'original-message',
          anchorPosition: { sourceSequence: 7, itemIndex: 256 },
          excerpt: 'original café text',
          matchedTerms: ['café'],
        },
      ],
      complete: false,
      gaps: ['source unavailable'],
    });
    raw('open-0');
    assert.deepEqual(out.events, [
      { name: 'selected', value: { session, anchor: { sourceSequence: 7, itemIndex: 256 } } },
    ]);
    raw('scope');
    assert.ok(validate('list')(out.actions[0].input));
    assert.equal(out.actions[0].input.filter, 'all');
    finish({
      sessions: [session],
      revision: 'catalog-fence',
      after: 'original-session',
      before: null,
      needsYouCount: 0,
    });
    event({
      kind: 'component',
      child: 'recall-reading',
      name: 'observed',
      event: {
        readingRevision: 1,
        scroll: { readingRevision: 1, direction: 'down' },
        viewport: { readingRevision: 1, atStart: false, atEnd: true, visibleChildKeys: [] },
        pendingCommand: null,
      },
    });
    assert.deepEqual(out.actions[0].input.position, {
      kind: 'after',
      revision: 'catalog-fence',
      cursor: 'original-session',
    });
    assert.ok(validate('list')(out.actions[0].input));
    finish({
      sessions: [session],
      revision: 'catalog-fence',
      after: null,
      before: null,
      needsYouCount: 0,
    });
    raw('session-original-session');
    raw('find');
    const stale = {
      ...observations.query,
      snapshot: {
        ...observations.query.snapshot,
        text: { ...observations.query.snapshot.text, revision: 2 },
      },
    };
    const key = out.actions[0].key;
    child('recall-query', 'changed', stale);
    event({
      kind: 'action',
      key,
      event: {
        status: 'succeeded',
        value: { text: form.query, metadata: observations.query.snapshot.text },
      },
    });
    assert.equal(out.actions.length, 0);
    assert.equal(out.frame.state.notice, 'recallUnavailable');
  });

test('embedded Recall back closes its picker before leaving and consumes each request once', async () => {
  const { component } = await import('../dist/packages/recall/tui.js');
  const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
  let out = component.mount(props, {});
  assert.deepEqual(out.events.find((event) => event.name === 'chrome').value, {
    title: 'Search conversations',
    backEnabled: true,
  });
  out = component.event(out.frame.state, { kind: 'raw', key: 'scope', event: {} }, props, {});
  assert.equal(out.frame.state.picker, true);
  props.shell.back = 1;
  out = component.update(out.frame.state, props, {});
  assert.equal(out.frame.state.picker, false);
  assert.equal(
    out.events.some((event) => event.name === 'closed'),
    false,
  );
  out = component.update(out.frame.state, props, {});
  assert.equal(
    out.events.some((event) => event.name === 'closed'),
    false,
  );
  props.shell.back = 2;
  out = component.update(out.frame.state, props, {});
  assert.equal(
    out.events.some((event) => event.name === 'closed'),
    true,
  );
  props.open = false;
  props.shell.back = 3;
  out = component.update(out.frame.state, props, {});
  assert.equal(
    out.events.some((event) => event.name === 'closed'),
    false,
  );
  props.open = true;
  out = component.update(out.frame.state, props, {});
  assert.equal(
    out.events.some((event) => event.name === 'closed'),
    false,
  );
});
