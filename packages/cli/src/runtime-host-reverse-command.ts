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

import { randomBytes } from 'node:crypto';
import { mkdir, readFile, stat, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import {
  resolveStorageRoot,
  prepareStorageRootControlDirectory,
  discoverMarkedStorageRoot,
  resolveExistingStorageRootControlDirectory,
} from '@maka/storage/root-authority';
import {
  startReverseRuntimeHostGateway,
  reverseGatewaySchema,
  reverseConnectionSchema,
  readReverseConnectionConfiguration,
  writeReverseConnectionConfiguration,
  removeReverseConnectionConfiguration,
} from '@maka/runtime-host/server';
import type { RuntimeHostCliCommand } from './runtime-host-cli.js';

type Command = Extract<RuntimeHostCliCommand, { kind: 'runtime-host-reverse' }>;
async function readSecretJson(path: string): Promise<unknown> {
  const metadata = await stat(path);
  if (process.platform !== 'win32' && (metadata.mode & 0o077) !== 0)
    throw new Error('Reverse configuration contains secrets; set file permissions to 0600');
  return JSON.parse(await readFile(path, 'utf8'));
}
export async function runRuntimeHostReverseCli(command: Command): Promise<number> {
  if (command.action === 'init') {
    const token = randomBytes(32).toString('base64url');
    const connector = reverseConnectionSchema.parse({
      schemaVersion: 1,
      url: command.url,
      route: command.route,
      token,
    });
    const gateway = reverseGatewaySchema.parse({
      schemaVersion: 1,
      host: '127.0.0.1',
      port: command.port ?? 7444,
      routes: [{ route: connector.route, token }],
    });
    const directory = resolve(command.directory!);
    // Refuse existing directories rather than replacing an established pairing.
    await mkdir(directory, { mode: 0o700 });
    await writeFile(join(directory, 'gateway.json'), JSON.stringify(gateway, null, 2) + '\n', {
      mode: 0o600,
      flag: 'wx',
    });
    await writeFile(join(directory, 'connector.json'), JSON.stringify(connector, null, 2) + '\n', {
      mode: 0o600,
      flag: 'wx',
    });
    process.stdout.write(
      `Created ${directory}/gateway.json and connector.json. Transfer only connector.json to the executing Host. Client URL: ${connector.url}/runtime-host/${connector.route}\n`,
    );
    return 0;
  }
  if (command.action === 'gateway') {
    const configuration = reverseGatewaySchema.parse(await readSecretJson(command.configPath!));
    const gateway = await startReverseRuntimeHostGateway({
      configuration,
      ...(command.tlsCertificatePath && command.tlsPrivateKeyPath
        ? {
            tls: {
              certificate: await readFile(command.tlsCertificatePath),
              privateKey: await readFile(command.tlsPrivateKeyPath),
            },
          }
        : {}),
    });
    process.stdout.write(
      `Reverse gateway ready at ${gateway.endpoint}. Host clients use /runtime-host/<route>.\n`,
    );
    try {
      await new Promise<void>((resolve) => {
        const stop = () => {
          process.off('SIGINT', stop);
          process.off('SIGTERM', stop);
          resolve();
        };
        process.once('SIGINT', stop);
        process.once('SIGTERM', stop);
      });
    } finally {
      await gateway.close();
    }
    return 0;
  }
  // Read configuration before creating any root, so malformed input leaves no data behind.
  const configuration =
    command.action === 'configure'
      ? reverseConnectionSchema.parse(await readSecretJson(command.configPath!))
      : undefined;
  const root = configuration
    ? await resolveStorageRoot({ path: command.rootPath!, kind: 'interactive' })
    : await discoverMarkedStorageRoot({ path: command.rootPath! });
  if (root.kind !== 'interactive')
    throw new Error('Reverse connections require an interactive Host root');
  const { controlDirectory } = configuration
    ? await prepareStorageRootControlDirectory(root)
    : await resolveExistingStorageRootControlDirectory(root);
  if (configuration) await writeReverseConnectionConfiguration(controlDirectory, configuration);
  else if (command.action === 'disable')
    await removeReverseConnectionConfiguration(controlDirectory);
  const current = await readReverseConnectionConfiguration(controlDirectory);
  process.stdout.write(
    JSON.stringify({
      rootId: root.rootId,
      configured: !!current,
      ...(current ? { url: current.url, route: current.route } : {}),
      status: 'configuration_only',
      ...(command.action !== 'status' ? { restartRequired: true } : {}),
    }) + '\n',
  );
  return 0;
}
