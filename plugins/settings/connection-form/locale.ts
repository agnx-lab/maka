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
  msg_changeProvider,
  msg_check,
  msg_chooseProvider,
  msg_configurationHelp,
  msg_confirmRemoveConnection,
  msg_connecting,
  msg_connectionChanged,
  msg_connectionConfiguration,
  msg_connectionConflict,
  msg_connectionDisabled,
  msg_connectionDraftKept,
  msg_connectionEnabled,
  msg_connectionEndpoint,
  msg_connectionLeaveUnknown,
  msg_connectionModels,
  msg_connectionName,
  msg_connectionOverlay,
  msg_connectionOverrides,
  msg_connectionProvider,
  msg_connectionRebaseHelp,
  msg_connectionRejected,
  msg_connectionRemoved,
  msg_connectionSaved,
  msg_connectionSetupHelp,
  msg_connectionSlug,
  msg_connectionUnknown,
  msg_continueSignIn,
  msg_createConnection,
  msg_defaultFieldKept,
  msg_defaultPrefillFailed,
  msg_discard,
  msg_discardConnection,
  msg_earlierConnectionHelp,
  msg_earlierConnectionOperations,
  msg_editConnection,
  msg_fieldPlaceholder,
  msg_fieldPrefillFailed,
  msg_historyRejected,
  msg_historyUnavailable,
  msg_historyUnknown,
  msg_invalidConnectionFields,
  msg_keepEditing,
  msg_keepSavedField,
  msg_leave,
  msg_models,
  msg_newCollectionHelp,
  msg_noConnectionChanges,
  msg_noProviders,
  msg_noSearchResults,
  msg_providersUnavailable,
  msg_refreshConnections,
  msg_removeConnection,
  msg_replaceCollectionHelp,
  msg_replaceField,
  msg_replaceFromEmpty,
  msg_reviewConnection,
  msg_saveConnection,
  msg_savedFieldKept,
  msg_searchProviders,
  msg_settings,
  msg_useConnection,
  msg_useDefaultField,
  msg_searchUnavailable,
} from '../../shared/locale-messages.js';
import { translate } from '../../shared/tui-locale.js';
import {
  tui_anonymous_setup,
  tui_form_cancel,
  tui_providers_no_matches,
  tui_provider_custom,
} from '../../shared/tui-messages.js';

// This feature owns this explicit message catalog; shared messages remain tree-shakeable.

const messages = {
  searchUnavailable: msg_searchUnavailable,
  advancedSettings: msg_advancedSettings,
  back: msg_back,
  basicSettings: msg_basicSettings,
  changeProvider: msg_changeProvider,
  check: msg_check,
  chooseProvider: msg_chooseProvider,
  configurationHelp: msg_configurationHelp,
  confirmRemoveConnection: msg_confirmRemoveConnection,
  connecting: msg_connecting,
  connectionChanged: msg_connectionChanged,
  connectionConfiguration: msg_connectionConfiguration,
  connectionConflict: msg_connectionConflict,
  connectionDisabled: msg_connectionDisabled,
  connectionDraftKept: msg_connectionDraftKept,
  connectionEnabled: msg_connectionEnabled,
  connectionEndpoint: msg_connectionEndpoint,
  connectionLeaveUnknown: msg_connectionLeaveUnknown,
  connectionModels: msg_connectionModels,
  connectionName: msg_connectionName,
  connectionOverlay: msg_connectionOverlay,
  connectionOverrides: msg_connectionOverrides,
  connectionProvider: msg_connectionProvider,
  connectionRebaseHelp: msg_connectionRebaseHelp,
  connectionRejected: msg_connectionRejected,
  connectionRemoved: msg_connectionRemoved,
  connectionSaved: msg_connectionSaved,
  connectionSetupHelp: msg_connectionSetupHelp,
  connectionSlug: msg_connectionSlug,
  connectionUnknown: msg_connectionUnknown,
  continueSignIn: msg_continueSignIn,
  createConnection: msg_createConnection,
  defaultFieldKept: msg_defaultFieldKept,
  defaultPrefillFailed: msg_defaultPrefillFailed,
  discard: msg_discard,
  discardConnection: msg_discardConnection,
  earlierConnectionHelp: msg_earlierConnectionHelp,
  earlierConnectionOperations: msg_earlierConnectionOperations,
  editConnection: msg_editConnection,
  fieldPlaceholder: msg_fieldPlaceholder,
  fieldPrefillFailed: msg_fieldPrefillFailed,
  historyRejected: msg_historyRejected,
  historyUnavailable: msg_historyUnavailable,
  historyUnknown: msg_historyUnknown,
  invalidConnectionFields: msg_invalidConnectionFields,
  keepEditing: msg_keepEditing,
  keepSavedField: msg_keepSavedField,
  leave: msg_leave,
  models: msg_models,
  newCollectionHelp: msg_newCollectionHelp,
  noConnectionChanges: msg_noConnectionChanges,
  noProviders: msg_noProviders,
  noSearchResults: msg_noSearchResults,
  providersUnavailable: msg_providersUnavailable,
  refreshConnections: msg_refreshConnections,
  removeConnection: msg_removeConnection,
  replaceCollectionHelp: msg_replaceCollectionHelp,
  replaceField: msg_replaceField,
  replaceFromEmpty: msg_replaceFromEmpty,
  reviewConnection: msg_reviewConnection,
  saveConnection: msg_saveConnection,
  savedFieldKept: msg_savedFieldKept,
  searchProviders: msg_searchProviders,
  settings: msg_settings,
  useConnection: msg_useConnection,
  useDefaultField: msg_useDefaultField,
};
export const copy = /* @__PURE__ */ createCopy(messages);
export type Copy = ReturnType<typeof copy>;

const terminalMessages = {
  'provider-custom': tui_provider_custom,
  'anonymous-setup': tui_anonymous_setup,
  'form-cancel': tui_form_cancel,
  'providers-no-matches': tui_providers_no_matches,
};
export function tuiText(
  locale: string,
  key: string,
  values: Record<string, string | number> = {},
): string {
  return translate(locale, terminalMessages, key, values);
}
