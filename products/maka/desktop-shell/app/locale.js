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

// The language the desktop speaks: English, Simplified Chinese or
// Traditional Chinese, the three the TUI and the native client have. Settings
// applies the connected Host's resolved language. The local system language
// is only used while that first read is pending. Words come from messages.js.

import { system } from 'maka-locale';
import { MESSAGES } from './messages.js';

/** The languages, in the order of a message's entries. */
export const LANGUAGES = ['en', 'zh-CN', 'zh-TW'];
/** What each language is called, in itself, as a language menu lists it. */
export const LANGUAGE_NAMES = { en: 'English', 'zh-CN': '简体中文', 'zh-TW': '繁體中文' };
/** The Host's saved choices: its system language, or one of LANGUAGES. */
export const LANGUAGE_CHOICES = ['auto', ...LANGUAGES];

/** Which of LANGUAGES a BCP 47 tag means. */
export function languageOf(tag) {
  const lower = String(tag).toLowerCase();
  if (!lower.startsWith('zh')) return 'en';
  return lower.includes('hant') || /^zh-(tw|hk|mo)/.test(lower) ? 'zh-TW' : 'zh-CN';
}

let current = languageOf(system());
const listeners = new Set();

/** The language in use. */
export function language() {
  return current;
}

/**
 * Projects a confirmed Host language into labels and retained placeholders.
 * Preference persistence and automatic-language resolution belong to the Host.
 */
export function applyLanguage(next) {
  if (next === current) return;
  current = next;
  for (const listener of listeners) listener(next);
}

/** Calls `listener(language)` after each switch; answers a function that stops it. */
export function onLanguageChange(listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * The message `key` in the current language, with each `{name}` replaced
 * by `params[name]`. An unknown key is a bug: it throws.
 */
export function t(key, params) {
  const message = MESSAGES[key];
  if (message === undefined) throw new Error(`no message ${key}`);
  const text = message[LANGUAGES.indexOf(current)];
  return params === undefined ? text : fill(text, params);
}

/** `one` for a count of 1 and `other` for the rest, with `{count}` filled in. */
export function tn(count, one, other, params = {}) {
  return t(count === 1 ? one : other, { ...params, count });
}

function fill(text, params) {
  return text.replace(/\{(\w+)\}/g, (whole, name) =>
    Object.hasOwn(params, name) ? String(params[name]) : whole,
  );
}
