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
const identity = {
  packageId: 'maka.codex',
  entryId: 'maka.codex',
  scope: 'profile',
  name: 'chatgpt',
};
const provider = {
  identity,
  descriptor: {
    label: 'ChatGPT Subscription',
    customEndpoint: false,
    configurationDefaults: { baseUrl: 'https://chatgpt.com/backend-api/codex' },
    configurationSchema: {},
    anonymous: false,
    discovery: true,
    authentication: [
      { id: 'chatgpt', label: 'Sign in with ChatGPT', interactive: true, inputSchema: {} },
    ],
  },
};
function summary(provider) {
  return {
    identity: provider.identity,
    label: provider.descriptor.label,
    authentication: provider.descriptor.authentication.map(({ label, interactive }) => ({
      label,
      interactive,
    })),
  };
}
const target = {
  connectionId: 'connection-a',
  revision: 7,
  slug: 'chatgpt',
  provider: identity,
  configuration: provider.descriptor.configurationDefaults,
};
async function client(name, platform, extra = {}) {
  const { component } = await import(`../dist/packages/${name}/${platform}.js`);
  const props = {
    theme,
    locale: 'en',
    shell: { back: 0 },
    connection: null,
    catalogRevision: 3,
    ...extra,
  };
  let out = component.mount(props, {});
  return {
    get out() {
      return out;
    },
    get state() {
      return out.frame.state;
    },
    event(input) {
      out = component.event(out.frame.state, input, props, {});
      return this;
    },
    update(next) {
      Object.assign(props, next);
      out = component.update(out.frame.state, props, {});
      return this;
    },
    raw(key, kind = 'submit') {
      return this.event({ kind: 'raw', key, event: { kind } });
    },
    reply(action, value) {
      assert.ok(action);
      return this.event({ kind: 'action', key: action.key, event: { status: 'succeeded', value } });
    },
    action(method) {
      return out.actions.find((a) => a.method === method);
    },
    query(child, revision, text) {
      return this.event({
        kind: 'component',
        child,
        name: 'changed',
        event: {
          revision,
          snapshot: {
            text: { owner: child, revision, bytes: Buffer.byteLength(text) },
            marked: null,
          },
        },
      });
    },
  };
}
for (const platform of ['tui']) {
  test(`${platform} saved model restoration ignores late sessions and never substitutes the global default`, async () => {
    const preferred = { connectionId: 'saved', modelId: 'original', thinkingLevel: 'high' };
    const c = await client('models', platform, {
      selection: null,
      selectionKey: 'session-a',
      preferredModel: preferred,
      catalogEpoch: 0,
      openRequest: null,
      busy: false,
    });
    const first = c.action('resolveModel');
    c.reply(c.action('models'), {
      kind: 'ready',
      revision: 1,
      choices: [],
      next: null,
      defaultModel: {
        kind: 'ready',
        choice: {
          selection: { connectionId: 'other', revision: 1, modelId: 'default' },
          label: 'Default',
          thinkingLevels: [],
        },
      },
    });
    assert.equal(
      c.out.events.some((event) => event.name === 'selected'),
      false,
    );
    c.update({ selectionKey: 'session-b' });
    const second = c.action('resolveModel');
    const choice = {
      selection: { connectionId: 'saved', revision: 2, modelId: 'original' },
      label: 'Original',
      thinkingLevels: ['high'],
    };
    c.reply(first, { kind: 'ready', choice });
    assert.equal(
      c.out.events.some((event) => event.name === 'resolved'),
      false,
    );
    c.reply(second, { kind: 'ready', choice });
    assert.deepEqual(c.out.events.find((event) => event.name === 'resolved').value, {
      selectionKey: 'session-b',
      selection: { choice, thinkingLevel: 'high' },
    });
    c.update({ selection: { choice, thinkingLevel: 'high' }, catalogEpoch: 1 });
    c.reply(c.action('resolveModel'), { kind: 'unavailable', target: preferred });
    assert.equal(c.out.events.find((event) => event.name === 'resolved').value.selection, null);
    c.update({ selection: null });
    assert.match(JSON.stringify(c.out.frame.view), /Saved model unavailable/u);
    assert.equal(
      c.out.events.some((event) => event.name === 'selected'),
      false,
    );
  });
  test(`${platform} model and thinking edits stay open and survive navigation until applied`, async () => {
    const choice = (id) => ({
      selection: { connectionId: 'a', revision: 1, modelId: id },
      connectionName: 'A',
      label: id,
      thinkingLevels: ['low', 'high'],
      defaultThinkingLevel: 'low',
    });
    const a = choice('a'),
      b = choice('b');
    const selection = { choice: a, thinkingLevel: 'high' };
    const c = await client('models', platform, {
      catalogEpoch: 0,
      openRequest: null,
      selection,
      busy: false,
    });
    const page = (choices) => ({
      kind: 'ready',
      revision: 3,
      choices,
      defaultModel: { kind: 'none' },
      next: null,
    });
    c.reply(c.action('models'), page([a, b]));
    c.query('model-query', 1, '').raw('toggle');
    const key = (id) => c.state.choiceKeys[JSON.stringify(['a', id])];
    c.raw(key('b'));
    assert.equal(c.state.open, true, 'clicking a model leaves its thinking control available');
    assert.equal(c.action('read'), undefined, 'row selection does not commit');
    c.raw('thinking-low');
    c.raw(key('a'), 'activate');
    assert.equal(c.state.draft.thinkingLevel, 'high');
    c.raw(key('b'), 'activate');
    assert.equal(c.state.draft.thinkingLevel, 'low');
    c.query('model-query', 2, 'a');
    c.reply(c.action('read'), { text: 'a' });
    c.reply(c.action('models'), page([a]));
    assert.equal(c.state.draft.thinkingLevel, 'high', 'search preserves the applied effort');
    c.query('model-query', 3, '');
    c.reply(c.action('models'), page([a, b]));
    c.raw(key('b')).raw('apply');
    c.reply(c.action('read'), { text: '' });
    const applied = c.out.events.find((event) => event.name === 'selected').value;
    assert.equal(applied.thinkingLevel, 'low');
    assert.equal(applied.choice.selection.modelId, 'b');
    assert.equal(c.state.open, false);
    c.update({ selection: applied }).raw('toggle');
    c.raw(key('a'), 'activate').raw(key('b'), 'activate');
    assert.equal(
      c.state.draft.thinkingLevel,
      'low',
      'reopened selection survives keyboard navigation',
    );
    c.raw('thinking-high').raw('close').raw('toggle');
    assert.equal(c.state.draft.thinkingLevel, 'low', 'cancel discards unapplied edits');
    c.raw('thinking-default').raw(key('a'), 'activate').raw(key('b'), 'activate');
    assert.equal(c.state.draft.thinkingLevel, null, 'explicit default survives navigation');
  });
  test(`${platform} provider search Enter selects its visible result and exposes anonymous endpoints`, async () => {
    const c = await client('connection-form', platform);
    const anonymous = {
      ...provider,
      descriptor: { ...provider.descriptor, anonymous: true, authentication: [] },
    };
    c.reply(c.action('providers'), { providers: [summary(anonymous)], next: null });
    c.event({ kind: 'component', child: 'provider-query', name: 'submitted', event: {} });
    assert.deepEqual(c.action('provider').input, identity);
    c.reply(c.action('provider'), anonymous);
    assert.deepEqual(c.state.provider, anonymous);
    assert.equal(c.state.fields.endpoint.enabled, true);
  });
  test(`${platform} provider search scans all pages and ignores stale editor reads`, async () => {
    const c = await client('connection-form', platform);
    c.reply(c.action('providers'), {
      providers: [
        summary({
          ...provider,
          identity: { ...identity, name: 'other' },
          descriptor: { ...provider.descriptor, label: 'Other', authentication: [] },
        }),
      ],
      next: 1,
    });
    const second = c.action('providers');
    assert.equal(second.input.offset, 1);
    c.query('provider-query', 1, 'other');
    const old = c.action('read');
    c.query('provider-query', 2, 'oauth');
    c.reply(old, { metadata: { owner: 'provider-query', revision: 1, bytes: 5 }, text: 'other' });
    const latest = c.action('read');
    assert.equal(latest.resources.text.input.revision, 2);
    c.reply(latest, {
      metadata: { owner: 'provider-query', revision: 2, bytes: 5 },
      text: 'oauth',
    });
    c.reply(second, { providers: [summary(provider)], next: null });
    assert.equal(c.state.providers.length, 2);
    assert.equal(c.state.search.query, 'oauth');
    {
      c.raw('provider-1', 'activate');
      assert.equal(c.state.provider, null);
    }
    c.raw('provider-1');
    assert.deepEqual(c.action('provider').input, identity);
    c.reply(c.action('provider'), provider);
    assert.deepEqual(c.state.provider.identity, identity);
    assert.equal(c.state.fields.endpoint.enabled, false);
  });
  test(`${platform} a visible provider stays clickable while search or catalog reads are pending`, async () => {
    const c = await client('connection-form', platform);
    c.reply(c.action('providers'), { providers: [summary(provider)], next: 1 });
    const next = c.action('providers');
    c.query('provider-query', 1, 'chatgpt');
    assert.ok(c.action('read'));
    c.raw('provider-0');
    const selected = c.action('provider');
    assert.deepEqual(selected.input, identity);
    c.reply(next, { providers: [summary(provider)], next: 2 });
    assert.equal(c.action('providers'), undefined, 'selection stops unnecessary catalog scanning');
    c.reply(selected, provider);
    assert.deepEqual(c.state.provider.identity, identity);
  });
  test(`${platform} provider detail keeps exact identity and ignores superseded selections`, async () => {
    const c = await client('connection-form', platform);
    const other = {
      identity: {
        ...identity,
        packageId: 'other.package',
        entryId: 'other.entry',
      },
      descriptor: {
        ...provider.descriptor,
        label: 'Other subscription',
        customEndpoint: true,
        configurationDefaults: {
          baseUrl: 'https://other.example/api',
          mode: 'custom',
        },
        configurationSchema: {
          type: 'object',
          properties: { mode: { type: 'string' } },
        },
      },
    };
    c.reply(c.action('providers'), {
      providers: [summary(provider), summary(other)],
      next: null,
    });
    c.raw('provider-0');
    const first = c.action('provider');
    assert.equal(first.binding, 'authentication');
    c.raw('provider-1');
    const second = c.action('provider');
    assert.deepEqual(second.input, other.identity);
    c.reply(first, provider);
    assert.equal(c.state.provider, null);
    c.reply(second, provider);
    assert.equal(
      c.state.provider,
      null,
      'a same-name provider from another package cannot substitute',
    );
    assert.equal(c.state.providersFailed, true);
    c.raw('provider-1');
    c.reply(c.action('provider'), other);
    assert.deepEqual(c.state.provider, other);
    assert.equal(c.state.fields.endpoint.enabled, true);
  });
  test(`${platform} provider detail cannot initialize a changed connection form`, async () => {
    const c = await client('connection-form', platform);
    c.reply(c.action('providers'), {
      providers: [summary(provider)],
      next: null,
    });
    c.raw('provider-0');
    const selected = c.action('provider');
    c.update({
      connection: {
        kind: 'ready',
        target,
        name: 'Changed',
        enabled: true,
        enabledModelCount: 0,
      },
    });
    c.reply(selected, provider);
    assert.equal(c.state.provider, null);
    assert.equal(c.state.fields.name.enabled, false);
    assert.match(JSON.stringify(c.out.frame.view), /changed/u);
  });
  test(`${platform} connections append one catalog revision and restart changed scans`, async () => {
    const c = await client('connections', platform);
    const row = {
      basis: { connectionId: 'a', revision: 1 },
      name: 'Alpha',
      slug: 'alpha',
      provider: identity,
      editable: true,
      enabled: true,
      enabledModelCount: 1,
    };
    c.reply(c.action('connections'), {
      kind: 'ready',
      revision: 3,
      connections: [row],
      next: 16,
      defaultTarget: null,
    });
    assert.deepEqual(c.action('connections').input, { kind: 'continue', revision: 3, offset: 16 });
    c.reply(c.action('connections'), {
      kind: 'ready',
      revision: 3,
      connections: [{ ...row, basis: { connectionId: 'b', revision: 2 }, name: 'Beta' }],
      next: null,
      defaultTarget: null,
    });
    assert.equal(c.state.connections.length, 2);
    c.query('connection-query', 1, 'Beta');
    c.reply(c.action('read'), {
      metadata: { owner: 'connection-query', revision: 1, bytes: 4 },
      text: 'Beta',
    });
    assert.equal(c.state.selected, 'b');
    c.raw('refresh');
    c.reply(c.action('connections'), {
      kind: 'ready',
      revision: 4,
      connections: [row],
      next: 16,
      defaultTarget: null,
    });
    assert.equal(
      c.state.selected,
      null,
      'a filtered-out first page must not select another connection',
    );
    c.reply(c.action('connections'), { kind: 'changed', revision: 5 });
    assert.deepEqual(c.action('connections').input, { kind: 'start' });
    assert.equal(c.state.connections.length, 0);
  });
  test(`${platform} new connection goes directly to sign-in and rows expose it during paging`, async () => {
    const c = await client('connections', platform);
    c.reply(c.action('connections'), {
      kind: 'ready',
      revision: 3,
      connections: [],
      next: null,
      defaultTarget: null,
    });
    c.raw('create');
    c.event({
      kind: 'component',
      child: 'form-1',
      name: 'changed',
      event: { created: { connectionId: target.connectionId, revision: 7 } },
    });
    c.event({ kind: 'component', child: 'form-1', name: 'closed', event: {} });
    assert.deepEqual(c.action('connection').input, {
      connectionId: target.connectionId,
      revision: 7,
    });
    c.reply(c.action('connection'), {
      kind: 'ready',
      target,
      name: 'ChatGPT',
      enabled: true,
      enabledModelCount: 0,
    });
    assert.equal(c.state.route, 'credential');
    c.event({ kind: 'component', child: 'credential-2', name: 'closed', event: {} });
    c.reply(c.action('connections'), {
      kind: 'ready',
      revision: 4,
      connections: [
        {
          basis: { connectionId: target.connectionId, revision: 7 },
          name: 'ChatGPT',
          slug: 'chatgpt',
          provider: identity,
          editable: true,
          enabled: true,
          enabledModelCount: 0,
        },
      ],
      next: 16,
      defaultTarget: null,
    });
    const background = c.action('connections');
    c.raw('select-' + target.connectionId);
    const detail = c.action('connection');
    assert.ok(detail, 'loaded rows remain actionable during the next page');
    c.reply(background, { kind: 'changed', revision: 5 });
    c.reply(detail, {
      kind: 'ready',
      target,
      name: 'ChatGPT',
      enabled: true,
      enabledModelCount: 0,
    });
    assert.equal(c.state.route, 'credential');
  });
  test(`${platform} provider selection generates an unused ID and leaves technical fields optional`, async () => {
    const c = await client('connection-form', platform, {
      existingSlugs: ['chatgpt', 'chatgpt-2'],
    });
    c.reply(c.action('providers'), { providers: [summary(provider)], next: null });
    c.raw('provider-0');
    c.reply(c.action('provider'), provider);
    assert.equal(c.state.defaultSlug, 'chatgpt-3');
    assert.equal(c.state.fields.name.enabled, true);
    assert.equal(c.state.fields.slug.enabled, true);
    assert.equal(c.state.fields.models.enabled, false);
    assert.equal(c.state.advanced, false);
  });
  test(`${platform} API key save survives discovery failure and retries only model loading`, async () => {
    const apiProvider = {
      ...provider,
      descriptor: {
        ...provider.descriptor,
        authentication: [{ id: 'api-key', interactive: false, label: 'API key', inputSchema: {} }],
      },
    };
    const c = await client('credential', platform, { target });
    const status = c.action('credentialStatus'),
      definition = c.action('provider');
    const locator = { scope: 'connection', connectionId: target.connectionId, kind: 'provider' };
    c.reply(definition, apiProvider);
    c.reply(status, {
      kind: 'status',
      status: { locator, configured: false, credentialId: null, revision: null },
    });
    c.query('secret', 1, 'test-secret');
    c.raw('save');
    const save = c.action('saveCredential');
    const settle = (action, value) =>
      c.event({
        kind: 'action',
        key: action.key,
        event: {
          status: 'operation',
          reply: {
            reference: { operation: action.key },
            receipt: {
              kind: 'recorded',
              outcome: { kind: 'returned', value },
              cleanup: { kind: 'confirmed' },
            },
          },
        },
      });
    settle(save, {
      kind: 'saved',
      connection: { connectionId: target.connectionId, slug: target.slug, provider: identity },
    });
    const reset = c.state.command;
    c.event({
      kind: 'component',
      child: 'secret',
      name: 'changed',
      event: {
        revision: 2,
        snapshot: { text: { owner: 'secret', revision: 2, bytes: 0 }, marked: null },
        receipt: { owner: reset.owner, sequence: reset.sequence, outcome: { status: 'applied' } },
      },
    });
    const next = { ...target, revision: target.revision + 1 };
    c.reply(c.action('connection'), {
      kind: 'stale',
      actual: { connectionId: target.connectionId, revision: next.revision },
    });
    c.reply(c.action('connection'), {
      kind: 'ready',
      target: next,
      name: 'ChatGPT',
      enabled: true,
      enabledModelCount: 0,
    });
    settle(c.action('discoverModels'), { kind: 'failed' });
    assert.equal(c.state.notice, 'connectionModelsFailed');
    c.reply(c.action('credentialStatus'), {
      kind: 'status',
      status: { locator, configured: true, credentialId: 'saved-key', revision: 1 },
    });
    c.raw('setup-models');
    c.reply(c.action('connection'), {
      kind: 'ready',
      target: next,
      name: 'ChatGPT',
      enabled: true,
      enabledModelCount: 0,
    });
    assert.equal(c.action('saveCredential'), undefined);
    settle(c.action('discoverModels'), {
      kind: 'ready',
      connection: { connectionId: target.connectionId, revision: next.revision + 1 },
    });
    c.raw('done');
    assert.ok(c.out.events.some((e) => e.name === 'completed'));
  });
  test(`${platform} models extend on scroll and keep earlier choices without paging controls`, async () => {
    const c = await client('models', platform, {
      catalogEpoch: 0,
      openRequest: null,
      selection: null,
      busy: false,
    });
    const choice = (id) => ({
      selection: { connectionId: 'a', revision: 1, modelId: id },
      connectionName: 'A',
      label: id,
      thinkingLevels: [],
      defaultThinkingLevel: null,
    });
    const cursor = { query: '', connection: 0, model: 16 };
    c.reply(c.action('models'), {
      kind: 'ready',
      revision: 3,
      choices: [choice('first')],
      defaultModel: { kind: 'none' },
      next: cursor,
    });
    c.query('model-query', 1, '');
    c.raw('toggle');
    const scroll = (revision, direction, end) =>
      c.event({
        kind: 'component',
        child: 'model-rows',
        name: 'observed',
        event: {
          readingRevision: revision,
          scroll: { readingRevision: revision, direction },
          pendingCommand: null,
          viewport: { readingRevision: revision, atStart: !end, atEnd: end, visibleChildKeys: [] },
        },
      });
    c.event({
      kind: 'component',
      child: 'model-rows',
      name: 'observed',
      event: {
        readingRevision: 1,
        scroll: { readingRevision: 1, direction: 'down' },
        pendingCommand: null,
        viewport: { readingRevision: 0, atStart: true, atEnd: false, visibleChildKeys: [] },
      },
    });
    assert.equal(c.state.scrollRevision, 0, 'wait for the viewport produced by the scroll');
    scroll(1, 'down', true);
    assert.deepEqual(c.action('models').input, { kind: 'continue', revision: 3, cursor });
    c.reply(c.action('models'), {
      kind: 'ready',
      revision: 3,
      choices: Array.from({ length: 80 }, (_, at) => choice('model-' + at)),
      defaultModel: { kind: 'none' },
      next: null,
    });
    assert.equal(c.state.choices[0].label, 'first');
    assert.equal(c.state.choices.length, 81);
    scroll(2, 'down', true);
    assert.equal(c.state.windowStart, 32);
    scroll(3, 'up', false);
    assert.equal(c.state.windowStart, 0);
    assert.equal(c.state.choices[0].label, 'first');
    assert.equal(c.out.actions.length, 0, 'reverse scrolling uses the already loaded rows');
    const firstKey = c.state.choiceKeys[JSON.stringify(['a', 'first'])];
    c.raw(firstKey, 'activate');
    assert.equal(c.state.draft.choice.label, 'first');
  });
  test(`${platform} ChatGPT login retains progress, cancellation and original receipt`, async () => {
    const c = await client('credential', platform, { target });
    const status = c.action('credentialStatus'),
      definition = c.action('provider');
    c.reply(definition, provider);
    c.reply(status, {
      kind: 'status',
      status: {
        locator: { scope: 'connection', connectionId: target.connectionId, kind: 'provider' },
        configured: false,
        credentialId: null,
        revision: null,
      },
    });
    c.raw('login-chatgpt');
    const login = c.action('loginCredential');
    assert.equal(login.binding, 'authentication');
    assert.deepEqual(login.resources, {});
    assert.deepEqual(login.input, {
      expected: target,
      expectedCredential: null,
      method: 'chatgpt',
    });
    c.event({
      kind: 'action',
      key: login.key,
      event: {
        status: 'updated',
        value: { url: 'https://auth.openai.com/codex/device', userCode: 'ABCD-EFGH' },
      },
    });
    assert.equal(c.state.loginPrompt.userCode, 'ABCD-EFGH');
    assert.ok(c.state.pending[login.key]);
    c.raw('login-cancel');
    assert.deepEqual(c.out.cancel, [login.key]);
    assert.equal(c.state.current.kind, 'login');
    c.event({
      kind: 'action',
      key: login.key,
      event: {
        status: 'operation',
        reply: {
          reference: { operation: 'original' },
          receipt: {
            kind: 'recorded',
            outcome: {
              kind: 'returned',
              value: { kind: 'failed', reason: 'cancelled', message: '' },
            },
            cleanup: { kind: 'confirmed' },
          },
        },
      },
    });
    assert.equal(c.state.current, null);
    assert.equal(c.state.loginPrompt, null);
    assert.equal(c.state.notice, 'loginCancelled');
    c.raw('login-chatgpt');
    const retry = c.action('loginCredential');
    c.event({
      kind: 'action',
      key: retry.key,
      event: {
        status: 'operation',
        reply: {
          reference: { operation: 'retry' },
          receipt: {
            kind: 'recorded',
            outcome: {
              kind: 'returned',
              value: {
                kind: 'saved',
                connection: {
                  connectionId: target.connectionId,
                  slug: target.slug,
                  provider: identity,
                },
              },
            },
            cleanup: { kind: 'confirmed' },
          },
        },
      },
    });
    assert.equal(c.state.notice, 'loadingConnectionModels');
    assert.ok(c.action('connection'));
    const afterLogin = { ...target, revision: target.revision + 1 };
    c.reply(c.action('connection'), {
      kind: 'stale',
      actual: { connectionId: target.connectionId, revision: afterLogin.revision },
    });
    c.reply(c.action('connection'), {
      kind: 'ready',
      target: afterLogin,
      name: 'ChatGPT',
      enabled: true,
      enabledModelCount: 0,
    });
    const discovery = c.action('discoverModels');
    assert.equal(discovery.binding, 'setup');
    assert.deepEqual(discovery.input, afterLogin);
    c.event({
      kind: 'action',
      key: discovery.key,
      event: {
        status: 'operation',
        reply: {
          reference: { operation: 'discover' },
          receipt: {
            kind: 'recorded',
            outcome: {
              kind: 'returned',
              value: {
                kind: 'ready',
                connection: {
                  connectionId: target.connectionId,
                  revision: afterLogin.revision + 1,
                },
              },
            },
            cleanup: { kind: 'confirmed' },
          },
        },
      },
    });
    assert.equal(c.state.notice, 'connectionReady');
    assert.equal(c.state.setup, 'ready');
    assert.equal(c.state.finished, true);
    assert.equal(c.state.command, null);
    assert.equal(
      c.out.events.some((e) => e.name === 'changed'),
      true,
    );
  });
}

for (const platform of ['tui']) {
  test(`${platform} model settings capture token overrides and retain an uncertain save`, async () => {
    const selection = { connectionId: 'connection-a', revision: 7, modelId: 'model' };
    const c = await client('model-default', platform, {
      busy: false,
      defaultModel: { kind: 'none' },
      selection: {
        selection,
        label: 'Model',
        connectionName: 'Provider',
        tools: { codeMode: null, applyPatch: null },
      },
    });
    const values = {
      contextWindow: '272000',
      inputLimit: '',
      maxOutputTokens: '4096',
      compactionThreshold: '',
    };
    for (const [name, text] of Object.entries(values)) {
      const child = 'limit-' + name;
      c.query(child, 1, '');
      const command = c.state.limits[name].command;
      c.event({
        kind: 'component',
        child,
        name: 'changed',
        event: {
          revision: 2,
          snapshot: { text: { owner: child, revision: 2, bytes: 0 }, marked: null },
          receipt: {
            owner: command.owner,
            sequence: command.sequence,
            outcome: { status: 'applied' },
          },
        },
      });
      c.query(child, 3, text);
    }
    c.raw('tools-save');
    const reads = [...c.out.actions];
    assert.equal(reads.length, 4);
    for (const action of reads) {
      const name = action.resources.text.child.slice(6);
      assert.equal(action.resources.text.input.revision, 3);
      c.reply(action, {
        metadata: c.state.limits[name].observation.snapshot.text,
        text: values[name],
        offset: 0,
        next: null,
      });
    }
    const save = c.action('updateModelSettings');
    assert.deepEqual(save.input, {
      selection,
      tools: { codeMode: null, applyPatch: null },
      limits: {
        contextWindow: 272000,
        inputLimit: null,
        maxOutputTokens: 4096,
        compactionThreshold: null,
      },
    });
    c.event({
      kind: 'action',
      key: save.key,
      event: { status: 'failed', diagnostic: 'connection lost' },
    });
    c.raw('tools-save');
    assert.equal(c.out.actions.length, 0, 'an uncertain write cannot be repeated');
    assert.ok(c.state.current);
  });
}
