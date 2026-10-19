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
import { test } from 'node:test';
import net from 'node:net';
import WebSocket from 'ws';
import {
  openReverseSocket,
  reverseConnectionSchema,
} from '../transport/reverse-websocket-protocol.js';
import { randomBytes } from 'node:crypto';
import { mkdtemp, readFile, rm, writeFile, chmod } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import {
  resolveStorageRoot,
  prepareStorageRootControlDirectory,
} from '@maka/storage/root-authority';
import { waitFor } from '@maka/core/test-only/async-primitives';
import {
  connectRemoteRuntimeHost,
  connectRuntimeHost,
  consumeAccessCredentialDelivery,
  type RuntimeHostConnection,
} from '../client/index.js';
import {
  INTERACTIVE_RUNTIME_HOST_COMPOSITION_ID,
  RUNTIME_HOST_PROTOCOL_VERSION,
} from '../protocol/index.js';
import { startExecutionRuntimeHostService } from '../server/execution-service.js';
import { startReverseRuntimeHostGateway } from '../server/reverse-websocket-gateway.js';
import {
  readReverseConnectionConfiguration,
  writeReverseConnectionConfiguration,
  removeReverseConnectionConfiguration,
} from '../server/reverse-connection-config.js';
const PROTOCOL = { min: RUNTIME_HOST_PROTOCOL_VERSION, max: RUNTIME_HOST_PROTOCOL_VERSION };
function requireConnection(
  result: Awaited<ReturnType<typeof connectRuntimeHost>>,
): RuntimeHostConnection {
  if (result.kind !== 'connected') throw new Error('Local connection failed');
  return result.connection;
}
async function configureTestModel(local: RuntimeHostConnection): Promise<void> {
  const catalog = await local.request('connection.catalog.query', { kind: 'start' });
  assert.equal(catalog.kind, 'page');
  if (catalog.kind !== 'page') assert.fail('Expected the initial catalog page');
  const created = await local.request('connection.catalog.create', {
    expectedCatalogRevision: catalog.revision,
    connection: {
      slug: 'websocket-fixture',
      name: 'WebSocket fixture',
      providerType: 'custom',
      defaultApiProtocol: 'openai-chat',
      baseUrl: 'https://websocket-model.invalid/v1',
      enabled: true,
      enabledModelIds: ['websocket-test-model'],
    },
  });
  if (created.kind !== 'committed') assert.fail('Test connection must commit');
  const connectionId = created.connection.connectionId;
  const credential = await local.request('credential.vault.set', {
    locator: { scope: 'connection', connectionId, kind: 'api_key' },
    expected: null,
    secret: 'websocket-fixture-key',
  });
  assert.equal(credential.kind, 'committed');
  const selected = await local.request('connection.catalog.set-default-target', {
    expectedCatalogRevision: created.catalogRevision,
    target: { connectionId, modelId: 'websocket-test-model' },
  });
  assert.equal(selected.kind, 'committed');
}

test('reverse Host uses normal tools, preserves receipts, and resumes after gateway and Host restart', {
  timeout: 120000,
}, async () => {
  const base = await mkdtemp(join(tmpdir(), 'maka-remote-tools-wire-'));
  const root = join(base, 'target');
  const capability = await resolveStorageRoot({ path: root, kind: 'interactive' });
  const token = randomBytes(32).toString('base64url');
  let gateway = await startReverseRuntimeHostGateway({
    configuration: {
      schemaVersion: 1,
      host: '127.0.0.1',
      port: 0,
      routes: [{ route: 'target', token }],
    },
  });
  const { controlDirectory } = await prepareStorageRootControlDirectory(capability);
  await writeReverseConnectionConfiguration(controlDirectory, {
    schemaVersion: 1,
    url: gateway.endpoint,
    route: 'target',
    token,
  });
  let host = await startExecutionRuntimeHostService({ rootPath: root });
  let local: RuntimeHostConnection | undefined, remote: RuntimeHostConnection | undefined;
  try {
    local = requireConnection(await connectRuntimeHost({ rootPath: root, protocol: PROTOCOL }));
    await configureTestModel(local);
    const template = await local.request('session.create', {
      sessionId: 'remote-template',
      workspace: { kind: 'host_path', path: root },
      modelTarget: { kind: 'default' },
      permissionMode: 'bypass',
    });
    assert.ok(!('kind' in template));
    const issued = await local.request('access.credential.issue', {
      principalKind: 'remote_owner',
      principalId: 'remote-tools-test',
      operationGrants: ['remote.tools'],
      canPublishClientCapabilities: false,
      canUseHostPaths: false,
    });
    const credential = await consumeAccessCredentialDelivery(
      root,
      issued.deliveryId,
      issued.credentialId,
    );
    const connect = async () => {
      let result: Awaited<ReturnType<typeof connectRemoteRuntimeHost>>;
      await waitFor(
        async () => {
          result = await connectRemoteRuntimeHost({
            url: `${gateway.endpoint}/runtime-host/target`,
            credential,
            expectedRootId: capability.rootId,
            compositionId: INTERACTIVE_RUNTIME_HOST_COMPOSITION_ID,
            protocol: PROTOCOL,
          });
          return result.kind === 'connected';
        },
        { timeoutMs: 10000 },
      );
      if (result!.kind !== 'connected') throw Error('Not connected');
      return result!.connection;
    };
    remote = await connect();
    const unauthorized = await connectRemoteRuntimeHost({
      url: `${gateway.endpoint}/runtime-host/target`,
      credential: 'wrong',
      expectedRootId: capability.rootId,
      compositionId: INTERACTIVE_RUNTIME_HOST_COMPOSITION_ID,
      protocol: PROTOCOL,
    });
    assert.deepEqual(unauthorized, { kind: 'unavailable', reason: 'authentication_failed' });
    const wrongRoot = await connectRemoteRuntimeHost({
      url: `${gateway.endpoint}/runtime-host/target`,
      credential,
      expectedRootId: 'f'.repeat(64),
      compositionId: INTERACTIVE_RUNTIME_HOST_COMPOSITION_ID,
      protocol: PROTOCOL,
    });
    assert.deepEqual(wrongRoot, { kind: 'unavailable', reason: 'root_mismatch' });
    await assert.rejects(
      remote.request('host.diagnostics.query', {}),
      (error: any) => error.code === 'unauthorized',
    );
    const opened = (
      await remote.request('remote.tools', {
        action: 'open',
        originHost: 'origin-host',
        originSession: 'origin-bot',
        templateSession: 'remote-template',
      })
    ).value as { id: string };
    const configureDiscovery = {
      action: 'call' as const,
      contextId: opened.id,
      origin: { hostId: 'origin-host', sessionId: 'origin-bot' },
      callId: 'configure-discovery',
      tool: 'InitiativeConfigure',
      arguments: { intervalMinutes: 77 },
    };
    const configReceipt = (await remote.request('remote.tools', configureDiscovery)).value as any;
    assert.equal(configReceipt.result.result.intervalMs, 77 * 60000);
    await writeFile(join(root, 'evidence.txt'), 'REMOTE WIRE EVIDENCE');
    const call = {
      action: 'call' as const,
      contextId: opened.id,
      origin: { hostId: 'origin-host', sessionId: 'origin-bot' },
      callId: 'wire-read',
      tool: 'Read',
      arguments: { path: 'evidence.txt' },
    };
    let receipt = (await remote.request('remote.tools', call)).value as any;
    await waitFor(async () => {
      receipt = (
        await remote!.request('remote.tools', {
          action: 'result',
          contextId: opened.id,
          origin: { hostId: 'origin-host', sessionId: 'origin-bot' },
          callId: 'wire-read',
        })
      ).value;
      return receipt.status === 'completed';
    });
    assert.match(JSON.stringify(receipt), /REMOTE WIRE EVIDENCE/);
    // Real gateway loss must close the client, but not stop the target Host.
    const port = Number(new URL(gateway.endpoint).port);
    await gateway.close();
    await remote.closed;
    const localProjects = await local.request('project.catalog.query', {
      kind: 'list_start',
      view: 'summary',
    });
    assert.equal(localProjects.kind, 'page');
    gateway = await startReverseRuntimeHostGateway({
      configuration: {
        schemaVersion: 1,
        host: '127.0.0.1',
        port,
        routes: [{ route: 'target', token }],
      },
    });
    remote = await connect();
    assert.deepEqual((await remote.request('remote.tools', call)).value, receipt);
    await remote.close();
    remote = undefined;
    await local.close();
    local = undefined;
    await host.close();
    host = await startExecutionRuntimeHostService({ rootPath: root });
    remote = await connect();
    const recoveredDiscovery = (
      await remote.request('remote.tools', {
        ...configureDiscovery,
        callId: 'discovery-status',
        tool: 'InitiativeStatus',
        arguments: {},
      })
    ).value as any;
    assert.equal(recoveredDiscovery.result.result.intervalMs, 77 * 60000);
    assert.equal(recoveredDiscovery.result.result.enabled, false);
    await writeFile(join(root, 'evidence.txt'), 'CHANGED AFTER RESTART');
    assert.deepEqual((await remote.request('remote.tools', call)).value, receipt);
    assert.match(
      JSON.stringify(
        (await remote.request('remote.tools', { ...call, callId: 'fresh-read' })).value,
      ),
      /CHANGED AFTER RESTART/,
    );
    local = requireConnection(await connectRuntimeHost({ rootPath: root, protocol: PROTOCOL }));
    await local.request('access.credential.revoke', { credentialId: issued.credentialId });
    await remote.closed;
    const revoked = await connectRemoteRuntimeHost({
      url: `${gateway.endpoint}/runtime-host/target`,
      credential,
      expectedRootId: capability.rootId,
      compositionId: INTERACTIVE_RUNTIME_HOST_COMPOSITION_ID,
      protocol: PROTOCOL,
    });
    assert.deepEqual(revoked, { kind: 'unavailable', reason: 'authentication_failed' });
  } finally {
    await remote?.close();
    await local?.close();
    await host.close();
    await gateway.close();
    await rm(base, { recursive: true, force: true });
  }
});

test('reverse connection loss after a side effect recovers the receipt without repeating work', {
  timeout: 30000,
}, async () => {
  const base = await mkdtemp(join(tmpdir(), 'maka-remote-loss-'));
  const root = join(base, 'target');
  const capability = await resolveStorageRoot({ path: root, kind: 'interactive' });
  const token = randomBytes(32).toString('base64url');
  const gateway = await startReverseRuntimeHostGateway({
    configuration: {
      schemaVersion: 1,
      host: '127.0.0.1',
      port: 0,
      routes: [{ route: 'target', token }],
    },
  });
  const { controlDirectory } = await prepareStorageRootControlDirectory(capability);
  await writeReverseConnectionConfiguration(controlDirectory, {
    schemaVersion: 1,
    url: gateway.endpoint,
    route: 'target',
    token,
  });
  const host = await startExecutionRuntimeHostService({ rootPath: root });
  let local: RuntimeHostConnection | undefined, remote: RuntimeHostConnection | undefined;
  let blackhole = false;
  const sockets = new Set<net.Socket>();
  const targetUrl = new URL(`${gateway.endpoint}/runtime-host/target`);
  const proxy = net.createServer((client) => {
    const upstream = net.connect(Number(targetUrl.port), '127.0.0.1');
    sockets.add(client);
    sockets.add(upstream);
    client.pipe(upstream);
    upstream.on('data', (chunk) => {
      if (!blackhole) client.write(chunk);
    });
    client.on('close', () => {
      sockets.delete(client);
      upstream.destroy();
    });
    upstream.on('close', () => {
      sockets.delete(upstream);
      client.destroy();
    });
    client.on('error', () => upstream.destroy());
    upstream.on('error', () => client.destroy());
  });
  await new Promise<void>((resolve) => proxy.listen(0, '127.0.0.1', resolve));
  const proxyUrl = new URL(targetUrl);
  proxyUrl.port = String((proxy.address() as net.AddressInfo).port);
  try {
    local = requireConnection(await connectRuntimeHost({ rootPath: root, protocol: PROTOCOL }));
    await configureTestModel(local);
    await local.request('session.create', {
      sessionId: 'template',
      workspace: { kind: 'host_path', path: root },
      modelTarget: { kind: 'default' },
      permissionMode: 'bypass',
    });
    const issued = await local.request('access.credential.issue', {
      principalKind: 'remote_owner',
      principalId: 'loss-test',
      operationGrants: ['remote.tools'],
      canPublishClientCapabilities: false,
      canUseHostPaths: false,
    });
    const credential = await consumeAccessCredentialDelivery(
      root,
      issued.deliveryId,
      issued.credentialId,
    );
    const connect = async (url: string) => {
      const result = await connectRemoteRuntimeHost({
        url,
        credential,
        expectedRootId: capability.rootId,
        compositionId: INTERACTIVE_RUNTIME_HOST_COMPOSITION_ID,
        protocol: PROTOCOL,
      });
      if (result.kind !== 'connected') throw Error(JSON.stringify(result));
      return result.connection;
    };
    remote = await connect(proxyUrl.toString());
    const opened = (
      await remote.request('remote.tools', {
        action: 'open',
        originHost: 'origin',
        originSession: 'bot',
        templateSession: 'template',
      })
    ).value as any;
    const call = {
      action: 'call' as const,
      contextId: opened.id,
      origin: { hostId: 'origin', sessionId: 'bot' },
      callId: 'lost-response',
      tool: 'Bash',
      arguments: { command: 'echo once >> effect.txt; sleep 0.5; echo completed' },
    };
    const response = remote.request('remote.tools', call, 250);
    const rejected = assert.rejects(response, (error: any) => {
      assert.match(String(error.cause), /timed out/i);
      assert.match(error.message, /outcome is unknown/);
      return true;
    });
    await waitFor(
      async () => {
        try {
          return (await readFile(join(root, 'effect.txt'), 'utf8')).includes('once');
        } catch {
          return false;
        }
      },
      { timeoutMs: 3000 },
    );
    // The effect is confirmed before discarding the server's final response bytes.
    blackhole = true;
    await rejected;
    await remote.close();
    remote = await connect(targetUrl.toString());
    let receipt: any;
    await waitFor(
      async () => {
        receipt = (
          await remote!.request('remote.tools', {
            action: 'result',
            contextId: opened.id,
            origin: call.origin,
            callId: call.callId,
          })
        ).value;
        return receipt.status === 'completed';
      },
      { timeoutMs: 5000 },
    );
    assert.deepEqual((await remote.request('remote.tools', call)).value, receipt);
    assert.equal(await readFile(join(root, 'effect.txt'), 'utf8'), 'once\n');
    // A later healthy request still succeeds after the failed connection.
    const next = (
      await remote.request('remote.tools', {
        ...call,
        callId: 'next',
        tool: 'Read',
        arguments: { path: 'effect.txt' },
      })
    ).value;
    assert.match(JSON.stringify(next), /once/);
  } finally {
    for (const socket of sockets) socket.destroy();
    await new Promise<void>((resolve) => proxy.close(() => resolve()));
    await remote?.close();
    await local?.close();
    await host.close();
    await gateway.close();
    await rm(base, { recursive: true, force: true });
  }
});

test('reverse gateway rejects unregistered routes, incorrect connector credentials, browser origins, and ticket replay', async () => {
  const token = randomBytes(32).toString('base64url');
  const gateway = await startReverseRuntimeHostGateway({
    configuration: {
      schemaVersion: 1,
      host: '127.0.0.1',
      port: 0,
      routes: [{ route: 'target', token }],
    },
  });
  const rejectCode = (path: string, headers: Record<string, string>) =>
    new Promise<number>((resolve, reject) => {
      const ws = new WebSocket(`${gateway.endpoint}${path}`, { headers });
      ws.on('error', () => {});
      ws.once('open', () => {
        ws.terminate();
        reject(new Error('Unexpected acceptance'));
      });
      ws.once('unexpected-response', (_req, response) => {
        response.resume();
        ws.terminate();
        resolve(response.statusCode!);
      });
    });
  try {
    assert.equal(
      await rejectCode('/reverse/target/control', { Authorization: 'Bearer wrong' }),
      401,
    );
    assert.equal(
      await rejectCode('/reverse/missing/control', { Authorization: `Bearer ${token}` }),
      403,
    );
    assert.equal(
      await rejectCode('/reverse/target/control', {
        Authorization: `Bearer ${token}`,
        Origin: 'https://example.com',
      }),
      403,
    );
    assert.equal(
      await rejectCode('/reverse/target/data', {
        Authorization: `Bearer ${token}`,
        'X-Maka-Reverse-Ticket': token,
      }),
      403,
    );
    assert.equal(
      await rejectCode('/runtime-host/target', { Authorization: 'Bearer unverified' }),
      503,
    );
  } finally {
    await gateway.close();
  }
});

test('reverse config persists, rejects unsafe transport, and disables without touching Host data', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'maka-reverse-config-'));
  const config = {
    schemaVersion: 1,
    url: 'wss://example.com',
    route: 'laptop',
    token: randomBytes(32).toString('base64url'),
  };
  try {
    assert.equal(await readReverseConnectionConfiguration(dir), undefined);
    await writeReverseConnectionConfiguration(dir, config);
    assert.deepEqual(await readReverseConnectionConfiguration(dir), config);
    for (const url of [
      'ws://example.com',
      'wss://user:secret@example.com',
      'wss://example.com?token=secret',
      'wss://example.com/path',
      'https://example.com',
    ]) {
      assert.throws(() => reverseConnectionSchema.parse({ ...config, url }));
    }
    if (process.platform !== 'win32') {
      await chmod(join(dir, 'reverse-connection.json'), 0o644);
      await assert.rejects(readReverseConnectionConfiguration(dir), /0600/);
      await chmod(join(dir, 'reverse-connection.json'), 0o600);
    }
    await writeFile(join(dir, 'unrelated'), 'keep');
    await removeReverseConnectionConfiguration(dir);
    assert.equal(await readReverseConnectionConfiguration(dir), undefined);
    assert.equal(await readFile(join(dir, 'unrelated'), 'utf8'), 'keep');
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test('a duplicate connector replaces the old connection without a reconnect fight', async () => {
  const token = randomBytes(32).toString('base64url');
  const gateway = await startReverseRuntimeHostGateway({
    configuration: {
      schemaVersion: 1,
      host: '127.0.0.1',
      port: 0,
      routes: [{ route: 'target', token }],
    },
  });
  const { startReverseRuntimeHostListener } = await import(
    '../server/reverse-websocket-listener.js'
  );
  const states: string[] = [];
  const connector = startReverseRuntimeHostListener({
    configuration: { schemaVersion: 1, url: gateway.endpoint, route: 'target', token },
    accessAuthority: { authenticate: () => undefined },
    isReady: () => true,
    accept: () => assert.fail('Must not admit without a Host credential'),
    onStatus: (status) => states.push(status),
  });
  const lifetime = new AbortController();
  let replacement: WebSocket | undefined;
  try {
    await waitFor(() => states.includes('connected'), { timeoutMs: 3000 });
    replacement = await openReverseSocket(
      `${gateway.endpoint}/reverse/target/control`,
      { Authorization: `Bearer ${token}` },
      lifetime.signal,
    );
    await waitFor(() => states.includes('stopped'), { timeoutMs: 3000 });
    assert.equal(states.filter((s) => s === 'connecting').length, 1);
    assert.equal(replacement.readyState, WebSocket.OPEN);
  } finally {
    lifetime.abort();
    await connector.cleanup();
    await gateway.close();
  }
});

test('starting a Host while its reverse gateway is offline does not block local work', async () => {
  const base = await mkdtemp(join(tmpdir(), 'maka-reverse-offline-'));
  const root = join(base, 'target');
  const capability = await resolveStorageRoot({ path: root, kind: 'interactive' });
  const { controlDirectory } = await prepareStorageRootControlDirectory(capability);
  const token = randomBytes(32).toString('base64url');
  const gateway = await startReverseRuntimeHostGateway({
    configuration: {
      schemaVersion: 1,
      host: '127.0.0.1',
      port: 0,
      routes: [{ route: 'target', token }],
    },
  });
  const url = gateway.endpoint;
  await gateway.close();
  await writeReverseConnectionConfiguration(controlDirectory, {
    schemaVersion: 1,
    url,
    route: 'target',
    token,
  });
  const host = await startExecutionRuntimeHostService({ rootPath: root });
  let local: RuntimeHostConnection | undefined;
  try {
    local = requireConnection(await connectRuntimeHost({ rootPath: root, protocol: PROTOCOL }));
    assert.equal(
      (await local.request('project.catalog.query', { kind: 'list_start', view: 'summary' })).kind,
      'page',
    );
  } finally {
    await local?.close();
    await host.close();
    await rm(base, { recursive: true, force: true });
  }
});

test('two reverse Hosts route independently, including Desktop candidate startup', {
  timeout: 20000,
}, async () => {
  const { startInteractiveRuntimeHostCandidate } = await import('../server/candidate.js');
  const { createExecutionRuntimeHostCompositionSource } = await import(
    '../server/execution-composition-factory.js'
  );
  const base = await mkdtemp(join(tmpdir(), 'maka-reverse-two-'));
  const routes = ['office', 'home'].map((route) => ({
    route,
    token: randomBytes(32).toString('base64url'),
  }));
  const gateway = await startReverseRuntimeHostGateway({
    configuration: { schemaVersion: 1, host: '127.0.0.1', port: 0, routes },
  });
  const hosts: Awaited<ReturnType<typeof startExecutionRuntimeHostService>>[] = [];
  const connections: RuntimeHostConnection[] = [];
  const entries: {
    route: string;
    rootId: string;
    credential: string;
    remote: RuntimeHostConnection;
  }[] = [];
  try {
    for (const [i, route] of routes.entries()) {
      const root = join(base, route.route);
      const capability = await resolveStorageRoot({ path: root, kind: 'interactive' });
      const { controlDirectory } = await prepareStorageRootControlDirectory(capability);
      await writeReverseConnectionConfiguration(controlDirectory, {
        schemaVersion: 1,
        url: gateway.endpoint,
        ...route,
      });
      if (i === 0) hosts.push(await startExecutionRuntimeHostService({ rootPath: root }));
      else {
        const candidate = await startInteractiveRuntimeHostCandidate(
          { rootPath: root, expectedRootId: capability.rootId },
          () => createExecutionRuntimeHostCompositionSource({}),
        );
        if (candidate.kind !== 'winner') throw new Error('Expected independent Desktop Host root');
        hosts.push(candidate.host);
      }
      const local = requireConnection(
        await connectRuntimeHost({ rootPath: root, protocol: PROTOCOL }),
      );
      connections.push(local);
      const issued = await local.request('access.credential.issue', {
        principalKind: 'remote_owner',
        principalId: 'controller',
        operationGrants: ['project.catalog.query'],
        canPublishClientCapabilities: false,
        canUseHostPaths: false,
      });
      const credential = await consumeAccessCredentialDelivery(
        root,
        issued.deliveryId,
        issued.credentialId,
      );
      let remote: RuntimeHostConnection | undefined;
      await waitFor(
        async () => {
          const result = await connectRemoteRuntimeHost({
            url: `${gateway.endpoint}/runtime-host/${route.route}`,
            credential,
            expectedRootId: capability.rootId,
            compositionId: INTERACTIVE_RUNTIME_HOST_COMPOSITION_ID,
            protocol: PROTOCOL,
          });
          if (result.kind === 'connected') remote = result.connection;
          return !!remote;
        },
        { timeoutMs: 5000 },
      );
      connections.push(remote!);
      entries.push({ route: route.route, rootId: capability.rootId, credential, remote: remote! });
    }
    assert.notEqual(entries[0].remote.rootId, entries[1].remote.rootId);
    const crossed = await connectRemoteRuntimeHost({
      url: `${gateway.endpoint}/runtime-host/home`,
      credential: entries[0].credential,
      expectedRootId: entries[1].rootId,
      compositionId: INTERACTIVE_RUNTIME_HOST_COMPOSITION_ID,
      protocol: PROTOCOL,
    });
    assert.deepEqual(crossed, { kind: 'unavailable', reason: 'authentication_failed' });
    await hosts[0].close();
    await entries[0].remote.closed;
    assert.equal(
      (
        await entries[1].remote.request('project.catalog.query', {
          kind: 'list_start',
          view: 'summary',
        })
      ).kind,
      'page',
    );
  } finally {
    for (const connection of connections) await connection.close();
    for (const host of hosts) await host.close();
    await gateway.close();
    await rm(base, { recursive: true, force: true });
  }
});

test('reverse socket installs its reader before an immediate first protocol frame', async () => {
  const { WebSocketServer } = await import('ws');
  const { WebSocketTransport } = await import('../transport/websocket-transport.js');
  const server = new WebSocketServer({ host: '127.0.0.1', port: 0 });
  await new Promise<void>((resolve) => server.once('listening', () => resolve()));
  server.on('connection', (socket) => socket.send(JSON.stringify({ first: 'frame' })));
  const address = server.address();
  if (typeof address === 'string' || !address) throw new Error('Missing test port');
  const lifetime = new AbortController();
  let transport: InstanceType<typeof WebSocketTransport> | undefined;
  try {
    await openReverseSocket(`ws://127.0.0.1:${address.port}`, {}, lifetime.signal, (socket) => {
      transport = new WebSocketTransport(socket);
    });
    assert.deepEqual(await transport!.read(1000), { first: 'frame' });
  } finally {
    lifetime.abort();
    for (const socket of server.clients) socket.terminate();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});
