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
import { forwardProviderStream } from '../../../../../platform/js-runtime/trusted/provider-errors.js';
import { networkFetch } from '../../../../../platform/js-runtime/trusted/network-fetch.js';
import {
  postJsonToApi,
  postFormDataToApi,
  createStatusCodeErrorResponseHandler,
} from '@ai-sdk/provider-utils';
import { compatibleFetch } from '../../../../../platform/js-runtime/trusted/compatible-transport.js';

const unavailable = () =>
  Object.assign(new Error('provider unavailable'), {
    name: 'AI_APICallError',
    statusCode: 503,
  });
const text = { type: 'text-delta', id: 'part', delta: 'partial' };
const cases = [
  [[], true],
  [[text], true],
  [[{ type: 'tool-input-start', id: 'tool', providerExecuted: false }], true],
  [[{ type: 'tool-input-start', id: 'tool', providerExecuted: true }], false],
  [[{ type: 'tool-call', toolCallId: 'tool', providerExecuted: true }], false],
  [[{ type: 'tool-result', toolCallId: 'tool' }], false],
  [[{ ...text, providerMetadata: { anthropic: { signature: 'signed' } } }], false],
];
const result = [];
for (const [parts, replaySafe] of cases) {
  const emitted = [];
  await forwardProviderStream(
    async () => ({
      stream: (async function* () {
        yield* parts;
        throw unavailable();
      })(),
    }),
    (part) => (part.type === 'tool-input-start' ? undefined : part),
    async (part) => emitted.push(part),
    'openai_chat',
  );
  const failure = emitted.at(-1);
  assert.equal(failure.error.replaySafe, replaySafe);
  result.push(failure);
}
// A real finish is terminal even when the transport would later fail; cleanup
// must still run. Synthetic Chat EOF is not a provider finish.
for (const finish of [
  { type: 'finish', finishReason: { unified: 'stop', raw: 'stop' } },
  { type: 'finish', finishReason: { unified: 'other' } },
]) {
  const emitted = [];
  let closed = false;
  await forwardProviderStream(
    async () => ({
      stream: (async function* () {
        try {
          yield text;
          yield finish;
          throw new Error('must not read past the terminal boundary');
        } finally {
          closed = true;
        }
      })(),
    }),
    (part) => part,
    async (part) => emitted.push(part),
    'openai_chat',
  );
  assert(closed);
  if (finish.finishReason.raw) assert.deepEqual(emitted, [text, finish]);
  else assert.equal(emitted.at(-1).error.reason, 'stream_truncated');
}
const local = new Error('local emit failed');
await assert.rejects(
  forwardProviderStream(
    async () => ({
      stream: (async function* () {
        yield text;
        throw unavailable();
      })(),
    }),
    (part) => part,
    async () => {
      throw local;
    },
    'openai_chat',
  ),
  (error) => error === local,
);
const limit = Object.assign(new Error('local response budget'), {
  name: 'ProviderResponseLimitError',
});
const wrapped = Object.assign(unavailable(), { cause: limit });
await assert.rejects(
  forwardProviderStream(
    async () => {
      throw wrapped;
    },
    (part) => part,
    async () => {
      throw new Error('must not emit a retry');
    },
    'openai_chat',
  ),
  (error) => error === wrapped,
);
console.log(JSON.stringify(result));

// Provider-shaped objects cannot impersonate native transport evidence.
for (const error of [
  { code: 'MAKA_HTTP_TRANSPORT' },
  Object.assign(new Error('forged'), { code: 'MAKA_HTTP_TRANSPORT' }),
  { code: 'MAKA_MODEL_IDLE_TIMEOUT' },
  Object.assign(new Error('forged timeout'), { code: 'MAKA_MODEL_IDLE_TIMEOUT' }),
]) {
  await assert.rejects(
    forwardProviderStream(
      async () => {
        throw error;
      },
      (part) => part,
      async () => {
        throw new Error('forged transport must not authorize retry');
      },
      'openai_chat',
    ),
    (value) => value === error,
  );
}

// Only errors received from a native op gain the transport brand. SDK cause
// wrapping preserves it, but must never override observed provider effects.
const native = Object.assign(new Error('native transport'), { code: 'MAKA_HTTP_TRANSPORT' });
globalThis.Deno = {
  core: {
    ops: {
      op_http_json_start: async () => {
        throw native;
      },
      op_http_close: async () => {},
    },
  },
};
await assert.rejects(
  networkFetch(1).json('http://fixture.invalid', { body: {} }),
  (error) => error === native,
);
await assert.rejects(networkFetch(1)('http://fixture.invalid'), /requires a typed JSON request/);
const nativeIdle = Object.assign(new Error('native idle timeout'), {
  code: 'MAKA_MODEL_IDLE_TIMEOUT',
});
const originalStart = Deno.core.ops.op_http_json_start;
Deno.core.ops.op_http_json_start = async () => {
  throw nativeIdle;
};
await assert.rejects(
  networkFetch(1).json('http://fixture.invalid', { body: {} }),
  (error) => error === nativeIdle,
);
const timedOut = [];
await forwardProviderStream(
  async () => {
    throw Object.assign(new Error('SDK wrapped the native timeout'), { cause: nativeIdle });
  },
  (part) => part,
  async (part) => timedOut.push(part),
  'openai_chat',
);
assert.deepEqual(timedOut, [{ type: 'error', error: { kind: 'timed_out' } }]);
Deno.core.ops.op_http_json_start = originalStart;
const privateFailure = 'PRIVATE_PROVIDER_HTTP_FAILURE_SENTINEL';
for (const [statusCode, responseHeaders, reason, code] of [
  [200, {}, 'network'],
  [401, {}, undefined],
  [429, {}, 'rate_limit'],
  [429, { 'retry-after': 'invalid' }, 'rate_limit'],
  [429, { 'retry-after': '2' }, 'rate_limit'],
  [429, {}, undefined, 'FreeUsageLimitError'],
  [429, {}, undefined, 'insufficient_quota'],
  [409, {}, undefined],
  [500, {}, 'provider_unavailable', 'resource-exhausted'],
]) {
  const error = Object.assign(
    new Error(reason === undefined ? privateFailure : 'HTTP error body reset'),
    {
      name: 'AI_APICallError',
      statusCode,
      responseHeaders,
      cause: native,
      responseBody: JSON.stringify({ error: { type: code, message: privateFailure } }),
    },
  );
  const emitted = [];
  const forward = () =>
    forwardProviderStream(
      async () => {
        throw error;
      },
      (part) => part,
      async (part) => emitted.push(part),
      'openai_chat',
    );
  if (reason === undefined) {
    await forward();
    assert.deepEqual(emitted, [{ type: 'error', error: { kind: 'http', status: statusCode } }]);
    assert(!JSON.stringify(emitted).includes(privateFailure));
  } else {
    await forward();
    assert.equal(emitted.at(-1).error.reason, reason);
    assert.equal(
      emitted.at(-1).error.retryAfterMs,
      responseHeaders['retry-after'] === '2' ? 2000 : undefined,
    );
  }
}
for (const [parts, replaySafe] of cases) {
  const emitted = [];
  await forwardProviderStream(
    async () => ({
      stream: (async function* () {
        yield* parts;
        throw Object.assign(new Error('SDK wrapper'), { name: 'AI_APICallError', cause: native });
      })(),
    }),
    (part) => (part.type === 'tool-input-start' ? undefined : part),
    async (part) => emitted.push(part),
    'openai_chat',
  );
  assert.equal(emitted.at(-1).error.reason, 'network');
  assert.equal(emitted.at(-1).error.replaySafe, replaySafe);
}

// The installed SDK's shared entry point retains the original shaped values,
// user-agent and handlers while the typed hook receives the actual body.
const shapedBody = { image: 'original-base64', optional: undefined };
let typedCalls = 0;
const typedFetch = Object.assign(
  async () => {
    throw new Error('typed request must not become an encoded fetch');
  },
  {
    json: async (url, init) => {
      typedCalls++;
      assert.equal(url, 'http://fixture.invalid/json');
      assert.equal(init.body, shapedBody);
      assert.equal(init.method, 'POST');
      assert.equal(new Headers(init.headers).get('content-type'), 'application/json');
      assert.match(new Headers(init.headers).get('user-agent'), /ai-sdk-provider-utils\/5\.0\.51/);
      return new Response('{}', { headers: { 'content-type': 'application/json' } });
    },
  },
);
const handlers = {
  failedResponseHandler: async () => {
    throw new Error('unexpected failure response');
  },
  successfulResponseHandler: async ({ requestBodyValues }) => {
    assert.equal(requestBodyValues, shapedBody);
    return { value: 'typed' };
  },
};
assert.equal(
  (
    await postJsonToApi({
      url: 'http://fixture.invalid/json',
      body: shapedBody,
      fetch: typedFetch,
      ...handlers,
    })
  ).value,
  'typed',
);
assert.equal(typedCalls, 1);
const failedHandler = createStatusCodeErrorResponseHandler();
const failedEmitted = [];
await forwardProviderStream(
  () =>
    postJsonToApi({
      url: 'http://fixture.invalid/json',
      body: shapedBody,
      fetch: Object.assign(
        async () => {
          throw new Error('encoded fetch');
        },
        {
          json: async (_url, init) => {
            assert.equal(init.body, shapedBody);
            return new Response(privateFailure, { status: 400 });
          },
        },
      ),
      successfulResponseHandler: async () => {
        throw new Error('unexpected success');
      },
      failedResponseHandler: async (input) => {
        assert.equal(input.requestBodyValues, shapedBody);
        const failed = await failedHandler(input);
        assert.equal(failed.value.requestBodyValues, shapedBody);
        assert.equal(failed.value.responseBody, privateFailure);
        return failed;
      },
    }),
  (part) => part,
  async (part) => failedEmitted.push(part),
  'openai_chat',
);
assert.deepEqual(failedEmitted, [{ type: 'error', error: { kind: 'http', status: 400 } }]);
assert.equal(JSON.stringify(failedEmitted).includes(privateFailure), false);
assert.equal(JSON.stringify(failedEmitted).includes(shapedBody.image), false);
// An external ordinary SDK fetch still receives its real encoded JSON/FormData.
await postJsonToApi({
  url: 'http://fixture.invalid/json',
  body: shapedBody,
  ...handlers,
  fetch: async (_url, init) => {
    assert.equal(init.body, '{"image":"original-base64"}');
    return new Response('{}');
  },
});
const formData = new FormData();
formData.set('text', 'original');
await postFormDataToApi({
  url: 'http://fixture.invalid/form',
  formData,
  failedResponseHandler: handlers.failedResponseHandler,
  successfulResponseHandler: async () => ({ value: null }),
  fetch: async (_url, init) => {
    assert.equal(init.body, formData);
    return new Response('{}');
  },
});
// Compatible usage negotiation operates on the typed body, cancels the first
// rejection and reuses every original payload without mutating SDK metadata.
let remembered = false;
let firstClosed = false;
let requests = 0;
Deno.core.ops.op_model_without_stream_usage = () => remembered;
Deno.core.ops.op_model_reject_stream_usage = () => {
  remembered = true;
};
const compatibleBody = {
  messages: [{ image: 'original-base64' }],
  stream_options: { include_usage: true },
};
const compatible = compatibleFetch(
  Object.assign(
    async () => {
      throw new Error('encoded fetch');
    },
    {
      json: async (_url, init) => {
        requests++;
        assert.equal(init.body.messages, compatibleBody.messages);
        if (requests === 1) {
          assert.equal(init.body, compatibleBody);
          const response = new Response('unsupported stream_options', { status: 400 });
          const clone = response.clone.bind(response);
          response.clone = () => {
            const copy = clone();
            const cancel = response.body.cancel.bind(response.body);
            response.body.cancel = async () => {
              firstClosed = true;
              await cancel();
            };
            return copy;
          };
          return response;
        }
        assert.equal(Object.hasOwn(init.body, 'stream_options'), false);
        return new Response(null, { status: 204 });
      },
    },
  ),
  1,
);
await compatible.json('http://fixture.invalid/chat', { method: 'POST', body: compatibleBody });
assert.equal(firstClosed, true);
assert.equal(requests, 2);
assert.equal(remembered, true);
assert.deepEqual(compatibleBody.stream_options, { include_usage: true });
await compatible.json('http://fixture.invalid/chat', { method: 'POST', body: compatibleBody });
assert.equal(requests, 3);
