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
import childProcess from 'node:child_process';
import { EventEmitter } from 'node:events';
import { syncBuiltinESMExports } from 'node:module';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { runCli } from './run-cli.mjs';

const launcher = fileURLToPath(new URL('./run-cli.mjs', import.meta.url));
const root = fileURLToPath(new URL('../../', import.meta.url));

test('development builds select the native capability before forwarding CLI arguments', async (t) => {
  let invocation;
  t.mock.method(childProcess, 'spawn', (command, args, options) => {
    invocation = { command, args, options };
    const child = new EventEmitter();
    queueMicrotask(() => child.emit('exit', 0, null));
    return child;
  });
  syncBuiltinESMExports();
  t.after(() => {
    t.mock.restoreAll();
    syncBuiltinESMExports();
  });
  const native = ['run', '--locked', '-p', 'maka-cli', '--', 'code', '--log', 'a b.sqlite'];
  const bridge = [
    'run',
    '--locked',
    '-p',
    'maka-cli',
    '--no-default-features',
    '--',
    'code',
    '--log',
    'a b.sqlite',
  ];
  for (const [mode, platform, osRelease, expected] of [
    [undefined, 'linux', '6.8.0-generic', native],
    [undefined, 'linux', '6.6.87.2-microsoft-standard-WSL2', bridge],
    ['auto', 'linux', '4.4.0-Microsoft', bridge],
    ['auto', 'darwin', 'Microsoft', native],
    ['auto', 'win32', 'Microsoft', native],
    ['native', 'linux', '6.6.87.2-microsoft-standard-WSL2', native],
    ['bridge', 'linux', '6.8.0-generic', bridge],
  ]) {
    const env = mode === undefined ? {} : { MAKA_CUA_BUILD_MODE: mode };
    assert.deepEqual(
      await runCli(['code', '--log', 'a b.sqlite'], {
        env,
        platform,
        osRelease,
        repositoryRoot: '/source checkout',
      }),
      { code: 0, signal: null },
    );
    assert.deepEqual(invocation, {
      command: 'cargo',
      args: expected,
      options: {
        cwd: '/source checkout',
        env,
        stdio: 'inherit',
        windowsHide: true,
      },
    });
  }
});

// Run the public Node entrypoint while replacing only its external Cargo process.
function invokeLauncher(event, env = {}) {
  const preload = `
    import childProcess from 'node:child_process';
    import { EventEmitter } from 'node:events';
    import { syncBuiltinESMExports } from 'node:module';
    childProcess.spawn = (command, args, options) => {
      process.stdout.write(JSON.stringify({ command, args, cwd: options.cwd }) + '\\n');
      const child = new EventEmitter();
      queueMicrotask(() => ${event});
      return child;
    };
    syncBuiltinESMExports();
  `;
  return childProcess.spawnSync(
    process.execPath,
    ['--import', 'data:text/javascript,' + encodeURIComponent(preload), launcher, '--help'],
    {
      env: { ...process.env, MAKA_CUA_BUILD_MODE: 'bridge', ...env },
      encoding: 'utf8',
      timeout: 10_000,
      windowsHide: true,
    },
  );
}

test('the launcher preserves Cargo failures and invokes Cargo from the repository', () => {
  const result = invokeLauncher("child.emit('exit', 29, null)");
  assert.equal(result.status, 29);
  assert.equal(result.stderr, '');
  assert.deepEqual(JSON.parse(result.stdout), {
    command: 'cargo',
    args: ['run', '--locked', '-p', 'maka-cli', '--no-default-features', '--', '--help'],
    cwd: root,
  });
});

test('the launcher reports a Cargo spawn error', () => {
  const result = invokeLauncher("child.emit('error', new Error('Cargo could not start'))");
  assert.equal(result.status, 1);
  assert.equal(result.stderr, 'Cargo could not start\n');
});

test('the launcher rejects unknown build modes before starting Cargo', () => {
  const result = invokeLauncher("child.emit('exit', 0, null)", { MAKA_CUA_BUILD_MODE: 'typo' });
  assert.equal(result.status, 1);
  assert.equal(result.stdout, '');
  assert.equal(result.stderr, 'MAKA_CUA_BUILD_MODE must be auto, native, or bridge\n');
});

test('the launcher preserves Cargo termination signals', {
  skip: process.platform === 'win32',
}, () => {
  const result = invokeLauncher("child.emit('exit', null, 'SIGTERM')");
  assert.equal(result.status, null);
  assert.equal(result.signal, 'SIGTERM');
});
