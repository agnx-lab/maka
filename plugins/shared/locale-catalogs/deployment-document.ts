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
  msg_back,
  msg_cancel,
  msg_check,
  msg_confirmDefault,
  msg_copy,
  msg_cut,
  msg_deploymentConflict,
  msg_deploymentDocument,
  msg_deploymentEdit,
  msg_deploymentEditHelp,
  msg_deploymentHistory,
  msg_deploymentLoadFailed,
  msg_deploymentLoading,
  msg_deploymentReload,
  msg_deploymentReloadHelp,
  msg_deploymentReloadQuestion,
  msg_deploymentSave,
  msg_deploymentSaveHelp,
  msg_deploymentSaveQuestion,
  msg_edit,
  msg_failed,
  msg_loading,
  msg_paste,
  msg_redo,
  msg_selectAll,
  msg_undo,
} from '../locale-messages.js';

// This feature owns this explicit message catalog; shared messages remain tree-shakeable.

const messages = {
  back: msg_back,
  cancel: msg_cancel,
  check: msg_check,
  confirmDefault: msg_confirmDefault,
  copy: msg_copy,
  cut: msg_cut,
  deploymentConflict: msg_deploymentConflict,
  deploymentDocument: msg_deploymentDocument,
  deploymentEdit: msg_deploymentEdit,
  deploymentEditHelp: msg_deploymentEditHelp,
  deploymentHistory: msg_deploymentHistory,
  deploymentLoadFailed: msg_deploymentLoadFailed,
  deploymentLoading: msg_deploymentLoading,
  deploymentReload: msg_deploymentReload,
  deploymentReloadHelp: msg_deploymentReloadHelp,
  deploymentReloadQuestion: msg_deploymentReloadQuestion,
  deploymentSave: msg_deploymentSave,
  deploymentSaveHelp: msg_deploymentSaveHelp,
  deploymentSaveQuestion: msg_deploymentSaveQuestion,
  edit: msg_edit,
  failed: msg_failed,
  loading: msg_loading,
  paste: msg_paste,
  redo: msg_redo,
  selectAll: msg_selectAll,
  undo: msg_undo,
};
export const copy = /* @__PURE__ */ createCopy(messages);
export type Copy = ReturnType<typeof copy>;
