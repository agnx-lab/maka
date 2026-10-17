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
import { dirname, join } from 'node:path';
import { lstat, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import test from 'node:test';
import { buildCli, rustTarget, stageDesktopApp } from './build-cli.mjs';

test('CLI builds use the requested target and artifact path', async (t) => {
  const root = await mkdtemp(join(tmpdir(), 'maka-build-cli-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const sourceRoot = join(root, 'extracted-source');
  for (const repository of [root, sourceRoot]) {
    const app = join(repository, 'products/maka/desktop-shell/app');
    await mkdir(join(app, 'icons'), { recursive: true });
    await writeFile(join(app, 'gpui-shell.json'), '{"entry":"main.js"}');
    await writeFile(join(app, 'main.js'), 'authored application');
    await writeFile(join(app, 'icons/logo.svg'), '<svg/>');
  }
  const previous = process.env.CARGO_TARGET_DIR;
  process.env.CARGO_TARGET_DIR = 'target/isolated release';
  let invocation;
  let exitCode = 0;
  t.mock.method(childProcess, 'spawn', (command, args, options) => {
    invocation = { command, args, options };
    const child = new EventEmitter();
    queueMicrotask(() => child.emit('exit', exitCode, null));
    return child;
  });
  syncBuiltinESMExports();
  t.after(() => {
    t.mock.restoreAll();
    syncBuiltinESMExports();
    if (previous === undefined) delete process.env.CARGO_TARGET_DIR;
    else process.env.CARGO_TARGET_DIR = previous;
  });
  for (const target of [
    undefined,
    'x86_64-unknown-linux-gnu',
    'aarch64-unknown-linux-gnu',
    'aarch64-apple-darwin',
    'x86_64-pc-windows-msvc',
  ]) {
    const triple = target ?? rustTarget();
    const linux = triple.includes('-linux-');
    const directory = join(root, process.env.CARGO_TARGET_DIR);
    const binaryDirectory = join(directory, ...(linux || target ? [triple] : []), 'release');
    await mkdir(join(binaryDirectory, 'app'), { recursive: true });
    await writeFile(join(binaryDirectory, 'app/obsolete.js'), 'removed module');
    const output = await buildCli({ release: true, target, repositoryRoot: root });
    await assert.rejects(lstat(join(dirname(output), 'app/obsolete.js')), { code: 'ENOENT' });
    assert.equal(
      await readFile(join(dirname(output), 'app/main.js'), 'utf8'),
      'authored application',
    );
    assert.equal(await readFile(join(dirname(output), 'app/icons/logo.svg'), 'utf8'), '<svg/>');
    assert.equal(invocation.command, 'cargo');
    assert.equal(invocation.options.cwd, root);
    assert.deepEqual(invocation.args, [
      linux ? 'zigbuild' : 'build',
      '--locked',
      '-p',
      'maka-cli',
      '-p',
      'maka-desktop-shell',
      '--target-dir',
      directory,
      '--release',
      ...(linux ? ['--target', triple + '.2.28'] : target ? ['--target', target] : []),
    ]);
    assert.equal(
      output,
      join(
        directory,
        ...(linux || target ? [triple] : []),
        'release',
        triple.includes('-windows-') ? 'maka.exe' : 'maka',
      ),
    );
  }
  assert.equal(
    await buildCli({ repositoryRoot: root }),
    join(root, 'target/debug', process.platform === 'win32' ? 'maka.exe' : 'maka'),
  );
  assert.deepEqual(invocation.args, [
    'build',
    '--locked',
    '-p',
    'maka-cli',
    '-p',
    'maka-desktop-shell',
    '--target-dir',
    join(root, 'target'),
  ]);
  const sourceEnv = { MAKA_JS_DEPS: sourceRoot };
  assert.equal(
    await buildCli({
      release: true,
      target: 'aarch64-apple-darwin',
      repositoryRoot: sourceRoot,
      env: sourceEnv,
    }),
    join(sourceRoot, 'target/aarch64-apple-darwin/release/maka'),
  );
  assert.equal(invocation.options.cwd, sourceRoot);
  assert.equal(invocation.options.env, sourceEnv);
  exitCode = 1;
  await assert.rejects(buildCli({ release: true, repositoryRoot: root }), /Maka build failed: 1/u);
});

test('desktop app staging rejects symlinks without replacing the accepted sidecar', async () => {
  const root = await mkdtemp(join(tmpdir(), 'maka-app-staging-'));
  try {
    const source = join(root, 'products/maka/desktop-shell/app');
    const destination = join(root, 'bin/app');
    await mkdir(source, { recursive: true });
    await writeFile(join(source, 'gpui-shell.json'), '{"entry":"main.js"}');
    await writeFile(join(source, 'main.js'), 'original app');
    await stageDesktopApp(root, destination);
    await writeFile(join(root, 'outside.js'), 'not authored app content');
    await symlink(root, join(source, 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
    await assert.rejects(stageDesktopApp(root, destination), /non-regular file/);
    assert.equal(await readFile(join(destination, 'main.js'), 'utf8'), 'original app');
    await assert.rejects(lstat(join(destination, 'linked')), { code: 'ENOENT' });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
