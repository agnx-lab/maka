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

import { createCopy } from '../../shared/locale.js';
import {
  msg_back,
  msg_cancel,
  msg_executionReceipt,
  msg_models,
  msg_new,
  msg_refreshDefaultOperations,
  msg_settingOff,
  msg_settingOn,
} from '../../shared/locale-messages.js';
import { translate } from '../../shared/tui-locale.js';
import {
  tui_settings_subagents,
  tui_subagents_all,
  tui_subagents_choose_model,
  tui_subagents_conflict,
  tui_subagents_connections,
  tui_subagents_enabled,
  tui_subagents_help,
  tui_subagents_id,
  tui_subagents_invalid,
  tui_subagents_isolated,
  tui_subagents_isolated_help,
  tui_subagents_loading,
  tui_subagents_missing_tool,
  tui_subagents_model,
  tui_subagents_new,
  tui_subagents_none,
  tui_subagents_read,
  tui_subagents_reload,
  tui_subagents_remove,
  tui_subagents_remove_question,
  tui_subagents_save,
  tui_subagents_saved,
  tui_subagents_selected,
  tui_subagents_tools_help,
  tui_subagents_unavailable,
  tui_subagents_unknown,
  tui_subagents_web,
  tui_subagents_name,
  tui_subagents_description,
} from '../../shared/tui-messages.js';

// This feature owns this explicit message catalog; shared messages remain tree-shakeable.

const messages = {
  back: msg_back,
  cancel: msg_cancel,
  executionReceipt: msg_executionReceipt,
  models: msg_models,
  new: msg_new,
  refreshDefaultOperations: msg_refreshDefaultOperations,
  settingOff: msg_settingOff,
  settingOn: msg_settingOn,
};
export const copy = /* @__PURE__ */ createCopy(messages);
export type Copy = ReturnType<typeof copy>;

const terminalMessages = {
  'subagents-enabled-help': [
    'Allow WorkHub to delegate tasks to this subagent.',
    '允许 WorkHub 将任务交给这个 Subagent。',
    '允許 WorkHub 將任務交給這個 Subagent。',
  ] as const,
  'subagents-description': tui_subagents_description,
  'subagents-name': tui_subagents_name,
  'settings-subagents': tui_settings_subagents,
  'subagents-all': tui_subagents_all,
  'subagents-choose-model': tui_subagents_choose_model,
  'subagents-conflict': tui_subagents_conflict,
  'subagents-connections': tui_subagents_connections,
  'subagents-enabled': tui_subagents_enabled,
  'subagents-help': tui_subagents_help,
  'subagents-id': tui_subagents_id,
  'subagents-invalid': tui_subagents_invalid,
  'subagents-isolated': tui_subagents_isolated,
  'subagents-isolated-help': tui_subagents_isolated_help,
  'subagents-loading': tui_subagents_loading,
  'subagents-missing-tool': tui_subagents_missing_tool,
  'subagents-model': tui_subagents_model,
  'subagents-new': tui_subagents_new,
  'subagents-none': tui_subagents_none,
  'subagents-read': tui_subagents_read,
  'subagents-reload': tui_subagents_reload,
  'subagents-remove': tui_subagents_remove,
  'subagents-remove-question': tui_subagents_remove_question,
  'subagents-save': tui_subagents_save,
  'subagents-saved': tui_subagents_saved,
  'subagents-selected': tui_subagents_selected,
  'subagents-tools-help': tui_subagents_tools_help,
  'subagents-unavailable': tui_subagents_unavailable,
  'subagents-unknown': tui_subagents_unknown,
  'subagents-web': tui_subagents_web,
};
export function tuiText(
  locale: string,
  key: string,
  values: Record<string, string | number> = {},
): string {
  return translate(locale, terminalMessages, key, values);
}
