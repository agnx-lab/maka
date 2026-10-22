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

import { mkdir, writeFile, readFile, rm, access } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, resolve, relative } from 'node:path';
import { createRequire } from 'node:module';
import { parseArgs } from 'node:util';
import { writeProductVersion } from './product-version.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const dependencyRoot = process.env.MAKA_JS_DEPS || repo;
const require = createRequire(resolve(dependencyRoot, 'package.json'));
const { build } = require('esbuild');
const lock = JSON.parse(await readFile(resolve(repo, 'package-lock.json'), 'utf8'));
for (const name of ['esbuild', 'ajv', 'ajv-formats'])
  if (require(`${name}/package.json`).version !== lock.packages[`node_modules/${name}`]?.version)
    throw new Error(`Build dependency does not match repository lockfile: ${name}`);
const exists = (path) =>
  access(path).then(
    () => true,
    () => false,
  );
const header = (await readFile(resolve(repo, 'packages/plugin-sdk/src/index.ts'), 'utf8'))
  .split('\n\n', 1)[0]
  .trim();

// Product selections are paths to authored packages, not a second declaration catalog.
export async function bundle({ packages, output, composition }) {
  const destination = resolve(output);
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  const entries = Object.fromEntries(
    Object.entries(packages).map(([name, path]) => [
      `packages/${name}`,
      resolve(repo, path, 'package.ts'),
    ]),
  );
  entries.sdk = resolve(repo, 'packages/plugin-sdk/src/index.ts');
  if (composition) entries.composition = resolve(repo, composition);
  const authoring = await build({
    absWorkingDir: repo,
    entryPoints: entries,
    outdir: resolve(destination, 'authoring'),
    bundle: true,
    nodePaths: [resolve(dependencyRoot, 'node_modules')],
    platform: 'node',
    format: 'esm',
    outExtension: { '.js': '.mjs' },
    logLevel: 'silent',
    metafile: true,
  });
  const inputs = new Set(Object.keys(authoring.metafile.inputs).map((path) => resolve(repo, path)));
  const names = [];
  for (const [name, path] of Object.entries(packages)) {
    const source = resolve(repo, path);
    const { default: manifest } = await import(
      pathToFileURL(resolve(destination, `authoring/packages/${name}.mjs`))
    );
    if (manifest.schemaVersion !== 1 || typeof manifest.id !== 'string' || !manifest.application)
      throw new Error(`Invalid package declaration: ${path}`);
    const directory = resolve(destination, 'packages', name);
    await mkdir(directory, { recursive: true });
    const hook = resolve(source, 'build.mjs');
    const virtual = (await exists(hook))
      ? await (await import(pathToFileURL(hook))).default({ require })
      : {};
    if (await exists(hook)) inputs.add(hook);
    for (const entry of Object.values(manifest.application.entries ?? {})) {
      if (entry.implementation.kind !== 'javascript') continue;
      const module = entry.implementation.module;
      if (!/^[a-z][a-z0-9-]*\.js$/.test(module)) throw new Error(`Invalid local module: ${module}`);
      const bundled = await build({
        absWorkingDir: repo,
        entryPoints: [resolve(source, module.replace(/\.js$/, '.ts'))],
        outfile: resolve(directory, module),
        bundle: true,
        nodePaths: [resolve(dependencyRoot, 'node_modules')],
        format: 'esm',
        platform: 'neutral',
        target: 'es2022',
        logLevel: 'silent',
        metafile: true,
        plugins: [
          {
            name: 'authored-inputs',
            setup(builder) {
              builder.onResolve({ filter: /^maka:/ }, ({ path }) => {
                if (!(path in virtual)) return undefined;
                return { path, namespace: 'generated' };
              });
              builder.onLoad({ filter: /.*/, namespace: 'generated' }, ({ path }) => ({
                contents: virtual[path],
                loader: 'js',
                resolveDir: source,
              }));
            },
          },
        ],
        banner: { js: header },
      });
      for (const input of Object.keys(bundled.metafile.inputs))
        if (!input.startsWith('generated:')) inputs.add(resolve(repo, input));
    }
    await writeFile(resolve(directory, 'maka.extension.json'), JSON.stringify(manifest) + '\n');
    names.push(name);
  }
  await writeFile(resolve(destination, 'packages.json'), JSON.stringify(names, null, 2) + '\n');
  await writeFile(
    resolve(destination, 'bundle-inputs.json'),
    JSON.stringify([...inputs].sort()) + '\n',
  );
  // Publish the version last: incomplete builds must not advertise new bytes.
  if (composition) await writeProductVersion(destination, names);
  console.log(`${names.length} ordinary plugin packages built`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { values } = parseArgs({
    options: { plugin: { type: 'string' }, output: { type: 'string' } },
  });
  if (!values.plugin || !values.output)
    throw new Error('Usage: build.mjs --plugin plugins/<family>/<package> --output <directory>');
  const plugin = resolve(repo, values.plugin);
  const name = relative(repo, plugin).split('/').at(-1);
  await bundle({ packages: { [name]: plugin }, output: values.output });
}
