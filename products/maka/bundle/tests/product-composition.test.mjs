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
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, mkdtempSync, writeFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sdk = await import(pathToFileURL(resolve(root, 'dist/authoring/sdk.mjs')));

test('the product ships checked business Host packages without a default presenter', () => {
  const inputs = JSON.parse(readFileSync(resolve(root, 'package-inputs.json'), 'utf8'));
  const names = readdirSync(resolve(root, 'dist/packages')).sort();
  assert.deepEqual(names, Object.keys(inputs).sort());
  for (const name of names) {
    const manifest = JSON.parse(
      readFileSync(resolve(root, 'dist/packages', name, 'maka.extension.json'), 'utf8'),
    );
    assert.deepEqual(Object.keys(manifest.application.entries), ['host'], name);
    assert.ok(manifest.displayName?.trim(), name);
    assert.ok(manifest.description?.trim(), name);
    assert.ok(manifest.localizations?.['zh-CN']?.description?.trim(), name);
  }
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
    {
      const configuration = {
        locale: 'en',
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
      };
      const path = resolve(directory, 'configuration.json');
      writeFileSync(path, JSON.stringify(configuration));
      const output = resolve(directory, 'host-source');
      execFileSync(
        binary,
        ['application', 'compose', '--configuration', path, '--output', output],
        { encoding: 'utf8', env: { ...process.env, PATH: directory, NODE_PATH: '' } },
      );
      const read = (name) => JSON.parse(readFileSync(resolve(output, `${name}.json`), 'utf8'));
      const { keys, hostDeployment } = read('installation');
      const host = read('host');
      assert.match(hostDeployment, /^[0-9a-f-]{36}$/);
      assert.deepEqual(host.environment, { location: 'host' });
      assert.equal(existsSync(resolve(output, 'ui.json')), false);
      assert.equal(existsSync(resolve(output, 'management-host.json')), false);
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
      const endpoints = read('endpoints');
      assert.deepEqual(endpoints.coordinator.consumer, [keys.gateway]);
      assert.equal(endpoints.coordinator.import, 'coordinator');
      assert.deepEqual(endpoints.workhub.consumer, [keys.gateway]);
      assert.equal(endpoints.workhub.import, 'workhub');
      const grants = read('native-grants');
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
      const installation = read('installation');
      writeFileSync(path, JSON.stringify(installation));
      const repeated = resolve(directory, 'same-installation');
      execFileSync(
        binary,
        ['application', 'compose', '--configuration', path, '--output', repeated],
        { encoding: 'utf8', env: { ...process.env, PATH: directory, NODE_PATH: '' } },
      );
      for (const name of ['host', 'endpoints', 'native-grants', 'installation'])
        assert.deepEqual(
          JSON.parse(readFileSync(resolve(repeated, `${name}.json`), 'utf8')),
          read(name),
          `${name} identities and authority survive reauthoring`,
        );
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('native Web tools freeze their display contract while retaining explicit custom presentations', async () => {
  const { composition, instanceNames, packageNames } = await import(
    '../dist/authoring/composition.mjs'
  );
  const { host } = composition({
    locale: 'en',
    hostDeployment: 'host',
    keys: Object.fromEntries(instanceNames.map((name, index) => [name, `key-${index}`])),
    packages: Object.fromEntries(
      packageNames.map((name) => [name, { id: name, revision: 'a'.repeat(64) }]),
    ),
    web: { network: 'denied' },
    agent: {
      workspaceRoots: [],
      tools: [
        {
          name: 'WorkspacePage',
          description: 'Fetch a page',
          import: 'web',
          method: 'fetch',
          result: 'json',
          confirmation: 'preapproved',
        },
        {
          name: 'WorkspaceQuery',
          description: 'Search the web',
          import: 'web',
          method: 'search',
          result: 'json',
          confirmation: 'preapproved',
        },
        {
          name: 'CustomPage',
          description: 'Use a custom card',
          import: 'web',
          method: 'fetch',
          result: 'json',
          confirmation: 'preapproved',
          presentation: 'product.customPage@2',
        },
      ],
    },
  });
  const tools = host.instances.agent.configuration.values.tools;
  assert.deepEqual(
    tools.map(({ name, input, presentation }) => ({ name, input, presentation })),
    [
      { name: 'WorkspacePage', input: 'web', presentation: 'maka.workspace.Web@1/fetch' },
      { name: 'WorkspaceQuery', input: 'web', presentation: 'maka.workspace.Web@1/search' },
      { name: 'CustomPage', input: 'web', presentation: 'product.customPage@2' },
    ],
  );
  assert.deepEqual(host.instances.agent.bindings.web, [
    { kind: 'instance', path: ['workspace-web'], export: 'web' },
  ]);
});
