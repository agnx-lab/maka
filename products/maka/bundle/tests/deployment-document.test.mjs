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
const { component } = await import('../dist/packages/deployment/tui.js');
const theme = activate({ scheme: 'light' }).Appearance.value;
const source = {
  stateDomain: 'a'.repeat(64),
  deployment: '00000000-0000-0000-0000-000000000001',
  revision: 2,
};
const owner = 'document-editor';
function observed(bytes, revision, receipt = null) {
  return {
    revision,
    snapshot: {
      text: { owner, revision, bytes },
      selection: { start: 0, end: 0 },
      reversed: false,
      marked: null,
    },
    receipt,
  };
}
function nodes(value) {
  if (!value || typeof value !== 'object') return [];
  return [value, ...Object.values(value).flatMap(nodes)];
}
function client() {
  const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
  let out = component.mount(props, {});
  const c = {
    get out() {
      return out;
    },
    get editor() {
      return out.frame.state.document;
    },
    event(input) {
      out = component.event(out.frame.state, input, props, {});
    },
    raw(key) {
      c.event({ kind: 'raw', key, event: { kind: 'activate' } });
    },
    done(value, key = out.actions[0].key) {
      c.event({ kind: 'action', key, event: { status: 'succeeded', value } });
    },
    observe(value) {
      c.event({
        kind: 'component',
        child: 'deployment-document-editor',
        name: 'changed',
        event: value,
      });
    },
    find(key) {
      return nodes(out.frame.view).find((node) => node.key === key);
    },
  };
  c.done({ source, instances: [], next: null });
  c.event({
    kind: 'component',
    child: 'settings-checkpoint',
    name: 'bookmark',
    event: { sequence: 0, state: { kind: 'ready', revision: 0, value: null } },
  });
  c.raw('deployment-advanced');
  return c;
}

test('tui deployment loads and saves a complete multibyte document through native editor ownership', () => {
  const packageRef = { id: 'example.payload', revision: `sha256:${'b'.repeat(64)}` };
  const document = {
    input: {
      instances: {
        payload: {
          key: '00000000-0000-0000-0000-000000000002',
          package: { source: 'pinned', package: packageRef },
          usePackage: 'entry',
          configuration: {
            values: { start: 'SOURCE START', text: '保留😀z'.repeat(300_000), end: 'SOURCE END' },
          },
        },
      },
      environment: { location: 'host', presentation: null },
    },
    remoteExports: [],
    packages: [packageRef],
  };
  const text = JSON.stringify(document);
  const bytes = Buffer.from(text);
  assert.ok(
    bytes.length > 3 * 1024 * 1024,
    'fixture exceeds the former UI and Host aggregate limits',
  );
  const c = client();
  let offset = 0;
  let revision = 0;
  let reconstructed = '';
  while (offset < bytes.length) {
    const read = c.out.actions[0];
    assert.equal(read?.method, 'readDocument');
    assert.deepEqual(read.input, { source, offset });
    let end = Math.min(offset + 65536, bytes.length);
    while (end < bytes.length && (bytes[end] & 0xc0) === 0x80) end--;
    const chunk = {
      source,
      offset,
      totalBytes: bytes.length,
      text: bytes.subarray(offset, end).toString('utf8'),
      next: end === bytes.length ? null : end,
    };
    if (offset === 0) {
      c.done(chunk, 'stale-document-read');
      assert.equal(c.out.frame.state.pending.key, read.key);
    }
    c.done(chunk, read.key);
    if (offset === 0) c.observe(observed(0, 0));
    const command = c.editor.command;
    assert.ok(command, `a supported complete document must initialize (phase=${c.editor.phase})`);
    assert.equal(command.owner, owner);
    assert.equal(command.basedOn, revision);
    assert.equal(command.sequence, revision + 1);
    assert.equal(command.edit.kind, offset === 0 ? 'reset' : 'append');
    assert.ok(command.edit.text === chunk.text, 'the native command preserves every page byte');
    reconstructed =
      command.edit.kind === 'reset' ? command.edit.text : reconstructed + command.edit.text;
    assert.equal(c.find('deployment-document-editor').props.readOnly, true);
    if (offset === 0) {
      c.observe(
        observed(0, revision, {
          owner: 'foreign-editor',
          sequence: command.sequence,
          revision,
          outcome: { status: 'applied' },
        }),
      );
      assert.equal(c.editor.offset, 0, 'a foreign receipt cannot advance the source');
      assert.equal(c.editor.command.sequence, command.sequence);
    }
    revision++;
    c.observe(
      observed(end, revision, {
        owner,
        sequence: command.sequence,
        revision,
        outcome: { status: 'applied' },
      }),
    );
    offset = end;
  }
  assert.ok(reconstructed === text, 'the complete original UTF-8 source reaches the native editor');
  assert.equal(c.editor.phase, 'ready');
  assert.equal(c.editor.observation.snapshot.text.bytes, bytes.length);
  assert.equal(c.find('deployment-document-editor').props.readOnly, false);
  assert.equal(c.find('deployment-document-save').disabled, false);
  assert.equal(c.find('deployment-document-too-large'), undefined);

  document.input.instances.payload.configuration.values.end = 'EDITED COMPLETE END';
  const edited = JSON.stringify(document);
  revision++;
  const editedObservation = observed(Buffer.byteLength(edited), revision);
  c.observe(editedObservation);
  c.raw('deployment-document-save');
  assert.deepEqual(c.editor.confirmation, editedObservation);
  assert.equal(c.find('deployment-document-editor').props.readOnly, true);
  c.raw('deployment-document-confirm');
  const save = c.out.actions[0];
  assert.equal(save.binding, 'management');
  assert.equal(save.method, 'replace');
  assert.deepEqual(save.input, { expectedRevision: source.revision });
  assert.deepEqual(save.resources, {
    document: {
      child: 'deployment-document-editor',
      method: 'snapshot',
      input: { kind: 'at_revision', owner, revision },
    },
  });
  const next = { ...source, revision: 3 };
  c.event({
    kind: 'action',
    key: save.key,
    event: {
      status: 'operation',
      reply: {
        reference: { operation: 'original-document-replace' },
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'returned', value: { status: 'committed', value: next } },
          cleanup: { kind: 'confirmed' },
        },
      },
    },
  });
  assert.deepEqual(c.editor.source, next);
  assert.equal(c.out.frame.state.current, null);
  assert.equal(c.out.actions[0].method, 'instances');
  c.done({ source: next, instances: [], next: null });
  assert.equal(c.editor.observation.snapshot.text.bytes, Buffer.byteLength(edited));
});

for (const invalid of ['foreign source', 'noncontiguous offset']) {
  test(`tui deployment rejects a document page with ${invalid}`, () => {
    const c = client();
    c.done({
      source: invalid === 'foreign source' ? { ...source, revision: 99 } : source,
      offset: invalid === 'noncontiguous offset' ? 1 : 0,
      totalBytes: 3,
      text: '{}\n',
      next: null,
    });
    assert.equal(c.editor.phase, 'failed');
    assert.equal(c.editor.command, null);
    assert.equal(c.find('deployment-document-save').disabled, true);
    c.raw('deployment-document-save');
    assert.equal(c.editor.confirmation, null);
    assert.deepEqual(c.out.actions, []);
  });
}
