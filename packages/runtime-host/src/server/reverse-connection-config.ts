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

import { chmod, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { randomUUID } from 'node:crypto';
import {
  reverseConnectionSchema,
  type ReverseConnectionConfiguration,
} from '../transport/reverse-websocket-protocol.js';

const FILE = 'reverse-connection.json';
export async function readReverseConnectionConfiguration(
  controlDirectory: string,
): Promise<ReverseConnectionConfiguration | undefined> {
  const path = join(controlDirectory, FILE);
  try {
    const metadata = await stat(path);
    if (process.platform !== 'win32' && (metadata.mode & 0o077) !== 0)
      throw new Error('reverse-connection.json contains a secret and must have mode 0600');
    return reverseConnectionSchema.parse(JSON.parse(await readFile(path, 'utf8')));
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return undefined;
    throw error;
  }
}

export async function writeReverseConnectionConfiguration(
  controlDirectory: string,
  value: unknown,
): Promise<void> {
  const configuration = reverseConnectionSchema.parse(value);
  const path = join(controlDirectory, FILE);
  const temporary = `${path}.${randomUUID()}.tmp`;
  try {
    await writeFile(temporary, JSON.stringify(configuration, null, 2) + '\n', {
      mode: 0o600,
      flag: 'wx',
    });
    await chmod(temporary, 0o600);
    await rename(temporary, path);
  } finally {
    await rm(temporary, { force: true });
  }
}
export async function removeReverseConnectionConfiguration(
  controlDirectory: string,
): Promise<void> {
  await rm(join(controlDirectory, FILE), { force: true });
}
