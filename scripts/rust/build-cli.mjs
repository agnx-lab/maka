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
import { parseArgs } from 'node:util';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import { copyFile, lstat, mkdir, readdir, rm } from 'node:fs/promises';

const root = fileURLToPath(new URL('../../', import.meta.url));

export function rustTarget(platform = process.platform, arch = process.arch) {
  const cpu = { x64: 'x86_64', arm64: 'aarch64' }[arch];
  const os = { darwin: 'apple-darwin', linux: 'unknown-linux-gnu', win32: 'pc-windows-msvc' }[
    platform
  ];
  if (!cpu || !os) throw new Error(`Unsupported Maka target: ${platform}/${arch}`);
  return `${cpu}-${os}`;
}

export async function buildCli({
  release = false,
  target,
  repositoryRoot = root,
  env = process.env,
} = {}) {
  const triple = target ?? rustTarget();
  const linuxRelease = release && triple.includes('-linux-');
  if (linuxRelease && !/^(x86_64|aarch64)-unknown-linux-gnu$/u.test(triple)) {
    throw new Error('Linux releases require an x86_64 or aarch64 GNU target');
  }
  // Zig is only used with an explicit target, including native Linux builds.
  // Cargo writes the output under the Rust triple, without the glibc suffix.
  const outputTarget = linuxRelease ? triple : target;
  // Development builds use the repository target/debug path.
  const targetDirectory = resolve(repositoryRoot, (release && env.CARGO_TARGET_DIR) || 'target');
  const args = [
    linuxRelease ? 'zigbuild' : 'build',
    '--locked',
    '-p',
    'maka-cli',
    '-p',
    'maka-desktop-shell',
    '--target-dir',
    targetDirectory,
  ];
  if (release) args.push('--release');
  if (outputTarget) args.push('--target', linuxRelease ? `${triple}.2.28` : outputTarget);
  await new Promise((resolveBuild, reject) => {
    const child = spawn('cargo', args, {
      cwd: repositoryRoot,
      env,
      stdio: 'inherit',
      windowsHide: true,
    });
    child.once('error', reject);
    child.once('exit', (code, signal) => {
      if (code === 0) resolveBuild();
      else reject(new Error(`Maka build failed: ${signal ?? code}`));
    });
  });
  const windows = triple.includes('-windows-');
  const binary = join(
    targetDirectory,
    ...(outputTarget ? [outputTarget] : []),
    release ? 'release' : 'debug',
    windows ? 'maka.exe' : 'maka',
  );
  await stageDesktopApp(repositoryRoot, join(dirname(binary), 'app'));
  return binary;
}

/** Stage only regular authored app files; executable lookup is independent of cwd. */
export async function stageDesktopApp(repositoryRoot, destination) {
  const source = join(repositoryRoot, 'products/maka/desktop-shell/app');
  // Validate before replacing a previous successful development sidecar.
  const files = [];
  async function collect(directory, relative = '') {
    const metadata = await lstat(directory);
    if (!metadata.isDirectory())
      throw new Error('Desktop app directory must not be a symlink or special file');
    for (const name of await readdir(directory)) {
      const file = join(directory, name);
      const path = join(relative, name);
      const metadata = await lstat(file);
      if (metadata.isDirectory()) await collect(file, path);
      else if (metadata.isFile()) files.push(path);
      else throw new Error('Desktop app contains a non-regular file');
    }
  }
  await collect(source);
  if (!files.includes('gpui-shell.json') || !files.includes('main.js')) {
    throw new Error('Desktop app manifest and main.js are required');
  }
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  for (const file of files) {
    await mkdir(dirname(join(destination, file)), { recursive: true });
    await copyFile(join(source, file), join(destination, file));
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { values } = parseArgs({
    options: { release: { type: 'boolean' }, target: { type: 'string' } },
  });
  console.log(await buildCli(values));
}
