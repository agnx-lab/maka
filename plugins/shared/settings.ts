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

import type { ComponentInput } from '../../packages/plugin-sdk/src/index.js';
import type { Theme, InsightsProps } from './contracts.js';
import { saveAppearanceSetting, type AppearanceEditor } from './appearance-editor.js';
import { tuiText } from './locale-catalogs/settings.js';
import type { ThemeName } from './terminal-theme.js';
import type { Node, Presentation } from './view.js';

export const settingsCategories = [
  'appearance',
  'models',
  'sessions',
  'assistant',
  'subagents',
  'insights',
  'web',
  'advanced',
] as const;
export type SettingsCategory = (typeof settingsCategories)[number];
export function settingsCategory(value: string): SettingsCategory | null {
  const moved: Record<string, SettingsCategory> = {
    interface: 'appearance',
    host: 'advanced',
    plugins: 'advanced',
    jev: 'advanced',
  };
  return (
    moved[value] ??
    (settingsCategories.includes(value as SettingsCategory) ? (value as SettingsCategory) : null)
  );
}
interface SettingsState {
  preferences: AppearanceEditor;
  settingsCategory: SettingsCategory;
  pages: (
    | 'settings'
    | 'connections'
    | 'deployment'
    | 'projects'
    | 'imports'
    | 'apps'
    | 'recall'
    | 'scheduler'
  )[];
}
export function settingsEvent(state: SettingsState, input: ComponentInput, base: Theme): boolean {
  if (input.kind !== 'raw') return false;
  const category = input.key.slice('settings-category-'.length) as SettingsCategory;
  if (input.key.startsWith('settings-category-') && settingsCategories.includes(category)) {
    state.settingsCategory = category;
    return true;
  }
  if (input.key === 'settings-connections' || input.key === 'settings-plugins') {
    state.pages.push(input.key === 'settings-connections' ? 'connections' : 'deployment');
    return true;
  }
  if (input.key === 'settings-retry') {
    state.preferences.command = {
      kind: 'read',
      sequence: ++state.preferences.sequence,
    };
    state.preferences.error = null;
    return true;
  }
  const key = (input.event as { key?: string }).key;
  if (!key) return false;
  if (
    input.key === 'settings-palette' &&
    ['maka', 'dusk', 'paper', 'terminal', 'custom'].includes(key)
  )
    saveAppearanceSetting(state.preferences, base, (next) => {
      next.appearance.theme = key as ThemeName;
    });
  else if (input.key === 'settings-language' && ['auto', 'en', 'zh-CN', 'zh-TW'].includes(key))
    saveAppearanceSetting(state.preferences, base, (next) => {
      next.locale = key;
    });
  else if (input.key === 'settings-symbols' && ['ascii', 'unicode'].includes(key))
    saveAppearanceSetting(state.preferences, base, (next) => {
      next.appearance.ascii = key === 'ascii';
    });
  else if (input.key === 'settings-motion' && ['on', 'off'].includes(key))
    saveAppearanceSetting(state.preferences, base, (next) => {
      next.appearance.motion = key === 'on';
    });
  else return false;
  return true;
}
export function settingsView<N extends Node>(
  presentation: Presentation<N>,
  state: AppearanceEditor,
  theme: Theme,
  locale: string,
  category: SettingsCategory,
  open = true,
  scope: InsightsProps['scope'] = null,
  web = false,
  jev = false,
  subagents = false,
): N {
  const ui = presentation.paint(theme),
    t = (key: string) => tuiText(locale, key);
  const categories = settingsCategories.filter(
    (key) => (key !== 'web' || web) && (key !== 'subagents' || subagents),
  );
  const appearance = state.saved?.appearance ?? theme.configuration;
  const busy = state.command !== null || state.checkpoint?.state.kind !== 'ready';
  const icons: Record<string, [string, string]> = {
    'settings-palette': ['◐', 'T'],
    'appearance-open': ['◒', 'C'],
    'settings-language': ['文', 'L'],
    'settings-symbols': ['◇', 'A'],
    'settings-motion': ['≈', 'M'],
    'settings-connections': ['⇄', 'C'],
    'settings-plugins': ['◇', 'P'],
  };
  const icon = (key: string) =>
    ui.box(
      key + '-icon',
      [ui.text(key + '-glyph', icons[key]![appearance.ascii ? 1 : 0], theme.subtle)],
      { terminal: { basis: 4 } },
    );
  const choice = (key: string, options: [string, string][], current: string) =>
    ui.box(
      key + '-row',
      [
        icon(key),
        ui.box(
          key + '-control',
          [
            ui.choice!(
              key,
              t(key),
              options.map(([key, label]) => ({ key, label })),
              current,
              busy,
            ),
          ],
          { terminal: { grow: 1 } },
        ),
      ],
      { row: true, gap: 0 },
    );
  const link = (key: string, label: string, disabled = false) =>
    ui.item!(
      key,
      ui.box(
        key + '-row',
        [
          icon(key),
          ui.box(key + '-label', [ui.text(key + '-text', label, theme.text)], {
            terminal: { grow: 1 },
          }),
          ui.text(key + '-arrow', appearance.ascii ? '>' : '›', theme.subtle),
        ],
        { row: true, gap: 0 },
      ),
      false,
      disabled,
    );
  const title = (key: string) => t(key === 'plugins' ? 'route-plugins' : 'settings-' + key);
  const sections: Record<string, N[]> = {
    subagents: subagents
      ? [
          presentation.slot(
            'subagent-settings',
            'subagentSettings',
            { theme, locale, open, busy: false, shell: { back: 0 } },
            { closed: 'subagent-settings-closed' },
          ),
        ]
      : [],
    host: [
      presentation.slot(
        'network-settings',
        'networkSettings',
        { theme, locale, open, busy: false, shell: { back: 0 } },
        { closed: 'network-settings-closed' },
      ),
    ],
    sessions: [
      presentation.slot(
        'session-defaults',
        'sessionDefaults',
        {
          theme,
          locale,
          open,
          busy: false,
          shell: { back: 0 },
        },
        { closed: 'session-defaults-closed' },
      ),
    ],
    web: web
      ? [
          presentation.slot(
            'web-settings',
            'webSettings',
            {
              theme,
              locale,
              open,
              busy: false,
              shell: { back: 0 },
            },
            { closed: 'web-settings-closed' },
          ),
        ]
      : [],
    jev: jev
      ? [
          presentation.slot(
            'jev-settings',
            'jevSettings',
            {
              theme,
              locale,
              open,
              busy: false,
              shell: { back: 0 },
            },
            { closed: 'jev-settings-closed' },
          ),
        ]
      : [],
    assistant: [
      presentation.slot(
        'assistant-preferences',
        'assistantPreferences',
        {
          theme,
          locale,
          open,
          busy: false,
          shell: { back: 0 },
        },
        { closed: 'assistant-preferences-closed' },
      ),
    ],
    insights: [
      presentation.slot(
        'insights',
        'insights',
        {
          scope,
          theme,
          locale,
          open,
          busy: false,
          shell: { back: 0 },
        },
        { closed: 'insights-closed' },
      ),
    ],
    appearance: [
      choice(
        'settings-palette',
        [
          ['maka', t('settings-palette-dark')],
          ['dusk', t('settings-palette-dusk')],
          ['paper', t('settings-palette-paper')],
          ['terminal', t('settings-palette-default')],
          ...(appearance.customTheme
            ? [['custom', appearance.customTheme.name] as [string, string]]
            : []),
        ],
        appearance.theme ?? (appearance.scheme === 'light' ? 'paper' : 'maka'),
      ),
      link('appearance-open', t('theme-customize'), busy),
    ],
    interface: [
      choice(
        'settings-language',
        [
          ['auto', tuiText(locale, 'language-auto', { language: locale })],
          ['en', 'English'],
          ['zh-CN', '简体中文'],
          ['zh-TW', '繁體中文'],
        ],
        state.saved?.locale ?? 'auto',
      ),
      choice(
        'settings-symbols',
        [
          ['unicode', t('settings-symbols-unicode')],
          ['ascii', t('settings-symbols-ascii')],
        ],
        appearance.ascii ? 'ascii' : 'unicode',
      ),
      choice(
        'settings-motion',
        [
          ['on', t('settings-motion-on')],
          ['off', t('settings-motion-off')],
        ],
        appearance.motion === false ? 'off' : 'on',
      ),
    ],
    models: [
      presentation.slot(
        'settings-models',
        'connections',
        { theme, locale },
        { changed: 'settings-models-changed', closed: 'settings-models-closed' },
      ),
    ],
    plugins: [link('settings-plugins', t('route-plugins'))],
  };
  sections.appearance!.push(...sections.interface!);
  sections.advanced = [
    ui.text('network-heading', t('settings-host'), theme.text, 500),
    ...sections.host!,
    ...(jev
      ? [ui.text('tools-heading', t('settings-jev'), theme.text, 500), ...sections.jev!]
      : []),
    ...sections.plugins!,
  ];
  const page = ui.box(
    'settings-page',
    [
      ...(state.error ? [ui.text('settings-error', t(state.error), theme.warning)] : []),
      ...(state.checkpoint?.state.kind === 'failed'
        ? [
            ui.text('settings-save-failed', t('state-save-failed'), theme.warning),
            ui.button('settings-retry', t('home-retry')),
          ]
        : []),
      ui.box(
        'settings-sections',
        [
          ui.box(
            'settings-categories',
            categories.map((key) =>
              ui.item!(
                'settings-category-' + key,
                ui.text('settings-category-label-' + key, title(key), theme.text),
                category === key,
                false,
                { followFocus: true },
              ),
            ),
            { gap: 1, focusGroup: true, terminal: { basis: 0 } },
          ),
          ui.box('settings-gap-before', [], { terminal: { basis: 0 } }),
          ui.box('settings-divider', [ui.rule!('settings-rule', 'column')], {
            row: true,
            gap: 0,
            terminal: { basis: 0 },
          }),
          ui.box('settings-gap-after', [], { terminal: { basis: 0 } }),
          ...categories.flatMap((key) => [
            ui.box(
              'settings-heading-' + key,
              [ui.text('settings-heading-label-' + key, title(key), theme.text, 650)],
              { terminal: { basis: 1 } },
            ),
            ui.box(
              'settings-section-' + key,
              [
                ui.box('settings-rows-' + key, sections[key], {
                  gap: 1,
                  terminal:
                    key === 'assistant' || key === 'subagents'
                      ? { grow: 1, shrink: true }
                      : { basis: 64, shrink: true },
                }),
              ],
              {
                row: true,
                gap: 0,
              },
            ),
            ui.box('settings-section-gap-' + key, [], {
              terminal: { basis: 1 },
            }),
          ]),
        ],
        {
          gap: 0,
          terminal: { grow: 1 },
          scroll: 'manual',
          wide: {
            min_width: 56,
            direction: 'row',
            scroll: 'none',
            children: {
              'settings-categories': {
                basis_fraction: {
                  numerator: 1,
                  denominator: 3,
                  min: 0,
                  max: 22,
                },
              },
              'settings-gap-before': { basis: 2 },
              'settings-divider': { basis: 1 },
              'settings-gap-after': { basis: 2 },
              ...Object.fromEntries(
                categories.flatMap((key) => [
                  ['settings-heading-' + key, { basis: 0 }],
                  ['settings-section-gap-' + key, { basis: 0 }],
                  [
                    'settings-section-' + key,
                    key === category ? { grow: 1, basis: 0 } : { basis: 0 },
                  ],
                ]),
              ),
            },
          },
        },
      ),
    ],
    {
      gap: 0,
      padding: 0,
      terminal: { grow: 1 },
    },
  );
  return ui.box(
    'settings-inset',
    [
      ui.box('settings-left-inset', [], { terminal: { basis: 1 } }),
      page,
      ui.box('settings-right-inset', [], { terminal: { basis: 1 } }),
    ],
    { row: true, gap: 0, terminal: { grow: 1 } },
  );
}
