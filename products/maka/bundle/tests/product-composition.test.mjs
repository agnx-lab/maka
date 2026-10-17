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
import Ajv from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, mkdtempSync, writeFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sdk = await import(pathToFileURL(resolve(root, 'dist/authoring/sdk.mjs')));

test('default Agent and WorkHub installations allow requests for additional process access', async () => {
  const { installation } = await import('../dist/authoring/composition.mjs');
  const config = installation({
    workspace: '/project',
    hostRoot: '/state',
    helper: '/maka',
    themeFile: '/theme.json',
  });
  for (const [agent, processes] of [
    [config.agent, config.processes],
    [config.coordinator, config.coordinatorProcesses],
  ]) {
    assert.equal(agent.permissions.access, 'workspace_write');
    assert.equal(agent.permissions.requestPermissions, true);
    assert.equal(processes.network, 'allowed');
    assert.deepEqual(processes.filesystem, { default: 'write', rules: [], denyGlobs: [] });
  }
});

test('each bundled UI exports the exact contracts admitted by its original manifest', async () => {
  const checked = new Set();
  for (const name of readdirSync(resolve(root, 'dist/packages'))) {
    const directory = resolve(root, 'dist/packages', name);
    const manifest = JSON.parse(readFileSync(resolve(directory, 'maka.extension.json'), 'utf8'));
    for (const target of ['tui']) {
      const entry = manifest.application.entries[target];
      if (entry?.implementation.kind !== 'javascript') continue;
      const guest = await import(pathToFileURL(resolve(directory, entry.implementation.module)));
      const declarations = guest.activate({ locale: 'en', scheme: 'light' });
      assert.deepEqual(Object.keys(declarations).sort(), Object.keys(entry.exports).sort());
      for (const [key, declaration] of Object.entries(declarations)) {
        assert.deepEqual(
          declaration.contract,
          entry.exports[key].contract,
          `${name}/${target}/${key}`,
        );
        checked.add(`${name}/${key}`);
      }
    }
  }
  for (const name of [
    'agent-screen/AgentScreen',
    'workhub-root/WorkHub',
    'connections/Connections',
  ])
    assert.equal(checked.has(name), true, `missing actual TUI declaration: ${name}`);
});

test('typed scene constructors serialize directly into the actual Rust scene schemas', async () => {
  const terminalText = sdk.ui.terminal.text({
    key: 'terminal-text',
    text: '界面',
    color: 0xffffff,
  });
  const terminal = sdk.ui.terminal.stack({
    key: 'terminal',
    direction: 'column',
    gap: 1,
    padding: 0,
    background: 0,
    children: [terminalText],
    scroll: 'anchor',
  });
  const terminalScenes = [
    terminal,
    terminalText,
    sdk.ui.terminal.button({ key: 'button', label: 'Save' }),
    sdk.ui.terminal.dialog({
      key: 'dialog',
      label: 'Dialog',
      background: terminal,
      content: terminalText,
      open: true,
      width: 40,
      height: 10,
      background_color: 0,
      border_color: 0xffffff,
    }),
    sdk.ui.terminal.reading({
      key: 'reading',
      binding: 'reading',
      props: { content: terminal },
      on: { observed: 'viewport' },
    }),
  ];
  const encoded = JSON.stringify(terminal);
  assert.deepEqual(JSON.parse(encoded), {
    key: 'terminal',
    direction: 'column',
    gap: 1,
    padding: 0,
    background: 0,
    children: [{ key: 'terminal-text', text: '界面', color: 0xffffff, kind: 'text' }],
    scroll: 'anchor',
    kind: 'stack',
  });
  assert.equal(Object.getOwnPropertySymbols(terminal).length, 1);
  assert.equal(encoded.includes('maka.scene.target'), false);
  assert.equal(terminalScenes.at(-1).member, 0);
  if (process.env.MAKA_PRODUCT_CLI_CHECK) {
    const binary = resolve(
      root,
      '../../..',
      process.env.CARGO_TARGET_DIR ?? 'target',
      'debug/maka',
    );
    const catalog = JSON.parse(
      execFileSync(binary, ['application', 'schema'], {
        encoding: 'utf8',
        maxBuffer: 4 * 1024 * 1024,
      }),
    );
    const { default: Ajv } = await import('ajv/dist/2020.js');
    for (const [name, scenes] of [['TerminalScene', terminalScenes]]) {
      const validate = new Ajv({ strict: false }).compile({
        $ref: '#/$defs/' + name,
        $defs: catalog.schema.$defs,
      });
      for (const scene of scenes)
        assert.equal(
          validate(JSON.parse(JSON.stringify(scene))),
          true,
          JSON.stringify(validate.errors),
        );
    }
  }
});

test('WorkHub product authors two independent Agent graphs with explicit native grants in actual CLI', {
  skip: !process.env.MAKA_PRODUCT_CLI_CHECK,
}, async () => {
  const binary = resolve(root, '../../..', process.env.CARGO_TARGET_DIR ?? 'target', 'debug/maka');
  const directory = mkdtempSync(resolve(tmpdir(), 'maka-workhub-composition-'));
  try {
    const agent = {
      workspaceRoots: [{ label: 'Project', path: '/private/product-workspace' }],
      permissions: { access: 'workspace_write', requestPermissions: true },
      tools: [],
    };
    for (const presentation of ['tui']) {
      const configuration = {
        presentation,
        locale: 'en',
        scheme: 'light',
        agent,
        coordinator: {
          ...agent,
          tools: [
            {
              name: 'Execute',
              description: 'Run a command',
              import: 'processes',
              method: 'execute',
              result: 'json',
              confirmation: 'preapproved',
            },
          ],
        },
        coordinatorProcesses: {
          filesystem: { default: 'read', rules: [], denyGlobs: [] },
          network: 'denied',
          helper: null,
        },
        hostRoot: resolve(directory, 'unopened-host'),
      };
      const path = resolve(directory, 'configuration.json');
      writeFileSync(path, JSON.stringify(configuration));
      const output = resolve(directory, presentation);
      execFileSync(
        binary,
        [
          'application',
          'compose',
          '--configuration',
          path,
          '--output',
          output,
          '--product',
          'workhub',
        ],
        { encoding: 'utf8', env: { ...process.env, PATH: directory, NODE_PATH: '' } },
      );
      const read = (name) => JSON.parse(readFileSync(resolve(output, `${name}.json`), 'utf8'));
      const { keys } = read('installation'),
        host = read('host'),
        ui = read('ui');
      assert.equal(existsSync(configuration.hostRoot), false);
      assert.deepEqual(host.instances.workhub.bindings.worker, [
        { kind: 'instance', path: ['agent'], export: 'agent' },
      ]);
      assert.deepEqual(
        host.instances['coordinator-workspace-processes'].configuration.values.sessionOwner,
        [keys.workhub],
      );
      assert.equal(
        host.instances['coordinator-workspace-processes'].configuration.values.sessionKind,
        'workhub',
      );
      assert.equal(host.instances['workspace-processes'], undefined);
      assert.deepEqual(ui.launch, { kind: 'instance', path: ['workhub-root'], export: 'WorkHub' });
      assert.equal(ui.instances['agent-screen'].bindings.agent[0].instance[0], keys.agent);
      assert.equal(
        ui.instances['coordinator-agent-screen'].bindings.agent[0].instance[0],
        keys.workhub,
      );
      assert.equal(
        ui.instances['coordinator-directory-picker'].bindings.agent[0].instance[0],
        keys.workhub,
      );
      const grants = read('connections').hosts.find((host) => host.name === 'local').native;
      assert.ok(
        grants.some(
          (grant) =>
            grant.origin[0] === keys.workhub &&
            grant.provider[0] === keys.agent &&
            grant.methods.includes('send'),
        ),
      );
      assert.ok(
        grants.some(
          (grant) =>
            grant.origin[0] === keys.workhub &&
            grant.provider[0] === keys['coordinator-workspace-processes'] &&
            grant.methods.includes('execute'),
        ),
      );
      assert.equal(
        grants.some((grant) => grant.origin[0] === keys.agent),
        false,
      );
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('workspace and navigation mount only declared child capabilities with valid props', async () => {
  const appearance = await import('../dist/packages/appearance/tui.js');
  const theme = appearance.activate({ scheme: 'dark' }).Appearance.value;
  const ajv = new Ajv({
    strict: false,
    formats: { uint64: true, uint32: true, uint16: true, int16: true, uint: true },
  });
  addFormats(ajv);
  function check(node, imports) {
    if (!node || typeof node !== 'object') return;
    if (node.kind === 'component') {
      assert.ok(imports[node.binding], `undeclared child: ${node.binding}`);
      const schema = imports[node.binding].contract.interface.props;
      if (schema) {
        const valid = ajv.compile(schema);
        assert.ok(valid(node.props), `${node.key}: ${JSON.stringify(valid.errors)}`);
      }
    }
    for (const value of Object.values(node))
      if (Array.isArray(value)) value.forEach((child) => check(child, imports));
      else check(value, imports);
  }
  for (const target of ['tui']) {
    for (const name of ['agent-screen', 'navigation']) {
      const module = await import(`../dist/packages/${name}/${target}.js`);
      module.activate({ locale: 'en' });
      const manifest = JSON.parse(
        readFileSync(resolve(root, 'dist/packages', name, 'maka.extension.json'), 'utf8'),
      );
      const props =
        name === 'agent-screen'
          ? {}
          : {
              theme,
              locale: 'en',
              sessions: [],
              selected: null,
              busy: false,
              loading: false,
              older: false,
              newer: false,
            };
      const out = module.component.mount(props, { appearance: [theme] });
      check(out.frame.view, manifest.application.entries[target].imports);
    }
  }
});

test('inline approval Escape returns to the retained task and composer', async () => {
  const appearance = await import('../dist/packages/appearance/tui.js');
  const theme = appearance.activate({ scheme: 'dark' }).Appearance.value;
  const module = await import('../dist/packages/agent-screen/tui.js');
  module.activate({ locale: 'en' });
  const values = { appearance: [theme] };
  let out = module.component.mount({}, values);
  out.frame.state.selected = {
    id: 'task-a',
    title: 'Task A',
    labels: [],
    revision: 1,
    updatedAt: 1,
    archived: false,
    activity: 'idle',
    boundaryRevision: 0,
    hasUnread: false,
    dataSource: {
      stateDomain: 'a'.repeat(64),
      deployment: '00000000-0000-0000-0000-000000000001',
      namespace: ['00000000-0000-0000-0000-000000000002'],
    },
    permissions: { access: 'workspace_write', requestPermissions: true },
    workspace: { path: '/project', directoryIdentity: 'a'.repeat(64), identity: 'workspace-a' },
  };
  out.frame.state.interactions = {
    revision: 1,
    entries: [{ requestId: 'request-a', kind: 'permissions' }],
  };
  out.frame.state.dialog = 'approval';
  out = module.component.update(out.frame.state, {}, values);
  const nodes = (node) =>
    node && typeof node === 'object'
      ? [
          node,
          ...Object.values(node).flatMap((value) =>
            Array.isArray(value) ? value.flatMap(nodes) : nodes(value),
          ),
        ]
      : [];
  const before = nodes(out.frame.view);
  const composer = before.find((n) => n.kind === 'component' && n.key === 'composer');
  const shortcut = before
    .flatMap((n) => n.shortcuts ?? [])
    .find((s) => s.key === 'escape' && s.input === 'question-close');
  assert.ok(shortcut?.enabled, 'inline interaction keeps the dialog Escape affordance');
  out = module.component.event(
    out.frame.state,
    { kind: 'raw', key: shortcut.input, event: { kind: 'activate' } },
    {},
    values,
  );
  assert.equal(out.frame.state.dialog, null);
  assert.equal(out.frame.state.selected.id, 'task-a');
  const after = nodes(out.frame.view).find((n) => n.kind === 'component' && n.key === 'composer');
  assert.equal(
    after.props.context,
    composer.props.context,
    'closing approval preserves the native draft owner',
  );
  assert.equal(after.props.command, composer.props.command);
  assert.equal(after.props.approvalPending, true, 'closing a review does not settle its approval');
  for (const kind of ['service_approval', 'permissions', 'client_capability', 'question', 'form']) {
    out.frame.state.interactions.entries[0].kind = kind;
    out = module.component.update(out.frame.state, {}, values);
    assert.equal(
      nodes(out.frame.view).find((n) => n.key === 'composer').props.approvalPending,
      !['question', 'form'].includes(kind),
    );
  }
  out.frame.state.interactions.entries = [];
  out = module.component.update(out.frame.state, {}, values);
  assert.equal(
    nodes(out.frame.view).find((n) => n.key === 'composer').props.approvalPending,
    false,
  );
});
