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
import { msg_assistant, msg_failed, msg_models, msg_settings } from '../locale-messages.js';
import { translate } from '../tui-locale.js';
import {
  tui_home_retry,
  tui_route_plugins,
  tui_settings_host,
  tui_settings_jev,
  tui_settings_language,
  tui_settings_models,
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
  tui_theme_customize,
  tui_settings_subagents,
  tui_settings_web,
  tui_settings_insights,
  tui_settings_host_details,
  tui_settings_advanced,
  tui_settings_appearance,
  tui_settings_assistant,
  tui_settings_interface,
  tui_settings_sessions,
  tui_settings_palette_custom_fallback,
  tui_settings_choose,
  tui_settings_host_connected,
  tui_settings_host_connecting,
  tui_settings_host_disconnected,
  tui_settings_host_failed,
} from '../tui-messages.js';

// This feature owns this explicit message catalog; shared messages remain tree-shakeable.

const messages = {
  assistant: msg_assistant,
  failed: msg_failed,
  models: msg_models,
  settings: msg_settings,
};
export const copy = /* @__PURE__ */ createCopy(messages);
export type Copy = ReturnType<typeof copy>;

const terminalMessages = {
  'settings-language-loading': ['Loading…', '正在读取…', '正在讀取…'] as const,
  'settings-host-failed': tui_settings_host_failed,
  'settings-host-disconnected': tui_settings_host_disconnected,
  'settings-host-connecting': tui_settings_host_connecting,
  'settings-host-connected': tui_settings_host_connected,
  'settings-choose': tui_settings_choose,
  'settings-palette-custom-fallback': tui_settings_palette_custom_fallback,
  'settings-sessions': tui_settings_sessions,
  'settings-interface': tui_settings_interface,
  'settings-assistant': tui_settings_assistant,
  'settings-appearance': tui_settings_appearance,
  'settings-advanced': tui_settings_advanced,
  'settings-host-details': tui_settings_host_details,
  'settings-insights': tui_settings_insights,
  'settings-web': tui_settings_web,
  'settings-subagents': tui_settings_subagents,
  'home-retry': tui_home_retry,
  'settings-language-auto': [
    'Auto (system language)',
    '自动（系统语言）',
    '自動（系統語言）',
  ] as const,
  'route-plugins': tui_route_plugins,
  'settings-host': tui_settings_host,
  'settings-jev': tui_settings_jev,
  'settings-language': tui_settings_language,
  'settings-models': tui_settings_models,
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
  'theme-customize': tui_theme_customize,
};
export function tuiText(
  locale: string,
  key: string,
  values: Record<string, string | number> = {},
): string {
  return translate(locale, terminalMessages, key, values);
}
