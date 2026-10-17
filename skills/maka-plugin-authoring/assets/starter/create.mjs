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

// Authoring utility: writes a new example directory and asks the real CLI to check it.
import { execFileSync } from 'node:child_process';
import { mkdirSync, copyFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { randomUUID } from 'node:crypto';
import { activate as host } from './host.mjs';
import { activate as ui } from './tui.mjs';
const { values } = parseArgs({
  options: {
    maka: { type: 'string', default: 'maka' },
    output: { type: 'string' },
    target: { type: 'string', default: 'tui' },
  },
});
if (!values.output || !['host', 'tui'].includes(values.target))
  throw Error(
    'Usage: node create.mjs --output NEW_DIRECTORY [--target host|tui] [--maka EXECUTABLE]',
  );
const target = values.target;
const run = (args) =>
  JSON.parse(
    execFileSync(values.maka, ['application', ...args], {
      encoding: 'utf8',
      maxBuffer: 8 * 1024 * 1024,
    }),
  );
const schema = run(['schema']);
const output = resolve(values.output);
mkdirSync(output); // Never overwrite an existing project.
const packageDirectory = join(output, 'plugin');
mkdirSync(packageDirectory);
const source = dirname(fileURLToPath(import.meta.url));
const write = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n');
const required = (contract) => ({ contract, availability: 'required' });
const configuration = { type: 'object', additionalProperties: false };
const entry = (module, exports, extra = {}) => ({
  implementation: { kind: 'javascript', module, realm: 'dedicated' },
  configuration,
  imports: {},
  exports,
  ...extra,
});
const surface = schema.contracts.terminalSurface;
const entries = {
  host: entry('host.mjs', { Echo: required(host().Echo.contract) }),
  tui: entry(
    'tui.mjs',
    { Counter: required(ui().Counter.contract) },
    {
      scene: { name: 'maka.tui.scene', version: 1 },
      imports: { surface: { contract: surface, cardinality: { kind: 'one' } } },
    },
  ),
};
for (const file of ['host.mjs', 'tui.mjs'])
  copyFileSync(join(source, file), join(packageDirectory, file));
write(join(packageDirectory, 'maka.extension.json'), {
  schemaVersion: 1,
  id: 'example.plugin',
  application: { entries },
});
const node = (path) => ({
  key: randomUUID(),
  package: { source: 'pinned', package: run(['inspect-package', '--package', path]) },
  usePackage: 'entry',
});
const input = {
  environment: {
    location: target === 'host' ? 'host' : 'ui',
    ...(target === 'host' ? {} : { presentation: target }),
  },
  instances: { example: node(packageDirectory) },
};
const packages = [packageDirectory];
if (target !== 'host') {
  const path = join(output, 'surface');
  mkdirSync(path);
  write(join(path, 'maka.extension.json'), {
    schemaVersion: 1,
    id: 'example.surface',
    application: {
      entries: {
        [target]: {
          implementation: { kind: 'native', implementation: `maka.ui.surface.${target}` },
          configuration,
          imports: {},
          exports: { Surface: required(surface) },
        },
      },
    },
  });
  packages.push(path);
  input.instances.surface = node(path);
  input.instances.example.bindings = {
    surface: [{ kind: 'instance', path: ['surface'], export: 'Surface' }],
  };
  input.launch = { kind: 'instance', path: ['example'], export: 'Counter' };
}
const composition = join(output, 'composition.json');
write(composition, input);
run(['check', '--input', composition, ...packages.flatMap((path) => ['--package', path])]);
console.log(JSON.stringify({ input: composition, packages }));
