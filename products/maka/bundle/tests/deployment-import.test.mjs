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
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'light' }).Appearance.value;
const source = {
  stateDomain: 'a'.repeat(64),
  deployment: '00000000-0000-0000-0000-000000000001',
  revision: 2,
};
const url = 'https://example.com/footer.maka-plugin';
function observed(revision = 1) {
  return {
    revision,
    snapshot: { text: { owner: 'url-editor', revision, bytes: url.length }, marked: null },
  };
}
for (const target of ['tui']) {
  test(`${target} URL install captures the reviewed link and retains the original receipt`, async () => {
    const { component } = await import(`../dist/packages/deployment/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: { kind: 'activate' } });
    const child = (value) =>
      event({ kind: 'component', child: 'deployment-import-url', name: 'changed', event: value });
    const done = (value) =>
      event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
    done({ source, instances: [], next: null });
    event({
      kind: 'component',
      child: 'settings-checkpoint',
      name: 'bookmark',
      event: { sequence: 0, state: { kind: 'ready', revision: 0, value: null } },
    });
    raw('deployment-import');
    raw('deployment-import-remote');
    child(observed());
    raw('deployment-import-install');
    assert.deepEqual(out.actions[0].resources.text, {
      child: 'deployment-import-url',
      method: 'snapshot',
      input: { kind: 'at_revision', owner: 'url-editor', revision: 1 },
    });
    done({ text: url, metadata: observed().snapshot.text });
    assert.equal(out.frame.state.importer.confirmation.url, url);
    child(observed(2));
    assert.equal(out.frame.state.importer.confirmation, null);
    raw('deployment-import-confirm');
    assert.deepEqual(out.actions, []);
    raw('deployment-import-install');
    done({ text: url, metadata: observed().snapshot.text });
    assert.equal(
      out.frame.state.importer.confirmation,
      null,
      'stale capture metadata cannot confirm a URL',
    );
    raw('deployment-import-install');
    done({ text: url, metadata: observed(2).snapshot.text });
    raw('deployment-import-confirm');
    assert.equal(out.actions[0].method, 'installUrl');
    assert.deepEqual(out.actions[0].input, { expectedRevision: 2, url, sha256: null });
    const key = out.actions[0].key;
    const reference = { operation: 'original-url-install' };
    event({
      kind: 'action',
      key,
      event: { status: 'operation', reply: { reference, receipt: { kind: 'unknown' } } },
    });
    const lookup = out.actions[0];
    assert.equal(lookup.binding, 'receipts');
    assert.deepEqual(lookup.input, reference);
    raw('deployment-import-confirm');
    assert.deepEqual(out.actions, []);
    raw('deployment-import-check');
    assert.equal(out.frame.state.pending.key, lookup.key);
    const installed = {
      source: { ...source, revision: 3 },
      package: { id: 'example.footer', revision: 'b'.repeat(64) },
    };
    event({
      kind: 'action',
      key: lookup.key,
      event: {
        status: 'succeeded',
        value: {
          reference,
          receipt: {
            kind: 'recorded',
            outcome: { kind: 'returned', value: { status: 'committed', value: installed } },
            cleanup: { kind: 'confirmed' },
          },
        },
      },
    });
    assert.deepEqual(out.frame.state.importer.installed, installed);
    assert.equal(out.frame.state.current, null);
  });
}
