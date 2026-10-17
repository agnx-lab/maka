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
  contract: own.workhubRootContract,
  export: 'WorkHub',
  imports: {
    workhub: one(sdk.contracts.workhubCoordinator),
    coordinator: one(own.agentScreenContract),
    authoring: one(own.uiAuthoringContract),
    worker: one(own.agentScreenContract),
    directories: one(own.directoryPickerContract),
    reading: one(sdk.contracts.readingArea),
    source: one(sdk.contracts.sourceText),
    receipts: one(sdk.contracts.receiptReader),
    history: one(sdk.contracts.callerHistory),
    appearance: one(own.themeContract),
    locale: one(sdk.contracts.locale),
  },
  configuration: own.object(
    {
      locale: own.locale,
      worker: { type: 'string', const: 'worker' },
      initialRoute: { type: 'string', enum: ['coordinator', 'sessions'] },
    },
    ['locale', 'worker'],
  ),
});
entries.tui.imports.surface = one(sdk.contracts.terminalSurface);
export default {
  schemaVersion: 1,
  id: 'maka.workhub.root',
  displayName: 'WorkHub',
  description: 'Organize work across agents and projects.',
  localizations: {
    'zh-CN': { displayName: 'WorkHub', description: '组织多个助手和项目中的工作。' },
  },
  application: { entries },
};
