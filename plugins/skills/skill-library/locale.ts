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
  msg_closeModels,
  msg_refreshSessions,
  msg_skillsAlreadyInstalled,
  msg_skillsBundled,
  msg_skillsChangeRejected,
  msg_skillsChanged,
  msg_skillsCheckResult,
  msg_skillsCleanupUnknown,
  msg_skillsConfirm,
  msg_skillsConfirmTitle,
  msg_skillsDelete,
  msg_skillsDeleteDetail,
  msg_skillsDeleteSource,
  msg_skillsDeleteSourceHelp,
  msg_skillsDisable,
  msg_skillsDisabled,
  msg_skillsEmpty,
  msg_skillsEnable,
  msg_skillsEnabled,
  msg_skillsEntryCount,
  msg_skillsFailed,
  msg_skillsImport,
  msg_skillsImportInvalid,
  msg_skillsImported,
  msg_skillsInstall,
  msg_skillsInstallDetail,
  msg_skillsInstalled,
  msg_skillsInvalid,
  msg_skillsLoading,
  msg_skillsLocalModified,
  msg_skillsNotInstalled,
  msg_skillsPathsTruncated,
  msg_skillsPin,
  msg_skillsPinned,
  msg_skillsSearch,
  msg_skillsShadowed,
  msg_skillsSourceExists,
  msg_skillsSourceIdInvalid,
  msg_skillsSubmit,
  msg_skillsTruncated,
  msg_skillsUnknown,
  msg_skillsUnpin,
  msg_skillsUpdate,
  msg_skillsUpdateDetail,
  msg_toolByteRange,
} from '../../shared/locale-messages.js';
import { translate } from '../../shared/tui-locale.js';
import {
  tui_skills_changed,
  tui_skills_empty,
  tui_skills_failed,
  tui_skills_idle,
  tui_skills_loading,
  tui_skills_selected,
  tui_skills_title,
} from '../../shared/tui-messages.js';

// This feature owns this explicit message catalog; shared messages remain tree-shakeable.

const messages = {
  back: msg_back,
  cancel: msg_cancel,
  closeModels: msg_closeModels,
  refreshSessions: msg_refreshSessions,
  skillsAlreadyInstalled: msg_skillsAlreadyInstalled,
  skillsBundled: msg_skillsBundled,
  skillsChangeRejected: msg_skillsChangeRejected,
  skillsChanged: msg_skillsChanged,
  skillsCheckResult: msg_skillsCheckResult,
  skillsCleanupUnknown: msg_skillsCleanupUnknown,
  skillsConfirm: msg_skillsConfirm,
  skillsConfirmTitle: msg_skillsConfirmTitle,
  skillsDelete: msg_skillsDelete,
  skillsDeleteDetail: msg_skillsDeleteDetail,
  skillsDeleteSource: msg_skillsDeleteSource,
  skillsDeleteSourceHelp: msg_skillsDeleteSourceHelp,
  skillsDisable: msg_skillsDisable,
  skillsDisabled: msg_skillsDisabled,
  skillsEmpty: msg_skillsEmpty,
  skillsEnable: msg_skillsEnable,
  skillsEnabled: msg_skillsEnabled,
  skillsEntryCount: msg_skillsEntryCount,
  skillsFailed: msg_skillsFailed,
  skillsImport: msg_skillsImport,
  skillsImportInvalid: msg_skillsImportInvalid,
  skillsImported: msg_skillsImported,
  skillsInstall: msg_skillsInstall,
  skillsInstallDetail: msg_skillsInstallDetail,
  skillsInstalled: msg_skillsInstalled,
  skillsInvalid: msg_skillsInvalid,
  skillsLoading: msg_skillsLoading,
  skillsLocalModified: msg_skillsLocalModified,
  skillsNotInstalled: msg_skillsNotInstalled,
  skillsPathsTruncated: msg_skillsPathsTruncated,
  skillsPin: msg_skillsPin,
  skillsPinned: msg_skillsPinned,
  skillsSearch: msg_skillsSearch,
  skillsShadowed: msg_skillsShadowed,
  skillsSourceExists: msg_skillsSourceExists,
  skillsSourceIdInvalid: msg_skillsSourceIdInvalid,
  skillsSubmit: msg_skillsSubmit,
  skillsTruncated: msg_skillsTruncated,
  skillsUnknown: msg_skillsUnknown,
  skillsUnpin: msg_skillsUnpin,
  skillsUpdate: msg_skillsUpdate,
  skillsUpdateDetail: msg_skillsUpdateDetail,
  toolByteRange: msg_toolByteRange,
};
export const copy = /* @__PURE__ */ createCopy(messages);
export type Copy = ReturnType<typeof copy>;

const terminalMessages = {
  'skills-changed': tui_skills_changed,
  'skills-empty': tui_skills_empty,
  'skills-failed': tui_skills_failed,
  'skills-idle': tui_skills_idle,
  'skills-loading': tui_skills_loading,
  'skills-selected': tui_skills_selected,
  'skills-title': tui_skills_title,
};
export function tuiText(
  locale: string,
  key: string,
  values: Record<string, string | number> = {},
): string {
  return translate(locale, terminalMessages, key, values);
}
