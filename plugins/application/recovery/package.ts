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
  contract: own.recoveryContract,
  export: 'Recovery',
  imports: {
    deployment: one(own.deploymentContract),
    history: one(sdk.contracts.callerHistory),
    receipts: one(sdk.contracts.receiptReader),
    managementReceipts: one(sdk.contracts.receiptReader),
    skillOutcomes: one(sdk.contracts.skillOutcomes),
    reading: one(sdk.contracts.readingArea),
    appearance: one(own.themeContract),
    locale: one(sdk.contracts.locale),
  },
  configuration: own.object({ locale: own.locale }),
});
entries.tui.imports.surface = one(sdk.contracts.terminalSurface);
export default {
  schemaVersion: 1,
  id: 'maka.agent.recovery',
  displayName: 'App recovery',
  description: 'Check unfinished changes and repair plugin configuration.',
  localizations: {
    'zh-CN': { displayName: '应用修复', description: '查询未完成的更改，修复插件配置。' },
  },
  application: { entries },
};
