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
  msg_completed,
  msg_connecting,
  msg_connectionDisabled,
  msg_connectionsUnavailable,
  msg_createConnection,
  msg_editConnection,
  msg_empty,
  msg_failed,
  msg_loading,
  msg_noConnections,
  msg_noSearchResults,
  msg_refreshConnections,
  msg_removeConnection,
  msg_searchConnections,
  msg_settings,
  msg_signInConnection,
  msg_searchUnavailable,
} from '../../shared/locale-messages.js';
import { translate } from '../../shared/tui-locale.js';
import {
  tui_connection_actions,
  tui_connection_default,
  tui_connection_models,
} from '../../shared/tui-messages.js';

// This feature owns this explicit message catalog; shared messages remain tree-shakeable.

const messages = {
  searchUnavailable: msg_searchUnavailable,
  back: msg_back,
  completed: msg_completed,
  connecting: msg_connecting,
  connectionDisabled: msg_connectionDisabled,
  connectionsUnavailable: msg_connectionsUnavailable,
  createConnection: msg_createConnection,
  editConnection: msg_editConnection,
  empty: msg_empty,
  failed: msg_failed,
  loading: msg_loading,
  noConnections: msg_noConnections,
  noSearchResults: msg_noSearchResults,
  refreshConnections: msg_refreshConnections,
  removeConnection: msg_removeConnection,
  searchConnections: msg_searchConnections,
  settings: msg_settings,
  signInConnection: msg_signInConnection,
};
export const copy = /* @__PURE__ */ createCopy(messages);
export type Copy = ReturnType<typeof copy>;

const terminalMessages = {
  'connection-actions': tui_connection_actions,
  'connection-default': tui_connection_default,
  'connection-models': tui_connection_models,
};
export function tuiText(
  locale: string,
  key: string,
  values: Record<string, string | number> = {},
): string {
  return translate(locale, terminalMessages, key, values);
}
