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

import { createCopy } from '../locale.js';
import {
  msg_attachmentAdd,
  msg_attachmentChoose,
  msg_attachmentFailed,
  msg_attachmentHelp,
  msg_attachmentPasting,
  msg_attachmentPath,
  msg_attachmentPathPlaceholder,
  msg_attachmentRemove,
  msg_attachmentSkip,
  msg_attachmentTooLarge,
  msg_back,
  msg_cancel,
  msg_check,
  msg_clipboardPasteRejected,
  msg_clipboardUnavailable,
  msg_closeModels,
  msg_deploymentFileLoading,
  msg_directoryParent,
  msg_failed,
  msg_fileBrowse,
  msg_fileListEmpty,
  msg_fileListTruncated,
  msg_paste,
  msg_sourceRetry,
} from '../locale-messages.js';
import { translate } from '../tui-locale.js';
import {
  tui_attachments_add,
  tui_attachments_ready,
  tui_attachments_remove,
} from '../tui-messages.js';

// This feature owns this explicit message catalog; shared messages remain tree-shakeable.

const messages = {
  attachmentAdd: msg_attachmentAdd,
  attachmentChoose: msg_attachmentChoose,
  attachmentFailed: msg_attachmentFailed,
  attachmentHelp: msg_attachmentHelp,
  attachmentPasting: msg_attachmentPasting,
  attachmentPath: msg_attachmentPath,
  attachmentPathPlaceholder: msg_attachmentPathPlaceholder,
  attachmentRemove: msg_attachmentRemove,
  attachmentSkip: msg_attachmentSkip,
  attachmentTooLarge: msg_attachmentTooLarge,
  back: msg_back,
  cancel: msg_cancel,
  check: msg_check,
  clipboardPasteRejected: msg_clipboardPasteRejected,
  clipboardUnavailable: msg_clipboardUnavailable,
  closeModels: msg_closeModels,
  deploymentFileLoading: msg_deploymentFileLoading,
  directoryParent: msg_directoryParent,
  failed: msg_failed,
  fileBrowse: msg_fileBrowse,
  fileListEmpty: msg_fileListEmpty,
  fileListTruncated: msg_fileListTruncated,
  paste: msg_paste,
  sourceRetry: msg_sourceRetry,
};
export const copy = /* @__PURE__ */ createCopy(messages);
export type Copy = ReturnType<typeof copy>;

const terminalMessages = {
  'attachments-add': tui_attachments_add,
  'attachments-ready': tui_attachments_ready,
  'attachments-remove': tui_attachments_remove,
};
export function tuiText(
  locale: string,
  key: string,
  values: Record<string, string | number> = {},
): string {
  return translate(locale, terminalMessages, key, values);
}
