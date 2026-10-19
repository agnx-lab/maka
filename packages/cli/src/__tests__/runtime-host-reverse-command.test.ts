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
import { mkdtemp, readFile, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { parseRuntimeHostCommand } from '../runtime-host-cli.js';
import { runRuntimeHostReverseCli } from '../runtime-host-reverse-command.js';

test('reverse command validates options and exposes vendor-neutral setup help', () => {
  const parsed = parseRuntimeHostCommand([
    'reverse',
    'init',
    '--directory',
    '/tmp/new',
    '--url',
    'wss://example.com',
    '--route',
    'laptop',
  ]);
  assert.equal(parsed.kind, 'runtime-host-reverse');
  for (const argv of [
    ['reverse', 'configure', '--config', 'connector.json'],
    ['reverse', 'gateway', '--config', 'a', '--config', 'b'],
    ['reverse', 'gateway', '--config', 'a', '--tls-certificate', 'cert'],
    [
      'reverse',
      'init',
      '--directory',
      '/tmp/new',
      '--url',
      'wss://example.com',
      '--route',
      'laptop',
      '--port',
      '99999',
    ],
    ['reverse', 'status', '--root', '/tmp/root', '--url', 'wss://wrong.com'],
  ])
    assert.equal(parseRuntimeHostCommand(argv).kind, 'error');
  const help = parseRuntimeHostCommand(['reverse', '--help']);
  assert.equal(help.kind, 'help');
  if (help.kind === 'help') assert.match(help.text, /No tunnel vendor is required/);
});

test('reverse CLI creates private configuration, applies it to a root, redacts status, and disables without restarting', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'maka-reverse-cli-'));
  const setup = join(dir, 'pairing');
  const root = join(dir, 'root');
  let output = '';
  const originalWrite = process.stdout.write;
  process.stdout.write = ((chunk: string | Uint8Array) => {
    output += chunk.toString();
    return true;
  }) as typeof process.stdout.write;
  const run = async (args: string[]) => {
    const command = parseRuntimeHostCommand(['reverse', ...args]);
    if (command.kind !== 'runtime-host-reverse') throw new Error(JSON.stringify(command));
    return runRuntimeHostReverseCli(command);
  };
  try {
    await run(['init', '--directory', setup, '--url', 'wss://example.com', '--route', 'laptop']);
    const connector = JSON.parse(await readFile(join(setup, 'connector.json'), 'utf8'));
    const gateway = JSON.parse(await readFile(join(setup, 'gateway.json'), 'utf8'));
    assert.equal(connector.token, gateway.routes[0].token);
    if (process.platform !== 'win32')
      assert.equal((await stat(join(setup, 'connector.json'))).mode & 0o777, 0o600);
    await assert.rejects(
      run(['init', '--directory', setup, '--url', 'wss://example.com', '--route', 'laptop']),
    );
    assert.equal(
      JSON.parse(await readFile(join(setup, 'connector.json'), 'utf8')).token,
      connector.token,
    );
    await run(['configure', '--root', root, '--config', join(setup, 'connector.json')]);
    assert.match(output, /"restartRequired":true/);
    output = '';
    await run(['status', '--root', root]);
    assert.match(output, /"configured":true/);
    assert.match(output, /"status":"configuration_only"/);
    assert.ok(!output.includes(connector.token));
    output = '';
    await run(['disable', '--root', root]);
    assert.match(output, /"configured":false/);
    assert.match(output, /"restartRequired":true/);
  } finally {
    process.stdout.write = originalWrite;
    await rm(dir, { recursive: true, force: true });
  }
});
