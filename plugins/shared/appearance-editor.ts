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

import { editorMenu } from './editor-menu.js';
import type {
  Action,
  ActionOutcome,
  ComponentInput,
  EditorObservation,
  EditorCommand,
  InputProps,
  TextChunk,
  TextFileCommand,
  TextFileObservation,
  ValueCheckpointCommand,
  ValueCheckpointObservation,
} from '../../packages/plugin-sdk/src/index.js';
import type { Theme } from './contracts.js';
import { appearance } from '../settings/appearance/index.js';
import {
  terminalPalette,
  roles,
  type AppearanceConfiguration,
  type ThemeName,
  type CustomTheme,
} from './terminal-theme.js';
import { tuiText } from './locale-catalogs/appearance-editor.js';

import { sameValue } from './value.js';
import type { Node, Presentation } from './view.js';

interface Preferences {
  version: 1;
  appearance: AppearanceConfiguration;
}
interface Field {
  observed: EditorObservation | null;
  command: EditorCommand | null;
  sequence: number;
  initialized: boolean;
}
export interface AppearanceEditor {
  open: boolean;
  saved: Preferences | null;
  draft: Preferences | null;
  checkpoint: ValueCheckpointObservation | null;
  command: ValueCheckpointCommand | null;
  sequence: number;
  role: number;
  fields: { name: Field; hex: Field };
  reads: Record<string, { field: 'name' | 'hex'; captured: EditorObservation }>;
  error: string | null;
  file: {
    observed: TextFileObservation | null;
    command: TextFileCommand | null;
    reload: { draft: string; fields: string } | null;
    message: string | null;
  };
}
const field = (): Field => ({ observed: null, command: null, sequence: 0, initialized: false });
export const initialAppearance = (): AppearanceEditor => ({
  open: false,
  saved: null,
  draft: null,
  checkpoint: null,
  command: null,
  sequence: 0,
  role: 3,
  fields: { name: field(), hex: field() },
  reads: {},
  error: null,
  file: { observed: null, command: null, reload: null, message: null },
});
export function effectiveAppearance(state: AppearanceEditor, base: Theme): Theme {
  const config = (state.open ? state.draft : state.saved)?.appearance;
  if (!config) return base;
  try {
    return appearance(config.scheme, true, config);
  } catch {
    return config.customTheme
      ? appearance(config.scheme, true, {
          ...config,
          customTheme: { ...config.customTheme, name: 'Preview' },
        })
      : base;
  }
}

function valid(value: unknown): Preferences | null {
  if (!value || typeof value !== 'object') return null;
  const item = value as Preferences;
  if (item.version !== 1 || !item.appearance || !['light', 'dark'].includes(item.appearance.scheme))
    return null;
  try {
    terminalPalette(item.appearance);
    return JSON.parse(JSON.stringify({ version: 1, appearance: item.appearance }));
  } catch {
    return null;
  }
}
export function saveAppearanceSetting(
  state: AppearanceEditor,
  base: Theme,
  change: (next: Preferences) => void,
) {
  if (state.open || state.command || state.checkpoint?.state.kind !== 'ready') return;
  const next = valid(state.saved ?? { version: 1, appearance: base.configuration });
  if (!next) return;
  change(next);
  if (!valid(next)) return;
  state.error = null;
  state.command = {
    kind: 'write',
    sequence: ++state.sequence,
    expectedRevision: state.checkpoint.state.revision,
    value: next,
  };
}

function custom(state: AppearanceEditor): CustomTheme {
  const configuration = state.draft!.appearance;
  if (!configuration.customTheme)
    configuration.customTheme = {
      version: 1,
      name: 'My theme',
      base:
        configuration.theme === 'dusk' || configuration.theme === 'paper'
          ? configuration.theme
          : 'maka',
      colors: {},
      syntax: {},
    };
  return configuration.customTheme;
}
function roleHex(state: AppearanceEditor): string {
  const palette = terminalPalette({
    ...state.draft!.appearance,
    theme: 'custom',
    customTheme: custom(state),
  });
  const name = roles[state.role]!;
  const color =
    state.role >= 17
      ? palette.syntax[state.role - 17]
      : (palette as unknown as Record<string, number>)[
          name.replace(/-([a-z])/g, (_, c: string) => c.toUpperCase())
        ];
  return '#' + Number(color).toString(16).padStart(6, '0').toUpperCase();
}
function reset(state: AppearanceEditor, name: 'name' | 'hex', text: string) {
  const f = state.fields[name];
  if (!f.observed) {
    f.initialized = false;
    return;
  }
  f.initialized = true;
  f.command = {
    owner: f.observed.snapshot.text.owner,
    sequence: ++f.sequence,
    basedOn: f.observed.revision,
    edit: { kind: 'reset', text },
  };
}
function setColor(state: AppearanceEditor, value: string) {
  if (!/^#[0-9a-f]{6}$/i.test(value)) {
    state.error = 'theme-hex-invalid';
    return;
  }
  const theme = custom(state);
  const section = state.role < 17 ? (theme.colors ??= {}) : (theme.syntax ??= {});
  section[roles[state.role]!] = value.toUpperCase();
  state.draft!.appearance.theme = 'custom';
  state.error = null;
}
function fileBusy(state: AppearanceEditor): boolean {
  return state.file.command !== null;
}
function fieldVersions(state: AppearanceEditor): string {
  return JSON.stringify([
    state.fields.name.observed?.snapshot.text,
    state.fields.hex.observed?.snapshot.text,
  ]);
}
function fileRead(state: AppearanceEditor, reload: boolean) {
  const observed = state.file.observed;
  if (!observed || observed.state.kind === 'unavailable' || fileBusy(state)) return;
  const sequence = ++state.sequence;
  state.file.command = { owner: observed.owner, sequence, operation: { kind: 'read' } };
  state.file.message = null;
  state.file.reload = reload
    ? { draft: JSON.stringify(state.draft), fields: fieldVersions(state) }
    : null;
}
export function appearanceEvent(
  state: AppearanceEditor,
  input: ComponentInput,
  base: Theme,
  locale: string,
  actions: Action[],
): boolean {
  if (input.kind === 'component' && input.child === 'appearance-file') {
    const observed = input.event as TextFileObservation;
    const file = state.file;
    // A replacement native mount cannot complete a command addressed to the old owner.
    if (file.observed?.owner !== observed.owner) {
      file.command = null;
      file.reload = null;
      file.message = null;
    }
    file.observed = observed;
    if (observed.state.kind === 'idle') fileRead(state, false);
    if (!file.command || observed.sequence !== file.command.sequence) return true;
    if (observed.state.kind === 'loading' || observed.state.kind === 'saving') return true;
    const writing = file.command.operation.kind === 'write';
    file.command = null;
    const reload = file.reload;
    file.reload = null;
    if (observed.state.kind === 'failed') {
      file.message =
        observed.state.error === 'conflict'
          ? 'theme-file-changed'
          : writing
            ? 'theme-file-save'
            : 'theme-file-unreadable';
      return true;
    }
    if (observed.state.kind !== 'ready') return true;
    if (writing) file.message = 'theme-exported';
    if (!reload || !state.open || !state.draft) return true;
    if (reload.draft !== JSON.stringify(state.draft) || reload.fields !== fieldVersions(state)) {
      file.message = 'theme-reload-stale';
      return true;
    }
    if (observed.state.text == null) {
      file.message = 'theme-file-missing';
      return true;
    }
    try {
      const theme = JSON.parse(observed.state.text) as CustomTheme;
      terminalPalette({ ...state.draft.appearance, theme: 'custom', customTheme: theme });
      state.draft.appearance.theme = 'custom';
      state.draft.appearance.customTheme = theme;
      state.reads = {};
      reset(state, 'name', theme.name);
      reset(state, 'hex', roleHex(state));
      state.error = null;
      file.message = 'theme-imported';
    } catch {
      file.message = 'theme-import-invalid';
    }
    return true;
  }
  if (input.kind === 'component' && input.child === 'appearance-checkpoint') {
    const observed = input.event as ValueCheckpointObservation;
    state.checkpoint = observed;
    if (observed.state.kind === 'ready') {
      const preferences = observed.state.value === null ? null : valid(observed.state.value);
      if (observed.state.value !== null && !preferences) {
        state.error = 'theme-file-version';
        return true;
      }
      state.saved = preferences;
      if (state.open && preferences && state.command?.kind !== 'write') {
        state.draft = JSON.parse(JSON.stringify(preferences));
        custom(state);
        reset(state, 'name', custom(state).name);
        reset(state, 'hex', roleHex(state));
      }
      if (state.command?.kind === 'write' && observed.sequence === state.command.sequence) {
        state.command = null;
        state.open = false;
        state.draft = null;
      } else if (state.command?.kind === 'read' && observed.sequence === state.command.sequence) {
        state.command = null;
      }
    }
    return true;
  }
  if (input.kind === 'action' && state.reads[input.key]) {
    const request = state.reads[input.key]!;
    delete state.reads[input.key];
    const outcome = input.event as ActionOutcome;
    const chunk = outcome.status === 'succeeded' ? (outcome.value as TextChunk) : null;
    if (!state.open || !state.draft) return true;
    if (
      !chunk ||
      !sameValue(chunk.metadata, request.captured.snapshot.text) ||
      !sameValue(
        state.fields[request.field].observed?.snapshot.text,
        request.captured.snapshot.text,
      )
    )
      return true;
    if (request.field === 'name') {
      custom(state).name = chunk.text;
      try {
        terminalPalette({ ...state.draft.appearance, theme: 'custom' });
        state.error = null;
      } catch {
        state.error = 'theme-file-name';
      }
    } else setColor(state, chunk.text);
    return true;
  }
  if (input.kind === 'component' && ['appearance-name', 'appearance-hex'].includes(input.child)) {
    const name = input.child === 'appearance-name' ? 'name' : 'hex';
    const f = state.fields[name];
    f.observed = input.event as EditorObservation;
    if (!state.open || !state.draft) return true;
    if (!f.initialized) reset(state, name, name === 'name' ? custom(state).name : roleHex(state));
    else if (f.command && f.observed.receipt?.sequence === f.command.sequence) {
      const conflict = f.observed.receipt.outcome.status === 'conflict';
      f.command = null;
      if (conflict) reset(state, name, name === 'name' ? custom(state).name : roleHex(state));
    } else if (!f.command && !f.observed.snapshot.marked) {
      const key = 'appearance-read-' + ++state.sequence;
      state.reads[key] = { field: name, captured: f.observed };
      actions.push({
        key,
        binding: 'reader',
        member: 0,
        method: 'read',
        input: { start: 0, end: f.observed.snapshot.text.bytes },
        resources: {
          text: {
            child: input.child,
            method: 'snapshot',
            input: {
              kind: 'at_revision',
              owner: f.observed.snapshot.text.owner,
              revision: f.observed.revision,
            },
          },
        },
      });
    }
    return true;
  }
  if (input.kind !== 'raw' || !input.key.startsWith('appearance-')) return false;
  if (input.key === 'appearance-open') {
    state.open = true;
    state.error = null;
    state.draft = JSON.parse(
      JSON.stringify(state.saved ?? { version: 1, appearance: base.configuration }),
    );
    custom(state);
    reset(state, 'name', custom(state).name);
    reset(state, 'hex', roleHex(state));
    return true;
  }
  if (!state.open || !state.draft) return true;
  const saving = state.checkpoint?.state.kind === 'saving';
  if (input.key === 'appearance-close' || input.key === 'appearance-dialog') {
    if (!saving) {
      state.open = false;
      state.draft = null;
      state.reads = {};
      state.file.reload = null;
    }
    return true;
  }
  if (input.key === 'appearance-retry') {
    state.command = { kind: 'read', sequence: ++state.sequence };
    state.error = null;
    return true;
  }
  if (saving || fileBusy(state)) return true;
  if (input.key === 'appearance-file-reload') {
    fileRead(state, true);
    return true;
  }
  if (input.key === 'appearance-file-save') {
    const observed = state.file.observed;
    if (observed?.state.kind !== 'ready' || Object.keys(state.reads).length || state.error)
      return true;
    try {
      terminalPalette({ ...state.draft.appearance, theme: 'custom' });
      state.file.command = {
        owner: observed.owner,
        sequence: ++state.sequence,
        operation: {
          kind: 'write',
          expectedDigest: observed.state.digest,
          text: JSON.stringify(custom(state), null, 2) + '\n',
        },
      };
      state.file.message = null;
    } catch {
      state.error = 'theme-import-invalid';
    }
    return true;
  }
  const choice = input.event as { kind?: string; key?: string };
  if (
    input.key === 'appearance-theme' &&
    ['maka', 'dusk', 'paper', 'terminal', 'custom'].includes(choice.key ?? '')
  )
    state.draft.appearance.theme = choice.key as ThemeName;
  else if (input.key === 'appearance-motion') state.draft.appearance.motion = choice.key === 'on';
  else if (input.key === 'appearance-symbols')
    state.draft.appearance.ascii = choice.key === 'ascii';
  else if (input.key === 'appearance-role') {
    state.role = Math.max(0, roles.indexOf(choice.key!));
    reset(state, 'hex', roleHex(state));
  } else if (
    input.key === 'appearance-base' &&
    ['maka', 'dusk', 'paper'].includes(choice.key ?? '')
  ) {
    state.draft.appearance.customTheme = {
      version: 1,
      name: custom(state).name,
      base: choice.key as 'maka' | 'dusk' | 'paper',
      colors: {},
      syntax: {},
    };
    reset(state, 'hex', roleHex(state));
  } else if (input.key.startsWith('appearance-swatch-')) {
    setColor(state, '#' + input.key.slice('appearance-swatch-'.length));
    reset(state, 'hex', roleHex(state));
  } else if (
    input.key === 'appearance-save' &&
    state.checkpoint?.state.kind === 'ready' &&
    !Object.keys(state.reads).length &&
    !state.error
  ) {
    const next = valid(state.draft);
    if (next)
      state.command = {
        kind: 'write',
        sequence: ++state.sequence,
        expectedRevision: state.checkpoint.state.revision,
        value: next,
      };
    else state.error = 'theme-file-name';
  }
  return true;
}
const swatches = [
  '71A8FD',
  'BE9DF7',
  'EC939B',
  'E7BD7F',
  '7ECFA4',
  '71CCD1',
  '205FC1',
  '804CAF',
  'B33E48',
  '8C5C13',
  '21714F',
  '147078',
  '131720',
  '202837',
  '647186',
  'A0AFC3',
  'E9EDF5',
  'F7F8FC',
];
export function appearanceView<N extends Node>(
  presentation: Presentation<N>,
  state: AppearanceEditor,
  theme: Theme,
  locale: string,
  background: N,
): N {
  const ui = presentation.paint(theme);
  const t = (key: string) => tuiText(locale, key);
  const draft = state.draft;
  const busy =
    state.checkpoint?.state.kind !== 'ready' || state.command?.kind === 'write' || fileBusy(state);
  const select = (key: string, label: string, choices: [string, string][], current: string) =>
    ui.choice!(
      key,
      label,
      choices.map(([key, label]) => ({ key, label })),
      current,
      busy,
    );
  const fieldView = (name: 'name' | 'hex', label: string) =>
    presentation.slot(
      `appearance-${name}`,
      'input',
      {
        menu: editorMenu(theme, locale),
        appearance: theme.input,
        accessibilityLabel: label,
        readOnly: false,
        disabled: busy,
        tabIndex: 0,
        autoFocus: false,
        maxBytes: name === 'hex' ? 7 : 160,
        terminalPaste: 'reject_controls',
        placeholder: name === 'hex' ? '#RRGGBB' : t('theme-custom-name'),
        command: state.fields[name].command,
      } satisfies InputProps,
      { changed: 'appearance-field' },
    );
  const content = ui.box(
    'appearance-content',
    [
      ...(draft
        ? [
            select(
              'appearance-theme',
              t('settings-palette'),
              [
                ['maka', t('settings-palette-dark')],
                ['dusk', t('settings-palette-dusk')],
                ['paper', t('settings-palette-paper')],
                ['terminal', t('settings-palette-default')],
                ['custom', custom(state).name],
              ],
              draft.appearance.theme ?? (draft.appearance.scheme === 'light' ? 'paper' : 'maka'),
            ),
            select(
              'appearance-symbols',
              t('settings-symbols'),
              [
                ['unicode', t('settings-symbols-unicode')],
                ['ascii', t('settings-symbols-ascii')],
              ],
              draft.appearance.ascii ? 'ascii' : 'unicode',
            ),
            select(
              'appearance-motion',
              t('settings-motion'),
              [
                ['on', t('settings-motion-on')],
                ['off', t('settings-motion-off')],
              ],
              draft.appearance.motion === false ? 'off' : 'on',
            ),
          ]
        : []),
      ui.box(
        'appearance-custom',
        [
          ui.text('appearance-name-label', t('theme-name'), theme.muted),
          fieldView('name', t('theme-name')),
          ...(draft
            ? [
                select(
                  'appearance-base',
                  t('settings-palette'),
                  [
                    ['maka', 'Maka'],
                    ['dusk', 'Dusk'],
                    ['paper', 'Paper'],
                  ],
                  custom(state).base ?? 'maka',
                ),
                select(
                  'appearance-role',
                  t('theme-customize'),
                  roles.map((role) => [role, t('theme-role-' + role)]),
                  roles[state.role]!,
                ),
              ]
            : []),
          fieldView('hex', '#RRGGBB'),
          ui.box(
            'appearance-swatches',
            swatches.map((hex) =>
              ui.box(
                'appearance-color-' + hex,
                [
                  ui.button('appearance-swatch-' + hex, theme.ascii ? '*' : '●', busy, false, {
                    background: 0,
                  }),
                ],
                {
                  background: ((Number.parseInt(hex, 16) << 8) | 255) >>> 0,
                  terminal: { basis: 3 },
                },
              ),
            ),
            { row: true, gap: 0, terminal: { wrap: true } },
          ),
          ...(state.file.observed && state.file.observed.state.kind !== 'unavailable'
            ? [
                ui.text(
                  'appearance-file-path',
                  tuiText(locale, 'theme-path', { path: state.file.observed.path ?? '' }),
                  theme.muted,
                ),
                ui.text('appearance-file-hint', t('theme-file-draft-hint'), theme.muted),
              ]
            : []),
          ui.text('appearance-preview', t('theme-preview'), theme.text),
          ui.box(
            'appearance-preview-row',
            [
              ui.text('appearance-preview-accent', 'Maka', theme.accent, 650),
              ui.text('appearance-preview-syntax', 'const answer = 42;', theme.thinking),
              ui.activity('appearance-preview-motion', 'spring'),
            ],
            { row: true },
          ),
        ],
        { visible: draft?.appearance.theme === 'custom', gap: 0 },
      ),
    ],
    { gap: 1, scroll: 'manual', terminal: { grow: 1 } },
  );
  return ui.dialog(
    'appearance-dialog',
    t('settings-appearance'),
    ui.box(
      'appearance-retained',
      [
        background,
        presentation.slot(
          'appearance-file',
          'themeFile',
          { command: state.file.command },
          { changed: 'appearance-file' },
        ),
        presentation.slot(
          'appearance-checkpoint',
          'checkpoint',
          { key: 'preferences', format: 'maka.agent.appearance/v1', command: state.command },
          { changed: 'appearance-checkpoint' },
        ),
      ],
      { gap: 0, terminal: { grow: 1 } },
    ),
    ui.box(
      'appearance-body',
      [
        content,
        ...(draft?.appearance.theme === 'custom' &&
        state.file.observed &&
        state.file.observed.state.kind !== 'unavailable'
          ? [
              ui.box(
                'appearance-file-actions',
                [
                  ui.button('appearance-file-reload', t('theme-reload-file'), busy),
                  ui.button(
                    'appearance-file-save',
                    t('theme-export'),
                    busy ||
                      state.file.observed.state.kind !== 'ready' ||
                      !!state.error ||
                      !!Object.keys(state.reads).length,
                  ),
                ],
                { row: true, gap: 1 },
              ),
            ]
          : []),
        // Save failures must remain visible when the custom editor scrolls.
        ...(state.file.message
          ? [ui.text('appearance-file-message', t(state.file.message), theme.muted)]
          : []),
        ...(state.error ? [ui.text('appearance-error', t(state.error), theme.warning)] : []),
        ...(state.checkpoint?.state.kind === 'failed'
          ? [
              ui.text('appearance-storage-error', t('state-save-failed'), theme.warning),
              ui.button('appearance-retry', t('home-retry')),
            ]
          : []),
        ui.box(
          'appearance-actions',
          [
            ui.button(
              'appearance-close',
              t('session-cancel'),
              state.checkpoint?.state.kind === 'saving',
            ),
            ui.button(
              'appearance-save',
              t('theme-save'),
              busy || !!state.error || !!Object.keys(state.reads).length,
              true,
            ),
          ],
          { row: true, gap: 1 },
        ),
      ],
      { gap: 1, terminal: { grow: 1 } },
    ),
    state.open,
    { height: draft?.appearance.theme === 'custom' ? 32 : 14 },
  );
}
