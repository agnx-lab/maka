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
  msg_advancedSettings,
  msg_back,
  msg_basicSettings,
  msg_cancel,
  msg_check,
  msg_completed,
  msg_confirmRemove,
  msg_confirmRemoveKey,
  msg_connectionChanged,
  msg_connectionModelsFailed,
  msg_connectionProvider,
  msg_connectionReady,
  msg_copyLoginCode,
  msg_copyLoginLink,
  msg_credentials,
  msg_discard,
  msg_discardKey,
  msg_draftKept,
  msg_earlierHelp,
  msg_earlierOperations,
  msg_failed,
  msg_historyRejected,
  msg_historyRemoved,
  msg_historySaved,
  msg_historyUnavailable,
  msg_historyUnknown,
  msg_keepEditing,
  msg_keyAbsent,
  msg_keyConfigured,
  msg_keyConflict,
  msg_keyHelp,
  msg_keyLoading,
  msg_keyPlaceholder,
  msg_keyRefresh,
  msg_keyRejected,
  msg_keyRemoved,
  msg_keySaved,
  msg_keyUnavailable,
  msg_keyUnknown,
  msg_leave,
  msg_leaveUnknown,
  msg_loadConnectionModels,
  msg_loading,
  msg_loadingConnectionModels,
  msg_loginCancelled,
  msg_loginCancelling,
  msg_loginExpired,
  msg_loginFailed,
  msg_loginInstructions,
  msg_loginSaved,
  msg_loginStarting,
  msg_loginTransportFailed,
  msg_loginWaiting,
  msg_providersUnavailable,
  msg_refreshConnections,
  msg_removeKey,
  msg_reviewConnection,
  msg_reviewReady,
  msg_saveKey,
  msg_settings,
  msg_setupDone,
  msg_signInConnection,
  msg_useConnection,
} from '../../shared/locale-messages.js';
import { translate } from '../../shared/tui-locale.js';
import { tui_anonymous_credential, tui_help } from '../../shared/tui-messages.js';

// This feature owns this explicit message catalog; shared messages remain tree-shakeable.

const messages = {
  advancedSettings: msg_advancedSettings,
  back: msg_back,
  basicSettings: msg_basicSettings,
  cancel: msg_cancel,
  check: msg_check,
  completed: msg_completed,
  confirmRemove: msg_confirmRemove,
  confirmRemoveKey: msg_confirmRemoveKey,
  connectionChanged: msg_connectionChanged,
  connectionModelsFailed: msg_connectionModelsFailed,
  connectionProvider: msg_connectionProvider,
  connectionReady: msg_connectionReady,
  copyLoginCode: msg_copyLoginCode,
  copyLoginLink: msg_copyLoginLink,
  credentials: msg_credentials,
  discard: msg_discard,
  discardKey: msg_discardKey,
  draftKept: msg_draftKept,
  earlierHelp: msg_earlierHelp,
  earlierOperations: msg_earlierOperations,
  failed: msg_failed,
  historyRejected: msg_historyRejected,
  historyRemoved: msg_historyRemoved,
  historySaved: msg_historySaved,
  historyUnavailable: msg_historyUnavailable,
  historyUnknown: msg_historyUnknown,
  keepEditing: msg_keepEditing,
  keyAbsent: msg_keyAbsent,
  keyConfigured: msg_keyConfigured,
  keyConflict: msg_keyConflict,
  keyHelp: msg_keyHelp,
  keyLoading: msg_keyLoading,
  keyPlaceholder: msg_keyPlaceholder,
  keyRefresh: msg_keyRefresh,
  keyRejected: msg_keyRejected,
  keyRemoved: msg_keyRemoved,
  keySaved: msg_keySaved,
  keyUnavailable: msg_keyUnavailable,
  keyUnknown: msg_keyUnknown,
  leave: msg_leave,
  leaveUnknown: msg_leaveUnknown,
  loadConnectionModels: msg_loadConnectionModels,
  loading: msg_loading,
  loadingConnectionModels: msg_loadingConnectionModels,
  loginCancelled: msg_loginCancelled,
  loginCancelling: msg_loginCancelling,
  loginExpired: msg_loginExpired,
  loginFailed: msg_loginFailed,
  loginInstructions: msg_loginInstructions,
  loginSaved: msg_loginSaved,
  loginStarting: msg_loginStarting,
  loginTransportFailed: msg_loginTransportFailed,
  loginWaiting: msg_loginWaiting,
  providersUnavailable: msg_providersUnavailable,
  refreshConnections: msg_refreshConnections,
  removeKey: msg_removeKey,
  reviewConnection: msg_reviewConnection,
  reviewReady: msg_reviewReady,
  saveKey: msg_saveKey,
  settings: msg_settings,
  setupDone: msg_setupDone,
  signInConnection: msg_signInConnection,
  useConnection: msg_useConnection,
};
export const copy = /* @__PURE__ */ createCopy(messages);
export type Copy = ReturnType<typeof copy>;

const terminalMessages = {
  'anonymous-credential': tui_anonymous_credential,
  help: tui_help,
};
export function tuiText(
  locale: string,
  key: string,
  values: Record<string, string | number> = {},
): string {
  return translate(locale, terminalMessages, key, values);
}
