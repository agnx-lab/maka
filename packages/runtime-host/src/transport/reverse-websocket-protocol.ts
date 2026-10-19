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

import WebSocket, { type RawData } from 'ws';
import { z } from 'zod';

export const reverseRouteSchema = z.string().regex(/^[A-Za-z0-9_-]{1,64}$/u);
export const reverseTokenSchema = z.string().regex(/^[A-Za-z0-9_-]{32,128}$/u);
// Optional credentials for a deployment's access proxy; never override transport headers.
const proxyHeadersSchema = z.record(z.string(), z.string()).refine((headers) => {
  const entries = Object.entries(headers);
  const names = entries.map(([name]) => name.toLowerCase());
  return (
    entries.length <= 16 &&
    new Set(names).size === names.length &&
    entries.reduce((size, [name, value]) => size + name.length + value.length, 0) <= 8192 &&
    entries.every(
      ([name, value]) =>
        /^[A-Za-z0-9-]+$/u.test(name) &&
        !/^(authorization|host|connection|upgrade|origin|cookie|content-length|transfer-encoding|proxy-authorization|sec-websocket-.*|x-maka-.*)$/iu.test(
          name,
        ) &&
        /^[\x20-\x7e]*$/u.test(value),
    )
  );
}, 'Invalid proxy headers');
export const reverseConnectionSchema = z
  .object({
    schemaVersion: z.literal(1),
    url: z.string().transform((value, ctx) => {
      try {
        return reverseBaseUrl(value);
      } catch {
        ctx.addIssue({
          code: 'custom',
          message: 'Use wss://, or ws:// on loopback, without credentials, path, query or fragment',
        });
        return z.NEVER;
      }
    }),
    route: reverseRouteSchema,
    token: reverseTokenSchema,
    headers: proxyHeadersSchema.optional(),
  })
  .strict();
export type ReverseConnectionConfiguration = z.infer<typeof reverseConnectionSchema>;
export const reverseControlMessage = z.discriminatedUnion('kind', [
  z
    .object({
      kind: z.literal('open'),
      ticket: reverseTokenSchema,
      credential: z.string().min(1).max(8192),
    })
    .strict(),
  z
    .object({
      kind: z.literal('reject'),
      ticket: reverseTokenSchema,
      status: z.union([z.literal(401), z.literal(503)]),
    })
    .strict(),
]);

export function reverseBaseUrl(value: string): string {
  const url = new URL(value);
  if (
    url.username ||
    url.password ||
    url.search ||
    url.hash ||
    url.pathname !== '/' ||
    (url.protocol !== 'wss:' &&
      !(url.protocol === 'ws:' && ['127.0.0.1', '[::1]', 'localhost'].includes(url.hostname)))
  ) {
    throw new Error('Invalid reverse connection URL');
  }
  return url.origin;
}

/** Both sides probe: half-open sockets must not leave a Host falsely online. */
export function monitorReverseSocket(socket: WebSocket): () => void {
  let alive = true;
  const pong = () => {
    alive = true;
  };
  socket.on('pong', pong);
  const timer = setInterval(() => {
    if (!alive) {
      socket.terminate();
      return;
    }
    alive = false;
    if (socket.readyState === WebSocket.OPEN) socket.ping();
  }, 15_000);
  timer.unref();
  const stop = () => {
    clearInterval(timer);
    socket.off('pong', pong);
  };
  socket.once('close', stop);
  return stop;
}

export function openReverseSocket(
  url: string,
  headers: Record<string, string>,
  signal: AbortSignal,
  onOpen?: (socket: WebSocket) => void,
): Promise<WebSocket> {
  signal.throwIfAborted();
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url, {
      headers,
      handshakeTimeout: 10_000,
      maxPayload: 2 * 1024 * 1024,
      perMessageDeflate: false,
      followRedirects: false,
    });
    const abort = () => socket.terminate();
    signal.addEventListener('abort', abort, { once: true });
    socket.once('close', () => signal.removeEventListener('abort', abort));
    socket.on('error', reject);
    socket.once('open', () => {
      try {
        monitorReverseSocket(socket);
        // Install protocol readers synchronously: upgrade and first frame can share a TCP packet.
        onOpen?.(socket);
        resolve(socket);
      } catch (error) {
        socket.terminate();
        reject(error);
      }
    });
  });
}

export function reverseFrameBytes(data: RawData): Buffer {
  return Array.isArray(data)
    ? Buffer.concat(data)
    : Buffer.isBuffer(data)
      ? data
      : Buffer.from(data);
}
