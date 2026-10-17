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

import type {
  DeploymentRef,
  ManagedInstance,
  InstanceSettingsPage,
  SettingField,
  SettingValue,
  SettingChange,
  EditorObservation,
  EditorCommand,
  TextChunk,
} from '../../packages/plugin-sdk/src/index.js';
import type { ConnectionsProps } from './contracts.js';
import { copy, type Copy } from './locale-catalogs/instance-settings.js';
import { sameValue } from './value.js';
import { compactLabel, type Node, type Presentation } from './view.js';

export interface InstanceSettings {
  source: DeploymentRef;
  instance: ManagedInstance;
  open: boolean;
  page: (Omit<InstanceSettingsPage, 'fields'> & { fields: SettingField[] }) | null;
  path: string[];
  after: string | null;

  window: number;
  changes: SettingChange[];
  field: { path: string[]; descriptor: SettingField; initial: string } | null;
  observation: EditorObservation | null;
  command: EditorCommand | null;
  sequence: number;
  initialized: boolean;
  conflict: boolean;
  discard: 'reload' | 'close' | ManagedInstance | null;
  notice: keyof Copy | null;
}
export const instanceSettings = (
  source: DeploymentRef,
  instance: ManagedInstance,
): InstanceSettings => ({
  source,
  instance,
  open: true,
  page: null,
  path: [],
  after: null,

  window: 0,
  changes: [],
  field: null,
  observation: null,
  command: null,
  sequence: 0,
  initialized: false,
  conflict: false,
  discard: null,
  notice: null,
});
export function settingValue(state: InstanceSettings, field: SettingField) {
  const change = state.changes.find((change) => sameValue(change.path, [...state.path, field.key]));
  return (change ? change.value : field.value) ?? null;
}
export function settingOptions(field: SettingField): SettingValue[] {
  const options: SettingValue[] = field.options.length
    ? field.options
    : field.kind === 'boolean'
      ? [
          { kind: 'boolean', value: true },
          { kind: 'boolean', value: false },
        ]
      : [];
  return options.length && field.nullable && !options.some((option) => option.kind === 'null')
    ? [...options, { kind: 'null' }]
    : options;
}
export function editSetting(state: InstanceSettings, path: string[], value: SettingValue | null) {
  const index = state.changes.findIndex((change) => sameValue(change.path, path));
  if (index < 0 && state.changes.length >= 32) {
    state.notice = 'settingsLimit';
    return;
  }
  if (!path.length || path.length > 16) return;
  const change: SettingChange = { path: path as SettingChange['path'], value };
  const changes = state.changes.slice();
  if (index < 0) changes.push(change);
  else changes[index] = change;
  // Reserve room for navigation within the 16 KiB UI checkpoint. The same
  // frozen draft also fits the original caller intent's 32 KiB recovery value.
  const encoded = JSON.stringify({
    source: state.source,
    instance: state.instance,
    path: state.path,
    changes,
    open: state.open,
  });
  let bytes = 0;
  for (const character of encoded) {
    const code = character.codePointAt(0)!;
    bytes += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
  }
  if (bytes > 14336) {
    state.notice = 'settingsLimit';
    return;
  }
  state.changes = changes;
  state.notice = null;
}
export function openSetting(state: InstanceSettings, field: SettingField) {
  const value = settingValue(state, field);
  state.field = {
    path: [...state.path, field.key],
    descriptor: field,
    initial: value?.kind === 'text' || value?.kind === 'number' ? value.text : '',
  };
  state.initialized = false;
  state.notice = null;
  initializeField(state);
}
function initializeField(state: InstanceSettings) {
  if (!state.field || !state.observation || state.initialized || state.command) return;
  state.command = {
    owner: state.observation.snapshot.text.owner,
    sequence: ++state.sequence,
    basedOn: state.observation.revision,
    edit: { kind: 'reset', text: state.field.initial },
  };
  state.initialized = true;
}
export function observeSetting(state: InstanceSettings, observation: EditorObservation) {
  if (
    state.observation &&
    state.observation.snapshot.text.owner !== observation.snapshot.text.owner
  ) {
    state.command = null;
    state.initialized = false;
  }
  state.observation = observation;
  if (
    state.command &&
    observation.receipt?.owner === state.command.owner &&
    observation.receipt.sequence === state.command.sequence
  ) {
    if (observation.receipt.outcome.status !== 'applied') {
      state.notice = 'settingsReadFailed';
      state.initialized = false;
    }
    state.command = null;
  }
  initializeField(state);
}
export function acceptSettingText(
  state: InstanceSettings,
  captured: EditorObservation,
  chunk: TextChunk | null,
  path: string[],
) {
  if (!state.field || !sameValue(state.field.path, path)) return;
  if (
    !chunk ||
    !sameValue(chunk.metadata, captured.snapshot.text) ||
    !sameValue(state.observation?.snapshot.text, captured.snapshot.text)
  ) {
    state.notice = 'settingsReadFailed';
    return;
  }
  const text = state.field.descriptor.kind === 'number' ? chunk.text.trim() : chunk.text;
  if ([...text].length > 4096) {
    state.notice = 'settingsReadFailed';
    return;
  }
  if (
    state.field.descriptor.kind === 'number' &&
    (text.length > 128 || !/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/.test(text))
  ) {
    state.notice = 'settingsInvalidNumber';
    return;
  }
  editSetting(state, path, {
    kind: state.field.descriptor.kind === 'number' ? 'number' : 'text',
    text,
  });
  if (state.notice !== 'settingsLimit') state.field = null;
}
function fieldLabel(field: SettingField): string {
  return field.label === field.key
    ? field.label
        .replace(/([a-z])([A-Z])/g, '$1 $2')
        .replace(/_/g, ' ')
        .replace(/^./, (c) => c.toUpperCase())
    : field.label;
}
function display(value: SettingValue | null, props: ConnectionsProps): string {
  const text = copy(props.locale);
  if (value?.kind === 'boolean') return value.value ? text.settingOn : text.settingOff;
  if (value?.kind === 'text' || value?.kind === 'number') return value.text || text.settingUnset;
  return value?.kind === 'null' ? text.settingNone : text.settingUnset;
}
export function instanceSettingsView<N extends Node>(
  presentation: Presentation<N>,
  props: ConnectionsProps,
  state: InstanceSettings,
  busy: boolean,
  notice: string | null,
  diagnostic: string | null,
  recoverable: boolean,
  navigationReady = true,
): N {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const editable = !busy && !state.conflict;
  const rows = (state.page?.fields ?? [])
    .slice(state.window, state.window + 64)
    .map((field, offset) => {
      const index = state.window + offset;
      const value = settingValue(state, field);
      const group = field.kind === 'object' || field.kind === 'array';
      const disabled = !editable || !field.editable;
      const label = fieldLabel(field);
      const options = settingOptions(field);
      const control =
        options.length && ui.choice
          ? ui.choice(
              'instance-setting-choice-' + index,
              label,
              options.map((value, at) => ({
                key: String(at),
                label: compactLabel(display(value, props), 120),
              })),
              options.some((option) => sameValue(option, value))
                ? String(options.findIndex((option) => sameValue(option, value)))
                : null,
              disabled,
            )
          : ui.button(
              'instance-setting-' + index,
              `${label}  ${group ? '›' : compactLabel(display(value, props), 120)}`,
              disabled || (group && !field.present),
              false,
              { background: 0, align: 'start' },
            );
      return ui.box(
        'instance-setting-row-' + index,
        [
          control,
          ...(field.description
            ? [
                ui.text(
                  'instance-setting-description-' + index,
                  field.description,
                  props.theme.muted,
                ),
              ]
            : []),
          ...(!field.editable
            ? [
                ui.text(
                  'instance-setting-unsupported-' + index,
                  text.settingUnsupported,
                  props.theme.muted,
                ),
              ]
            : []),
          ...(field.nullable && !group && (!options.length || !ui.choice)
            ? [
                ui.button('instance-setting-null-' + index, text.settingNone, disabled, false, {
                  background: 0,
                  align: 'start',
                }),
              ]
            : []),
          ...(!field.required &&
          field.present &&
          !group &&
          (state.path.length > 0 || state.instance.path.length === 1)
            ? [
                ui.button('instance-setting-clear-' + index, text.settingRemove, disabled, false, {
                  background: 0,
                  align: 'start',
                }),
              ]
            : []),
        ],
        { gap: 0 },
      );
    });
  const body = ui.box(
    'instance-settings-body',
    [
      ui.box(
        'instance-settings-header',
        [
          ...(!props.shell
            ? [
                ui.button('instance-settings-back', text.settingsBack, busy, false, {
                  background: 0,
                  align: 'start',
                }),
                ui.text(
                  'instance-settings-title',
                  text.instanceConfiguration,
                  props.theme.text,
                  600,
                ),
              ]
            : []),
          ui.text(
            'instance-settings-path',
            [...state.instance.path, ...state.path].join(' / '),
            props.theme.muted,
          ),
        ],
        { gap: 0 },
      ),
      presentation.reading(
        'instance-settings-reading',
        {
          command: null,
          content: ui.box(
            'instance-settings-fields',
            [
              ...(state.path.length ? [ui.button('instance-settings-up', text.back, busy)] : []),
              ...rows,
              ...(!rows.length
                ? [
                    ui.text(
                      'instance-settings-empty',
                      busy ? text.connecting : text.settingsEmpty,
                      props.theme.muted,
                    ),
                  ]
                : []),
            ],
            { gap: 1, scroll: 'anchor', terminal: { grow: 1 } },
          ),
        },
        { input: 'settings-input', observed: 'settings-observed' },
      ),
      ...(!navigationReady
        ? [ui.text('instance-settings-preparing', text.settingsPreparing, props.theme.muted)]
        : []),
      ...(state.notice
        ? [ui.text('instance-settings-notice', text[state.notice], props.theme.warning)]
        : []),
      ...(notice ? [ui.text('instance-settings-status', notice, props.theme.muted)] : []),
      ...(diagnostic ? [ui.text('instance-settings-error', diagnostic, props.theme.warning)] : []),
      ui.box(
        'instance-settings-controls',
        [
          ui.button(
            'instance-settings-save',
            text.settingsSave,
            !editable || !state.changes.length || !navigationReady,
            true,
          ),
          ui.button('instance-settings-reload', text.settingsReload, busy),
          ...(recoverable ? [ui.button('deployment-check', text.check, false)] : []),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    {
      terminal: { grow: 1 },
      gap: 1,
      shortcuts: [
        {
          key: 'escape',
          modifiers: [],
          input: 'instance-settings-back',
          enabled: !state.field && !state.discard && !busy,
        },
      ],
    },
  );
  return ui.box(
    'instance-settings-page',
    [
      ui.dialog(
        'instance-setting-dialog',
        state.discard
          ? text.settingsDiscardQuestion
          : state.field
            ? fieldLabel(state.field.descriptor)
            : text.instanceConfiguration,
        body,
        ui.box(
          'instance-setting-dialog-body',
          [
            ...(state.discard
              ? [ui.text('instance-settings-discard-question', text.settingsDiscardQuestion)]
              : []),
            // Keep the original native buffer mounted across field navigation.
            ui.box(
              'instance-setting-editor-region',
              [
                presentation.slot(
                  'instance-setting-editor',
                  'editor',
                  {
                    appearance: props.theme.textarea,
                    accessibilityLabel: state.field
                      ? fieldLabel(state.field.descriptor) || text.instanceConfiguration
                      : text.instanceConfiguration,
                    readOnly: busy,
                    disabled: busy,
                    softWrap: true,
                    terminalRows: { min: 3, max: 6 },
                    tabIndex: 0,
                    autoFocus: true,
                    placeholder: '',
                    command: state.command,
                  },
                  { changed: 'setting-changed' },
                ),
              ],
              { visible: !state.discard && !!state.field, terminal: { grow: 1 } },
            ),
            ...(state.notice
              ? [ui.text('instance-setting-error', text[state.notice], props.theme.warning)]
              : []),
            ui.box(
              'instance-setting-dialog-controls',
              [
                ui.button('instance-setting-cancel', text.cancel, busy),
                ui.button(
                  state.discard ? 'instance-settings-discard' : 'instance-setting-apply',
                  state.discard ? text.settingsDiscard : text.settingApply,
                  busy ||
                    (!state.discard &&
                      (!state.initialized ||
                        !!state.command ||
                        !state.observation ||
                        !!state.observation.snapshot.marked)),
                  true,
                ),
              ],
              { row: true, gap: 1 },
            ),
          ],
          { gap: 1, terminal: { grow: 1 } },
        ),
        !!state.field || !!state.discard,
        { width: 60, height: 12 },
      ),
    ],
    { visible: state.open, gap: 0, terminal: { grow: 1 } },
  );
}
