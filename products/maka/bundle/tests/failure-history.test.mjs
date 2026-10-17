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
import Ajv from 'ajv/dist/2020.js';
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'dark' }).Appearance.value;
const nodes = (node) =>
  node && typeof node === 'object'
    ? [
        node,
        ...Object.values(node).flatMap((value) =>
          Array.isArray(value) ? value.flatMap(nodes) : nodes(value),
        ),
      ]
    : [];
for (const target of ['tui']) {
  test(`${target} ordinary history accepts failures and links their diagnostic source`, async () => {
    const { component, activate } = await import(`../dist/packages/timeline/${target}.js`);
    const props = {
      theme,
      locale: 'en',
      copyRequest: null,
      browse: null,
      timings: [],
      markReadEnabled: false,
      toolContext: { sessionId: 'session', through: 8 },
      waitingTools: [],
      liveTools: [],
      source: null,
      search: null,
      loading: false,
      older: false,
      newer: false,
      latest: false,
      landing: { sequence: 0, edge: 'end' },
      live: false,
      messages: [
        {
          id: 'failed',
          contentKey: 'failed',
          role: 'failure',
          text: 'Provider stream closed before completion',
          timestamp: 10000,
          source: 8,
          image: false,
          continued: false,
          tool: null,
        },
      ],
    };
    const schema = activate().Timeline.contract.interface.props;
    const valid = new Ajv({ strict: false }).compile({
      ...schema.properties.messages,
      $defs: schema.$defs,
    });
    assert.equal(valid(props.messages), true, JSON.stringify(valid.errors));
    const out = component.mount(props, {});
    const tree = nodes(out.frame.view);
    assert.ok(tree.some((node) => node.source === props.messages[0].text));
    assert.deepEqual(tree.find((node) => node.key === 'marker-failed').color, theme.error >>> 8);
    const source = component.event(
      out.frame.state,
      { kind: 'raw', key: 'source-failed', event: { kind: 'activate' } },
      props,
      {},
    );
    assert.ok(
      source.events.some(
        (event) =>
          event.name === 'source' && event.value.sequence === 8 && event.value.field === 'body',
      ),
    );
  });
}
