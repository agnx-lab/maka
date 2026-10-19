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

import { setMaxListeners } from 'node:events';
import { setTimeout as delay } from 'node:timers/promises';
import WebSocket from 'ws';
import { WebSocketTransport } from '../transport/websocket-transport.js';
import {
  openReverseSocket,
  reverseConnectionSchema,
  reverseControlMessage,
  reverseFrameBytes,
  type ReverseConnectionConfiguration,
} from '../transport/reverse-websocket-protocol.js';
import type { RuntimeHostAccessAuthority } from './access-authority.js';
import type { RuntimeHostListener, RuntimeHostListenerConnection } from './listener-set.js';

export interface ReverseRuntimeHostOptions {
  readonly configuration: ReverseConnectionConfiguration;
  readonly accessAuthority: Pick<RuntimeHostAccessAuthority, 'authenticate'>;
  readonly isReady: () => boolean;
  readonly accept: (connection: RuntimeHostListenerConnection) => void;
  readonly onStatus?: (status: 'connecting' | 'connected' | 'offline' | 'stopped') => void;
}

/** Network dialer, protocol server: authority is still checked by the executing Host. */
export function startReverseRuntimeHostListener(
  options: ReverseRuntimeHostOptions,
): RuntimeHostListener {
  const config = reverseConnectionSchema.parse(options.configuration);
  const lifetime = new AbortController();
  let admitting = true;
  const sockets = new Set<WebSocket>();
  const endpoint = `${config.url}/reverse/${config.route}/control`;
  const report = (status: 'connecting' | 'connected' | 'offline' | 'stopped') => {
    try {
      options.onStatus?.(status);
    } catch {
      /* Observers cannot stop reconnection. */
    }
  };
  const run = async () => {
    let failures = 0;
    while (!lifetime.signal.aborted) {
      report('connecting');
      const generation = new AbortController();
      setMaxListeners(40, generation.signal);
      const stopGeneration = () => generation.abort();
      lifetime.signal.addEventListener('abort', stopGeneration, { once: true });
      let control: WebSocket | undefined;
      const dataSockets = new Set<WebSocket>();
      const tickets = new Set<string>();
      try {
        let began = Date.now();
        let controlClosed!: Promise<number>;
        control = await openReverseSocket(
          endpoint,
          { ...config.headers, Authorization: `Bearer ${config.token}` },
          generation.signal,
          (current) => {
            sockets.add(current);
            began = Date.now();
            controlClosed = new Promise<number>((resolve) =>
              current.once('close', (code) => resolve(code)),
            );
            current.on('message', (data, binary) => {
              try {
                if (binary || reverseFrameBytes(data).byteLength > 16_384)
                  throw new Error('Invalid control frame');
                const message = reverseControlMessage.parse(
                  JSON.parse(reverseFrameBytes(data).toString()),
                );
                if (message.kind !== 'open' || tickets.has(message.ticket))
                  throw new Error('Invalid control request');
                // Bound work per control connection; tickets are removed on data close.
                if (tickets.size >= 32 || !admitting || !options.isReady()) {
                  current.send(
                    JSON.stringify({ kind: 'reject', ticket: message.ticket, status: 503 }),
                  );
                  return;
                }
                const authority = options.accessAuthority.authenticate(message.credential);
                if (!authority) {
                  current.send(
                    JSON.stringify({ kind: 'reject', ticket: message.ticket, status: 401 }),
                  );
                  return;
                }
                tickets.add(message.ticket);
                void (async () => {
                  let socket: WebSocket | undefined;
                  try {
                    socket = await openReverseSocket(
                      `${config.url}/reverse/${config.route}/data`,
                      {
                        ...config.headers,
                        Authorization: `Bearer ${config.token}`,
                        'X-Maka-Reverse-Ticket': message.ticket,
                      },
                      generation.signal,
                      (connected) => {
                        if (generation.signal.aborted) {
                          connected.terminate();
                          return;
                        }
                        sockets.add(connected);
                        dataSockets.add(connected);
                        connected.once('close', () => {
                          sockets.delete(connected);
                          dataSockets.delete(connected);
                          tickets.delete(message.ticket);
                        });
                        const currentAuthority = options.accessAuthority.authenticate(
                          message.credential,
                        );
                        if (!currentAuthority) {
                          connected.terminate();
                          return;
                        }
                        options.accept({
                          transport: new WebSocketTransport(connected),
                          authority: currentAuthority,
                        });
                      },
                    );
                  } catch {
                    socket?.terminate();
                    tickets.delete(message.ticket);
                    if (current.readyState === WebSocket.OPEN)
                      current.send(
                        JSON.stringify({ kind: 'reject', ticket: message.ticket, status: 503 }),
                      );
                  }
                })();
              } catch {
                current.terminate();
              }
            });
            report('connected');
          },
        );
        if (lifetime.signal.aborted) {
          control.terminate();
          break;
        }
        const closeCode = await controlClosed;
        // A replacement is deliberate; do not fight the newer connector forever.
        if (closeCode === 4009) break;
        if (Date.now() - began > 30_000) failures = 0;
      } catch {
        /* Report state, never credentials or proxy response bodies. */
      } finally {
        generation.abort();
        lifetime.signal.removeEventListener('abort', stopGeneration);
        if (control) {
          sockets.delete(control);
          control.terminate();
        }
        for (const socket of dataSockets) socket.terminate();
      }
      if (!lifetime.signal.aborted) {
        report('offline');
        const backoff = Math.min(30_000, 500 * 2 ** Math.min(failures++, 6));
        await delay(backoff + Math.floor(Math.random() * 250), undefined, {
          signal: lifetime.signal,
        }).catch(() => {});
      }
    }
    report('stopped');
  };
  const running = run();
  const close = async () => {
    lifetime.abort();
    for (const socket of sockets) socket.terminate();
    await running;
  };
  return {
    kind: 'reverse_websocket',
    endpoint,
    closeAdmission: async () => {
      admitting = false;
    },
    cleanup: close,
  };
}
