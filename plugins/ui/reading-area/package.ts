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

import * as sdk from '../../../packages/plugin-sdk/src/index.js';
import * as own from '../../shared/contracts.js';
import { native } from '../../shared/package.js';
export default {
  schemaVersion: 1,
  id: 'maka.agent.reading-area',
  displayName: 'Scrollable content',
  description: 'Scroll content and keep the focused item visible.',
  localizations: {
    'zh-CN': { displayName: '滚动阅读区', description: '滚动内容，并保持当前选中项可见。' },
  },
  application: {
    entries: {
      tui: {
        ...native(sdk.nativeImplementations.readingArea),
        imports: {
          tools: { contract: own.toolDetailsContract, cardinality: { kind: 'optional' } },
          markdown: { contract: sdk.contracts.markdown, cardinality: { kind: 'optional' } },
          source: { contract: sdk.contracts.sourceText, cardinality: { kind: 'optional' } },
          label: { contract: sdk.contracts.label, cardinality: { kind: 'optional' } },
        },
      },
    },
  },
};
