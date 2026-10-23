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

import { spawn } from 'node:child_process';
import { release } from 'node:os';
import { resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));

/** Select development build capabilities before Cargo resolves native dependencies. */
export async function runCli(
  argv,
  {
    env = process.env,
    platform = process.platform,
    osRelease = release(),
    repositoryRoot = root,
  } = {},
) {
  const mode = env.MAKA_CUA_BUILD_MODE ?? 'auto';
  if (!['auto', 'native', 'bridge'].includes(mode)) {
    throw new Error('MAKA_CUA_BUILD_MODE must be auto, native, or bridge');
  }
  const bridge =
    mode === 'bridge' ||
    (mode === 'auto' && platform === 'linux' && osRelease.toLowerCase().includes('microsoft'));
  const args = [
    'run',
    '--locked',
    '-p',
    'maka-cli',
    ...(bridge ? ['--no-default-features'] : []),
    '--',
    ...argv,
  ];
  const build = await cargo([
    'build',
    '--locked',
    '-p',
    'maka-python-runtime',
    '--bin',
    'maka-python-worker',
  ]);
  if (build.code !== 0 || build.signal) return build;
  return await cargo(args);

  function cargo(args) {
    return new Promise((resolveRun, reject) => {
      const child = spawn('cargo', args, {
        cwd: repositoryRoot,
        env,
        stdio: 'inherit',
        windowsHide: true,
      });
      child.once('error', reject);
      child.once('exit', (code, signal) => resolveRun({ code, signal }));
    });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const { code, signal } = await runCli(process.argv.slice(2));
    if (signal) process.kill(process.pid, signal);
    else process.exitCode = code;
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
