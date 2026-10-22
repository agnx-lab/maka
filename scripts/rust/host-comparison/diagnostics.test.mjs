#!/usr/bin/env node
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
import test from 'node:test';
import { diagnosticError } from './diagnostics.mjs';

test('preserves connection interruption custody and its actual nested Error cause', () => {
  const cause = Object.assign(
    new Error('Timed out waiting for Runtime Host host.status response'),
    {
      name: 'RuntimeHostTransportError',
      code: 'read_timeout',
    },
  );
  const error = Object.assign(new Error('Runtime Host turn.query was interrupted', { cause }), {
    name: 'RuntimeHostRequestInterruptedError',
    operation: 'turn.query',
    mode: 'query',
    dispatch: 'dispatched',
    reason: 'connection_lost',
    retryable: true,
  });
  assert.deepEqual(JSON.parse(JSON.stringify(diagnosticError(error))), {
    name: 'RuntimeHostRequestInterruptedError',
    message: 'Runtime Host turn.query was interrupted',
    code: null,
    operation: 'turn.query',
    mode: 'query',
    dispatch: 'dispatched',
    reason: 'connection_lost',
    retryable: true,
    cause: {
      name: 'RuntimeHostTransportError',
      message: 'Timed out waiting for Runtime Host host.status response',
      code: 'read_timeout',
    },
  });
});

test('retains a non-dispatched mutation independently of its connection failure', () => {
  const error = Object.assign(
    new Error('operation outcome is unknown', {
      cause: Object.assign(new Error('socket closed'), { code: 'ECONNRESET' }),
    }),
    {
      operation: 'turn.submit',
      mode: 'mutation',
      dispatch: 'not_dispatched',
      reason: 'connection_lost',
      retryable: false,
    },
  );
  const actual = diagnosticError(error);
  assert.equal(actual.dispatch, 'not_dispatched');
  assert.equal(actual.retryable, false);
  assert.equal(actual.cause.message, 'socket closed');
  assert.equal(actual.cause.code, 'ECONNRESET');
});

test('circular and non-JSON causes still produce useful JSON diagnostics', () => {
  const error = Object.assign(new Error('outer'), { code: 42n });
  error.cause = error;
  assert.deepEqual(JSON.parse(JSON.stringify(diagnosticError(error))), {
    name: 'Error',
    message: 'outer',
    code: '42',
    cause: { message: '[circular error cause]', code: null },
  });
  assert.equal(
    diagnosticError(new Error('outer', { cause: Symbol('detail') })).cause.message,
    'Symbol(detail)',
  );
});

test('throwing accessors cannot suppress the original error response', () => {
  const error = new Error('query failed');
  Object.defineProperty(error, 'code', {
    get() {
      throw new Error('getter failed');
    },
  });
  Object.defineProperty(error, 'stack', {
    get() {
      throw new Error('stack must not be read');
    },
  });
  assert.deepEqual(diagnosticError(error), {
    name: 'Error',
    message: 'query failed',
    code: '[unreadable property]',
  });
});

test('bounds deep oversized causes while retaining the outer failure', () => {
  let error = new Error('deepest cause');
  for (let i = 0; i < 100; i += 1)
    error = new Error(`connection lost ${'x'.repeat(20000)}`, { cause: error });
  const encoded = JSON.stringify(diagnosticError(error));
  assert.ok(encoded.includes('connection lost'));
  assert.ok(encoded.includes('[error cause depth limit]'));
  assert.ok(encoded.length < 50000, `diagnostic bytes: ${encoded.length}`);
});

test('non-Error rejections remain diagnostic values', () => {
  assert.deepEqual(diagnosticError(null), { name: 'object', message: 'null', code: null });
  assert.deepEqual(diagnosticError('connection closed'), {
    name: 'string',
    message: 'connection closed',
    code: null,
  });
});
