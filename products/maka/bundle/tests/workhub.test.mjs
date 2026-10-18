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
const appearance = await import('../dist/packages/appearance/tui.js');
const theme = appearance.activate({ scheme: 'dark' }).Appearance.value;
const hub = await import('../dist/packages/workhub-root/tui.js');
const screen = await import('../dist/packages/agent-screen/tui.js');
const values = { appearance: [theme] };
const nodes = (node) =>
  node && typeof node === 'object' ? [node, ...Object.values(node).flatMap(nodes)] : [];
const nativeLanguage = (revision, language) => ({
  revision,
  language,
  resolvedLanguage: language,
  displayName: '',
  assistantTone: '',
  compactionPrompt: 'Default',
  defaultCompactionPrompt: 'Default',
  workspaceInstructions: true,
  incognitoActive: false,
  shellTitles: true,
});

test('WorkHub coalesces a stale language read across save and passes observed policy revisions to both screens', () => {
  hub.activate({ worker: 'worker' });
  const component = hub.component;
  let out = component.mount({}, values);
  const originalLanguage = out.actions.find((action) => action.method === 'assistantPreferences');
  const history = out.actions.find((action) => action.method === 'page');
  const event = (input) => (out = component.event(out.frame.state, input, {}, values));
  const reply = (action, value) =>
    event({
      kind: 'action',
      key: action.key,
      event: { status: 'succeeded', value },
    });
  const child = (name) => nodes(out.frame.view).find((node) => node.key === name);
  assert.deepEqual(
    { binding: originalLanguage.binding, input: originalLanguage.input },
    { binding: 'settings', input: {} },
  );
  reply(history, []);
  reply(
    out.actions.find((action) => action.method === 'coordinator'),
    { kind: 'ready', session: { id: 'coordinator-session' } },
  );
  for (const name of ['worker-agent-screen', 'coordinator-agent-screen'])
    event({
      kind: 'component',
      child: name,
      name: 'languageChanged',
      event: {},
    });
  assert.equal(out.actions.filter((action) => action.method === 'assistantPreferences').length, 0);
  reply(originalLanguage, nativeLanguage(3, 'en'));
  const followup = out.actions.find((action) => action.method === 'assistantPreferences');
  assert.deepEqual(
    { binding: followup.binding, input: followup.input },
    { binding: 'settings', input: {} },
  );
  assert.equal(child('worker-agent-screen').props.languageRevision, 3);
  assert.equal(child('coordinator-agent-screen').props.languageRevision, 3);
  reply(followup, nativeLanguage(17, 'zh-TW'));
  assert.equal(child('worker-agent-screen').props.languageRevision, 17);
  assert.equal(child('coordinator-agent-screen').props.languageRevision, 17);
  assert.equal(
    nodes(out.frame.view).find((node) => node.key === 'workhub-route-tasks').label,
    '任務',
  );
  out = component.update(out.frame.state, {}, values);
  assert.equal(out.actions.filter((action) => action.method === 'assistantPreferences').length, 0);
  event({ kind: 'raw', key: 'unrelated-keypress', event: {} });
  assert.equal(out.actions.filter((action) => action.method === 'assistantPreferences').length, 0);

  screen.activate({});
  const sibling = screen.component;
  let props = { embedded: true, languageRevision: 3 };
  let siblingOut = sibling.mount(props, values);
  const siblingRead = siblingOut.actions.find((action) => action.method === 'assistantPreferences');
  siblingOut = sibling.event(
    siblingOut.frame.state,
    {
      kind: 'action',
      key: siblingRead.key,
      event: { status: 'succeeded', value: nativeLanguage(3, 'en') },
    },
    props,
    values,
  );
  siblingOut = sibling.update(siblingOut.frame.state, props, values);
  assert.equal(
    siblingOut.actions.filter((action) => action.method === 'assistantPreferences').length,
    0,
  );
  props = {
    ...props,
    languageRevision: child('worker-agent-screen').props.languageRevision,
  };
  siblingOut = sibling.update(siblingOut.frame.state, props, values);
  assert.deepEqual(
    siblingOut.actions
      .filter((action) => action.method === 'assistantPreferences')
      .map(({ binding, input }) => ({ binding, input })),
    [{ binding: 'settings', input: {} }],
  );
  const refreshedRead = siblingOut.actions.find(
    (action) => action.method === 'assistantPreferences',
  );
  siblingOut = sibling.event(
    siblingOut.frame.state,
    {
      kind: 'action',
      key: refreshedRead.key,
      event: { status: 'succeeded', value: nativeLanguage(17, 'zh-TW') },
    },
    props,
    values,
  );
  assert.equal(siblingOut.frame.state.language.observed.resolvedLanguage, 'zh-TW');
  siblingOut = sibling.update(siblingOut.frame.state, props, values);
  assert.equal(
    siblingOut.actions.filter((action) => action.method === 'assistantPreferences').length,
    0,
  );
});
