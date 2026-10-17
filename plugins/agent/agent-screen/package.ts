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
  contract: own.agentScreenContract,
  export: 'AgentScreen',
  imports: {
    agent: one(sdk.contracts.agent),
    hostLogs: { contract: sdk.contracts.hostLogs, cardinality: { kind: 'optional' } },
    workhub: { contract: sdk.contracts.workhubCoordinator, cardinality: { kind: 'optional' } },
    reading: one(sdk.contracts.readingArea),
    file: one(sdk.contracts.filePicker),
    models: one(own.executionPickerContract),
    connections: one(own.connectionsContract),
    deployment: one(own.deploymentContract),
    receipts: one(sdk.contracts.receiptReader),
    history: one(sdk.contracts.callerHistory),
    reader: one(sdk.contracts.textReader),
    input: one(sdk.contracts.textInput),
    editor: one(sdk.contracts.textarea),
    composer: one(own.composerContract),
    footer: one(own.composerFooterContract),
    authoring: one(own.uiAuthoringContract),
    palette: one(own.paletteContract),
    directories: one(own.directoryPickerContract),
    projects: one(own.projectsContract),
    imports: one(own.importsContract),
    assistantPreferences: one(own.assistantPreferencesContract),
    networkSettings: one(own.networkSettingsContract),
    sessionDefaults: one(own.sessionDefaultsContract),
    webSettings: { contract: own.webSettingsContract, cardinality: { kind: 'optional' } },
    jevSettings: { contract: own.jevSettingsContract, cardinality: { kind: 'optional' } },
    subagentSettings: {
      contract: own.subagentSettingsContract,
      cardinality: { kind: 'optional' },
    },
    insights: one(own.insightsContract),
    recall: one(own.recallContract),
    scheduler: one(own.schedulerContract),
    apps: one(own.appsContract),
    details: one(own.sessionDetailsContract),
    terminals: one(own.terminalPanelContract),
    skills: {
      contract: own.skillLibraryContract,
      cardinality: { kind: 'optional' },
    },
    queueEditor: one(own.queueEditorContract),
    formEditor: one(own.formEditorContract),
    questionEditor: one(own.questionEditorContract),
    serviceApproval: one(own.serviceApprovalContract),
    permissionApproval: one(own.permissionApprovalContract),
    help: one(own.helpContract),
    navigation: one(own.navigationContract),
    timeline: one(own.timelineContract),
    imageViewer: one(own.imageViewerContract),
    appearance: one(own.themeContract),
    locale: one(sdk.contracts.locale),
  },
  configuration: own.object(
    {
      locale: own.locale,
      terminals: { type: 'boolean' },
      skills: { type: 'boolean' },
      scheduledTasks: { type: 'boolean' },
      web: { type: 'boolean' },
      jev: { type: 'boolean' },
      graphs: { type: 'boolean' },
      hostLogs: { type: 'boolean' },
      place: { type: 'string', maxLength: 4096 },
    },
    ['locale'],
  ),
});
Object.assign(entries.tui.imports, {
  rows: one(sdk.contracts.rows),
  checkpoint: one(sdk.contracts.valueCheckpoint),
  themeFile: one(sdk.contracts.textFile),
});
entries.tui.imports.surface = one(sdk.contracts.terminalSurface);
export default {
  schemaVersion: 1,
  id: 'maka.agent.agent-screen',
  displayName: 'Assistant',
  description: 'Chat with an agent and manage conversations, tasks, and settings.',
  localizations: {
    'zh-CN': { displayName: '助手', description: '与助手对话，管理对话、任务和设置。' },
  },
  application: { entries },
};
