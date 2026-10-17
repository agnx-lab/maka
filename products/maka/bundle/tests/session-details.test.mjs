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
const theme = activate({ scheme: 'dark' }).Appearance.value;
const original = {
  id: 'session-a',
  title: 'First title',
  labels: [],
  revision: 1,
  updatedAt: 1,
  archived: false,
  activity: 'idle',
  boundaryRevision: 0,
  permissions: { access: 'workspace_write', requestPermissions: true },
  workspace: { path: '/project', directoryIdentity: 'a'.repeat(64), identity: 'workspace-a' },
};
for (const target of ['tui']) {
  test(`${target} pristine details follow current session revisions while edited drafts retain their target`, async () => {
    const { component } = await import(`../dist/packages/session-details/${target}.js`);
    let props = { theme, locale: 'en', open: true, busy: false, session: original };
    let out = component.mount(props, {});
    const event = (child, observation) => {
      out = component.event(
        out.frame.state,
        { kind: 'component', child: 'details-' + child, name: 'changed', event: observation },
        props,
        {},
      );
    };
    const observed = (name, revision, text, receipt) => ({
      revision,
      snapshot: { text: { owner: name, revision, bytes: Buffer.byteLength(text) }, marked: null },
      ...(receipt ? { receipt } : {}),
    });
    const settleEditors = () => {
      for (const name of ['title', 'labels']) {
        const command = out.frame.state.fields[name].command;
        if (command)
          event(
            name,
            observed(name, command.basedOn + 1, command.edit.text, {
              owner: command.owner,
              sequence: command.sequence,
              outcome: { status: 'applied' },
            }),
          );
      }
    };
    for (const name of ['title', 'labels']) event(name, observed(name, 0, ''));
    settleEditors();
    const update = (session) => {
      props = { ...props, session };
      out = component.update(out.frame.state, props, {});
    };
    update({ ...original, revision: 2, title: 'Updated elsewhere' });
    assert.equal(out.frame.state.target.revision, 2);
    assert.equal(out.frame.state.fields.title.command.edit.text, 'Updated elsewhere');
    settleEditors();
    event('title', observed('title', 99, 'My unsaved title'));
    update({ ...original, revision: 3 });
    assert.equal(
      out.frame.state.target.revision,
      2,
      'a revision change cannot replace an edited draft',
    );
    update({ ...original, id: 'session-b', title: 'Other task' });
    assert.equal(
      out.frame.state.target.id,
      original.id,
      'changing tasks retains the original write target',
    );
    assert.equal(out.frame.state.fields.title.command, null);
    assert.equal(out.frame.state.fields.title.observation.snapshot.text.revision, 99);
  });
}
