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
import { one, uiEntries } from '../../shared/package.js';

const entries = uiEntries({
  contract: own.timelineContract,
  export: 'Timeline',
  imports: {},
});
entries.tui.imports.reading = one(sdk.contracts.readingArea);
Object.assign(entries.tui.imports, {
  formatter: one(sdk.contracts.markdownText),
  input: one(sdk.contracts.textInput),
  reader: one(sdk.contracts.textReader),
  agent: one(sdk.contracts.agent),
});
export default {
  schemaVersion: 1,
  id: 'maka.agent.timeline',
  displayName: 'Conversation messages',
  description: 'Read messages and follow the assistant’s progress.',
  localizations: {
    'zh-CN': { displayName: '对话消息', description: '阅读消息，查看助手的执行进度。' },
  },
  application: { entries },
};
