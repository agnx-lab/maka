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

import { createServer } from 'node:http';
import { createServer as createTlsServer } from 'node:https';
import type { IncomingMessage } from 'node:http';
import type { Duplex } from 'node:stream';
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';
import WebSocket, { WebSocketServer } from 'ws';
import { z } from 'zod';
import { RUNTIME_HOST_MAX_MESSAGE_BYTES } from '../protocol/index.js';
import {
  monitorReverseSocket,
  reverseControlMessage,
  reverseFrameBytes,
  reverseRouteSchema,
  reverseTokenSchema,
} from '../transport/reverse-websocket-protocol.js';

export const reverseGatewaySchema = z
  .object({
    schemaVersion: z.literal(1),
    host: z.string().default('127.0.0.1'),
    port: z.number().int().min(0).max(65535),
    behindProxy: z.boolean().optional(),
    routes: z
      .array(z.object({ route: reverseRouteSchema, token: reverseTokenSchema }).strict())
      .min(1)
      .max(64),
  })
  .strict()
  .superRefine((value, ctx) => {
    if (new Set(value.routes.map((r) => r.route)).size !== value.routes.length)
      ctx.addIssue({ code: 'custom', message: 'Duplicate reverse route' });
  });
export type ReverseGatewayConfiguration = z.infer<typeof reverseGatewaySchema>;

interface Pending {
  request: IncomingMessage;
  socket: Duplex;
  head: Buffer;
  controller: WebSocket;
  timer: NodeJS.Timeout;
  detach: () => void;
}
interface Route {
  tokenHash: Buffer;
  controller?: WebSocket;
  pending: Map<string, Pending>;
  active: Set<WebSocket>;
}

/** Trusted gateway, deliberately independent of DNS/proxy/tunnel vendors. */
export async function startReverseRuntimeHostGateway(input: {
  configuration: ReverseGatewayConfiguration;
  tls?: { certificate: string | Buffer; privateKey: string | Buffer };
}): Promise<{ endpoint: string; close(): Promise<void> }> {
  const config = reverseGatewaySchema.parse(input.configuration);
  if (!input.tls && !config.behindProxy && !['127.0.0.1', '::1', 'localhost'].includes(config.host))
    throw new Error('Public reverse gateway requires TLS or explicit behindProxy configuration');
  const routes = new Map<string, Route>(
    config.routes.map((r) => [
      r.route,
      {
        tokenHash: hash(r.token),
        pending: new Map(),
        active: new Set(),
      },
    ]),
  );
  const respond = (_request: IncomingMessage, response: import('node:http').ServerResponse) => {
    response.writeHead(404);
    response.end();
  };
  const server = input.tls
    ? createTlsServer({ cert: input.tls.certificate, key: input.tls.privateKey }, respond)
    : createServer(respond);
  const webSocketOptions = {
    noServer: true,
    perMessageDeflate: false,
    maxPayload: RUNTIME_HOST_MAX_MESSAGE_BYTES,
    maxFragments: 256,
    maxBufferedChunks: 256,
  };
  const wss = new WebSocketServer(webSocketOptions);
  let closing = false;
  const consume = (route: Route, ticket: string) => {
    const pending = route.pending.get(ticket);
    if (pending) {
      route.pending.delete(ticket);
      clearTimeout(pending.timer);
      pending.detach();
    }
    return pending;
  };
  const offline = (route: Route, controller: WebSocket) => {
    if (route.controller !== controller) return;
    route.controller = undefined;
    for (const ticket of route.pending.keys()) {
      const pending = consume(route, ticket)!;
      reject(pending.socket, 503);
    }
    for (const socket of route.active) socket.terminate();
    route.active.clear();
  };
  server.on('upgrade', (request, socket, head) => {
    socket.on('error', () => {});
    const path = request.url ?? '';
    const match = /^\/(reverse|runtime-host)\/([A-Za-z0-9_-]{1,64})(?:\/(control|data))?$/u.exec(
      path,
    );
    const route = match ? routes.get(match[2]) : undefined;
    if (closing || !match || !route || request.headers.origin !== undefined) {
      reject(socket, 403);
      return;
    }
    const credential = bearer(request);
    if (!credential) {
      reject(socket, 401);
      return;
    }
    if (match[1] === 'reverse') {
      if (!timingSafeEqual(hash(credential), route.tokenHash)) {
        reject(socket, 401);
        return;
      }
      if (match[3] === 'control') {
        wss.handleUpgrade(request, socket, head, (control) => {
          control.on('error', () => {});
          if (route.controller) {
            const old = route.controller;
            offline(route, old);
            old.close(4009, 'Replaced by another connector');
          }
          route.controller = control;
          monitorReverseSocket(control);
          control.once('close', () => offline(route, control));
          control.on('message', (data, binary) => {
            try {
              if (binary || reverseFrameBytes(data).byteLength > 16_384)
                throw new Error('Invalid control frame');
              const frame = reverseControlMessage.parse(
                JSON.parse(reverseFrameBytes(data).toString()),
              );
              if (frame.kind !== 'reject') throw new Error('Invalid control reply');
              const pending = route.pending.get(frame.ticket);
              if (pending?.controller === control) {
                consume(route, frame.ticket);
                reject(pending.socket, frame.status);
              }
            } catch {
              control.terminate();
            }
          });
        });
        return;
      }
      if (match[3] === 'data') {
        const ticket = request.headers['x-maka-reverse-ticket'];
        const pending = typeof ticket === 'string' ? route.pending.get(ticket) : undefined;
        if (!pending || pending.controller !== route.controller || pending.socket.destroyed) {
          reject(socket, 403);
          return;
        }
        consume(route, ticket as string);
        // Do not complete the Client upgrade until the executing Host accepts its credential.
        try {
          wss.handleUpgrade(request, socket, head, (target) => {
            target.on('error', () => {});
            try {
              wss.handleUpgrade(pending.request, pending.socket, pending.head, (client) => {
                route.active.add(client);
                route.active.add(target);
                bridge(client, target, () => {
                  route.active.delete(client);
                  route.active.delete(target);
                });
              });
            } catch {
              target.terminate();
              pending.socket.destroy();
            }
          });
        } catch {
          socket.destroy();
          pending.socket.destroy();
        }
        return;
      }
      reject(socket, 404);
      return;
    }
    if (match[3]) {
      reject(socket, 404);
      return;
    }
    const control = route.controller;
    if (!control || control.readyState !== WebSocket.OPEN) {
      reject(socket, 503);
      return;
    }
    if (route.pending.size >= 16 || route.active.size >= 64) {
      reject(socket, 503);
      return;
    }
    const ticket = randomBytes(32).toString('base64url');
    const detached = () => {
      consume(route, ticket);
    };
    const timer = setTimeout(() => {
      consume(route, ticket);
      reject(socket, 504);
    }, 10_000);
    socket.once('close', detached);
    route.pending.set(ticket, {
      request,
      socket,
      head,
      controller: control,
      timer,
      detach: () => socket.off('close', detached),
    });
    control.send(JSON.stringify({ kind: 'open', ticket, credential }), (error) => {
      if (error) {
        consume(route, ticket);
        reject(socket, 503);
      }
    });
  });
  try {
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      server.listen(config.port, config.host, () => {
        server.off('error', reject);
        resolve();
      });
    });
  } catch (error) {
    wss.close();
    throw error;
  }
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('Missing gateway address');
  let closed: Promise<void> | undefined;
  return {
    endpoint: `${input.tls ? 'wss' : 'ws'}://${address.family === 'IPv6' ? `[${address.address}]` : address.address}:${address.port}`,
    close() {
      return (closed ??= (async () => {
        closing = true;
        for (const route of routes.values()) {
          if (route.controller) {
            const old = route.controller;
            offline(route, old);
            old.terminate();
          }
        }
        for (const socket of wss.clients) socket.terminate();
        await new Promise<void>((resolve) => wss.close(() => resolve()));
        server.closeAllConnections();
        await new Promise<void>((resolve, reject) =>
          server.close((error) => (error ? reject(error) : resolve())),
        );
      })());
    },
  };
}

function bridge(a: WebSocket, b: WebSocket, onClose: () => void): void {
  const stop = () => {
    a.terminate();
    b.terminate();
    onClose();
  };
  for (const [from, to] of [
    [a, b],
    [b, a],
  ]) {
    from.once('close', stop);
    from.on('error', stop);
    monitorReverseSocket(from);
    from.on('message', (data, binary) => {
      if (
        binary ||
        to.readyState !== WebSocket.OPEN ||
        to.bufferedAmount + reverseFrameBytes(data).byteLength > 2 * 1024 * 1024
      ) {
        stop();
        return;
      }
      to.send(data, { binary: false, compress: false }, (error) => {
        if (error) stop();
      });
    });
  }
}
function hash(value: string): Buffer {
  return createHash('sha256').update(value).digest();
}
function bearer(request: IncomingMessage): string | undefined {
  const raw = request.headers.authorization;
  return typeof raw === 'string' && /^Bearer [A-Za-z0-9_-]{1,8192}$/u.test(raw)
    ? raw.slice(7)
    : undefined;
}
function reject(socket: Duplex, status: number): void {
  if (socket.destroyed) return;
  socket.end(`HTTP/1.1 ${status} Rejected\r\nConnection: close\r\nContent-Length: 0\r\n\r\n`);
}
