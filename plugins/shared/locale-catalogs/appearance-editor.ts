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
import { msg_failed, msg_loading, msg_unavailable } from '../locale-messages.js';
import { translate } from '../tui-locale.js';
import {
  tui_home_retry,
  tui_language_auto,
  tui_session_cancel,
  tui_settings_appearance,
  tui_settings_language,
  tui_settings_motion,
  tui_settings_motion_off,
  tui_settings_motion_on,
  tui_settings_palette,
  tui_settings_palette_dark,
  tui_settings_palette_default,
  tui_settings_palette_dusk,
  tui_settings_palette_paper,
  tui_settings_symbols,
  tui_settings_symbols_ascii,
  tui_settings_symbols_unicode,
  tui_state_save_failed,
  tui_theme_custom_name,
  tui_theme_customize,
  tui_theme_export,
  tui_theme_exported,
  tui_theme_file_changed,
  tui_theme_file_draft_hint,
  tui_theme_file_missing,
  tui_theme_file_name,
  tui_theme_file_save,
  tui_theme_file_unreadable,
  tui_theme_file_version,
  tui_theme_hex_invalid,
  tui_theme_import_invalid,
  tui_theme_imported,
  tui_theme_name,
  tui_theme_path,
  tui_theme_preview,
  tui_theme_reload_file,
  tui_theme_reload_stale,
  tui_theme_save,
  tui_theme_role_background,
  tui_theme_role_foreground,
  tui_theme_role_surface,
  tui_theme_role_accent,
  tui_theme_role_thinking,
  tui_theme_role_success,
  tui_theme_role_warning,
  tui_theme_role_error,
  tui_theme_role_muted,
  tui_theme_role_subtle,
  tui_theme_role_border,
  tui_theme_role_scrollbar,
  tui_theme_role_selection,
  tui_theme_role_selection_text,
  tui_theme_role_search,
  tui_theme_role_search_active,
  tui_theme_role_search_text,
  tui_theme_role_keyword,
  tui_theme_role_string,
  tui_theme_role_comment,
  tui_theme_role_number,
  tui_theme_role_function,
  tui_theme_role_type,
  tui_theme_role_operator,
} from '../tui-messages.js';

// This feature owns this explicit message catalog; shared messages remain tree-shakeable.

const messages = {
  failed: msg_failed,
  loading: msg_loading,
  unavailable: msg_unavailable,
};
export const copy = /* @__PURE__ */ createCopy(messages);
export type Copy = ReturnType<typeof copy>;

const terminalMessages = {
  'theme-role-operator': tui_theme_role_operator,
  'theme-role-type': tui_theme_role_type,
  'theme-role-function': tui_theme_role_function,
  'theme-role-number': tui_theme_role_number,
  'theme-role-comment': tui_theme_role_comment,
  'theme-role-string': tui_theme_role_string,
  'theme-role-keyword': tui_theme_role_keyword,
  'theme-role-search-text': tui_theme_role_search_text,
  'theme-role-search-active': tui_theme_role_search_active,
  'theme-role-search': tui_theme_role_search,
  'theme-role-selection-text': tui_theme_role_selection_text,
  'theme-role-selection': tui_theme_role_selection,
  'theme-role-scrollbar': tui_theme_role_scrollbar,
  'theme-role-border': tui_theme_role_border,
  'theme-role-subtle': tui_theme_role_subtle,
  'theme-role-muted': tui_theme_role_muted,
  'theme-role-error': tui_theme_role_error,
  'theme-role-warning': tui_theme_role_warning,
  'theme-role-success': tui_theme_role_success,
  'theme-role-thinking': tui_theme_role_thinking,
  'theme-role-accent': tui_theme_role_accent,
  'theme-role-surface': tui_theme_role_surface,
  'theme-role-foreground': tui_theme_role_foreground,
  'theme-role-background': tui_theme_role_background,
  'home-retry': tui_home_retry,
  'language-auto': tui_language_auto,
  'session-cancel': tui_session_cancel,
  'settings-appearance': tui_settings_appearance,
  'settings-language': tui_settings_language,
  'settings-motion': tui_settings_motion,
  'settings-motion-off': tui_settings_motion_off,
  'settings-motion-on': tui_settings_motion_on,
  'settings-palette': tui_settings_palette,
  'settings-palette-dark': tui_settings_palette_dark,
  'settings-palette-default': tui_settings_palette_default,
  'settings-palette-dusk': tui_settings_palette_dusk,
  'settings-palette-paper': tui_settings_palette_paper,
  'settings-symbols': tui_settings_symbols,
  'settings-symbols-ascii': tui_settings_symbols_ascii,
  'settings-symbols-unicode': tui_settings_symbols_unicode,
  'state-save-failed': tui_state_save_failed,
  'theme-custom-name': tui_theme_custom_name,
  'theme-customize': tui_theme_customize,
  'theme-export': tui_theme_export,
  'theme-exported': tui_theme_exported,
  'theme-file-changed': tui_theme_file_changed,
  'theme-file-draft-hint': tui_theme_file_draft_hint,
  'theme-file-missing': tui_theme_file_missing,
  'theme-file-name': tui_theme_file_name,
  'theme-file-save': tui_theme_file_save,
  'theme-file-unreadable': tui_theme_file_unreadable,
  'theme-file-version': tui_theme_file_version,
  'theme-hex-invalid': tui_theme_hex_invalid,
  'theme-import-invalid': tui_theme_import_invalid,
  'theme-imported': tui_theme_imported,
  'theme-name': tui_theme_name,
  'theme-path': tui_theme_path,
  'theme-preview': tui_theme_preview,
  'theme-reload-file': tui_theme_reload_file,
  'theme-reload-stale': tui_theme_reload_stale,
  'theme-save': tui_theme_save,
};
export function tuiText(
  locale: string,
  key: string,
  values: Record<string, string | number> = {},
): string {
  return translate(locale, terminalMessages, key, values);
}
