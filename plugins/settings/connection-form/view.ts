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
  Action,
  CatalogMutationResult,
  Emission,
  Identity,
} from '../../../packages/plugin-sdk/src/index.js';
import { type ConnectionFormProps } from '../../shared/contracts.js';
import { visibleRows } from '../../shared/scroll.js';
import { copy, type Copy, tuiText } from './locale.js';
import { pageHeader } from '../../shared/page-shell.js';
import { compactLabel, output, type Node, type Presentation } from '../../shared/view.js';
import { matchesProvider, providerGroup } from '../search.js';
import { busy, canRemove, canSave, fresh, sameConnection } from './operations.js';
import { advancedFields, names, type FieldName, type State } from './state.js';

export function fieldLabel(name: FieldName, text: Copy) {
  return {
    name: text.connectionName,
    slug: text.connectionSlug,
    endpoint: text.connectionEndpoint,
    models: text.connectionModels,
    configuration: text.connectionConfiguration,
    overrides: text.connectionOverrides,
    overlay: text.connectionOverlay,
  }[name];
}

export function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: ConnectionFormProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const locked = busy(state);
  const loadingProviders = Object.values(state.pending).some(
    (r) => r.kind === 'providers' || r.kind === 'provider',
  );
  const identity: Identity | undefined =
    state.baseline?.target.provider ?? state.provider?.identity;

  const groups = new Map<
    string,
    { label: string; rows: { provider: (typeof state.providers)[number]; at: number }[] }
  >();
  state.providers.forEach((provider, at) => {
    if (state.provider || state.baseline) return;
    if (!matchesProvider(state.search, provider)) return;
    const group = providerGroup(provider);
    if (!groups.has(group.key))
      groups.set(group.key, {
        label: group.key === 'custom' ? tuiText(props.locale, group.label) : group.label,
        rows: [],
      });
    groups.get(group.key)!.rows.push({ provider, at });
  });
  const providerRows = [...groups].flatMap(([key, group]) => {
    const expanded = !!state.search.query || state.providerGroup === key || group.rows.length === 1;
    return [
      ...(group.rows.length > 1
        ? [
            ui.button(
              'provider-group-' + key,
              `${props.theme.ascii ? (expanded ? '-' : '+') : expanded ? '▾' : '▸'} ${group.label}`,
              locked,
              false,
              { align: 'start', background: 0 },
            ),
          ]
        : []),
      ...(expanded
        ? group.rows.map(({ provider, at }) =>
            ui.item(
              'provider-' + at,
              ui.text(
                'provider-label-' + at,
                (group.rows.length > 1 ? '  ' : '') + provider.label,
                props.theme.text,
                400,
                { truncate: true },
              ),
              state.providerActive === at,
              locked,
              { followFocus: true, submit: true },
            ),
          )
        : []),
    ];
  });
  const form = ui.box(
    'form',
    [
      ...pageHeader(
        presentation,
        state,
        props,
        events,
        state.baseline ? text.editConnection : text.createConnection,
        'form-back',
        false,
      ),
      ...(state.notice
        ? [
            ui.text(
              'form-notice',
              text[state.notice] +
                (state.invalidField ? ': ' + fieldLabel(state.invalidField, text) : ''),
              props.theme.muted,
            ),
          ]
        : []),
      ...(!fresh(state, props)
        ? [ui.text('form-target-changed', text.connectionChanged, props.theme.muted)]
        : []),
      // Keep the original search owner alive while its captured read settles.
      ui.box(
        'provider-search',
        [
          ui.text('provider-heading', text.chooseProvider, props.theme.text, 550),
          ui.box(
            'provider-query-box',
            [
              presentation.slot(
                'provider-query',
                'input',
                {
                  appearance: props.theme.input,
                  accessibilityLabel: text.searchProviders,
                  placeholder: text.searchProviders,
                  autoFocus: true,
                  tabIndex: 1,
                  readOnly: false,
                  disabled: !!state.provider || !!state.baseline,
                },
                { changed: 'provider-query', submitted: 'submitted' },
              ),
            ],
            {
              gap: 0,
              shortcuts: ['down', 'up'].map((key) => ({
                key,
                modifiers: [],
                input: 'provider-focus',
                enabled: !locked && providerRows.length > 0,
              })),
            },
          ),
          ...(state.search.error ? [ui.text('search-error', text[state.search.error])] : []),
        ],
        { visible: !state.baseline && !state.provider, gap: 1 },
      ),
      ...(!state.baseline && !state.provider
        ? [
            ui.box(
              'provider-actions',
              [
                ui.button(
                  'provider-refresh',
                  props.theme.ascii ? 'r' : '↻',
                  locked || loadingProviders,
                  false,
                  { background: 0 },
                  undefined,
                  text.refreshConnections,
                ),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
            ...(loadingProviders
              ? [ui.text('provider-loading', text.connecting, props.theme.muted)]
              : []),
            ...(state.providersFailed
              ? [ui.text('provider-failed', text.providersUnavailable)]
              : []),
            ...(!locked &&
            !loadingProviders &&
            state.providers.length > 0 &&
            !state.providers.some((p) => matchesProvider(state.search, p))
              ? [ui.text('providers-no-matches', text.noSearchResults)]
              : []),
            ...(state.providers.length === 0 && !loadingProviders && !state.providersFailed
              ? [ui.text('providers-empty', text.noProviders)]
              : []),
            presentation.reading(
              'provider-list',
              {
                command: state.providerFocus,
                content: ui.box('provider-rows', providerRows, {
                  focusGroup: true,
                  scroll: 'manual',
                  gap: 0,
                  terminal: { grow: 1, basis: 0 },
                }),
              },
              { input: 'provider-input' },
            ),
          ]
        : [
            ui.box(
              'provider-summary',
              [
                ...(!state.baseline
                  ? [
                      ui.button(
                        'change-provider',
                        text.changeProvider,
                        locked || Object.values(state.fields).some((field) => !!field.command),
                      ),
                    ]
                  : []),
                ui.text(
                  'provider-identity',
                  compactLabel(state.provider?.descriptor.label ?? identity?.name ?? '', 36),
                  props.theme.muted,
                ),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
          ]),
      ...(!state.baseline && state.provider
        ? [
            ui.text(
              'setup-help',
              state.provider.descriptor.anonymous
                ? tuiText(props.locale, 'anonymous-setup')
                : text.connectionSetupHelp,
              props.theme.muted,
            ),
          ]
        : []),
      // Keep native owners mounted when advanced settings are collapsed.
      ...names.map((name) => {
        const f = state.fields[name],
          label = fieldLabel(name, text);
        const available =
          !!(state.baseline || state.provider) && (name !== 'slug' || !state.baseline);
        return ui.box(
          'pane-' + name,
          [
            ui.box(
              'field-heading-' + name,
              [
                ui.text('label-' + name, label, props.theme.text, 550),
                ...(state.baseline ||
                (name === 'endpoint' && !state.provider?.descriptor.customEndpoint) ||
                ['models', 'configuration', 'overrides', 'overlay'].includes(name)
                  ? [
                      ui.button(
                        'edit-' + name,
                        (f.enabled
                          ? state.baseline
                            ? text.keepSavedField
                            : text.useDefaultField
                          : text.replaceField) +
                          ': ' +
                          label,
                        locked ||
                          (name === 'configuration' && state.fields.endpoint.enabled) ||
                          (name === 'endpoint' && state.fields.configuration.enabled),
                      ),
                    ]
                  : []),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
            ...(['models', 'overrides', 'overlay'].includes(name)
              ? [
                  ui.text(
                    'replace-help-' + name,
                    state.baseline ? text.replaceCollectionHelp : text.newCollectionHelp,
                    props.theme.muted,
                  ),
                ]
              : []),
            ...(name === 'configuration'
              ? [ui.text('configuration-help', text.configurationHelp, props.theme.muted)]
              : []),
            ...(f.failed
              ? [
                  ui.text(
                    'field-failed-' + name,
                    state.baseline ? text.fieldPrefillFailed : text.defaultPrefillFailed,
                    props.theme.muted,
                  ),
                  ui.button('empty-' + name, text.replaceFromEmpty, locked),
                ]
              : []),
            ui.box(
              'owner-pane-' + name,
              [
                presentation.slot(
                  'field-' + name,
                  ['slug', 'name', 'endpoint'].includes(name) ? 'input' : 'editor',
                  {
                    appearance: ['slug', 'name', 'endpoint'].includes(name)
                      ? props.theme.input
                      : props.theme.textarea,
                    accessibilityLabel: label,
                    placeholder: text.fieldPlaceholder + label,
                    readOnly: locked || !f.enabled,
                    disabled: false,
                    tabIndex: f.enabled ? 0 : -1,
                    autoFocus:
                      name === (state.provider?.descriptor.anonymous ? 'endpoint' : 'name'),
                    command: f.command,
                    ...(['slug', 'name', 'endpoint'].includes(name) ? {} : { softWrap: true }),
                  },
                  {
                    changed: 'field-changed',
                    ...(name === 'slug' ? { submitted: 'field-submitted' } : {}),
                  },
                ),
              ],
              { visible: f.enabled, gap: 0 },
            ),
            ...(!f.enabled
              ? [
                  ui.text(
                    'kept-' + name,
                    state.baseline ? text.savedFieldKept : text.defaultFieldKept,
                    props.theme.muted,
                  ),
                ]
              : []),
          ],
          {
            visible:
              available &&
              (!advancedFields.includes(name) || state.advanced) &&
              (name !== 'endpoint' ||
                state.advanced ||
                !!state.baseline ||
                !!state.provider?.descriptor.customEndpoint ||
                !!state.provider?.descriptor.anonymous),
            gap: 1,
            padding: 0,
          },
        );
      }),
      ...(state.baseline || state.provider
        ? [
            ui.button(
              'form-advanced',
              state.advanced ? text.basicSettings : text.advancedSettings,
              locked,
            ),
          ]
        : []),
      ...(state.advanced
        ? [
            ui.button(
              'connection-enabled',
              state.enabled ? text.connectionEnabled : text.connectionDisabled,
              locked,
            ),
            ui.button('form-history', text.earlierConnectionOperations),
            ...(state.baseline
              ? [ui.button('form-remove', text.removeConnection, !canRemove(state, props))]
              : []),
          ]
        : []),
    ],
    {
      visible: !state.historyOpen && !state.confirmation,
      scroll: state.provider || state.baseline ? 'anchor' : 'none',
      gap: 1,
      terminal: state.provider || state.baseline ? { shrink: true } : { grow: 1 },
    },
  );
  const footer = ui.box(
    'form-footer',
    [
      ui.box(
        'form-actions',
        [
          ui.button(
            'form-save',
            state.baseline || state.provider?.descriptor.anonymous
              ? text.saveConnection
              : text.continueSignIn,
            !canSave(state, props),
            true,
          ),
          ...(state.current?.reference
            ? [
                ui.button(
                  'form-check',
                  text.check,
                  Object.values(state.pending).some(
                    (r) => r.kind === 'receipt' || r.kind === 'mutate',
                  ),
                ),
              ]
            : []),
          ...(state.conflict || !fresh(state, props)
            ? [
                ui.button(
                  'form-review',
                  text.reviewConnection,
                  locked || !sameConnection(state, props),
                ),
              ]
            : []),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    {
      visible: !!(state.baseline || state.provider) && !state.historyOpen && !state.confirmation,
      gap: 0,
      terminal: { shrink: false },
    },
  );
  const main = ui.box('form-main', [form, footer], { gap: 1, terminal: { grow: 1 } });
  const reviewValue =
    state.confirmation === 'remove'
      ? [
          text.connectionName + ': ' + state.baseline!.name,
          text.connectionSlug + ': ' + state.baseline!.target.slug,
          text.connectionProvider + ': ' + state.baseline!.target.provider.name,
        ].join('\n')
      : state.reviewed
        ? JSON.stringify(state.reviewed)
        : state.reviewedCatalog !== null
          ? JSON.stringify({
              catalogRevision: state.reviewedCatalog,
              provider: state.provider?.identity,
            })
          : '';
  // Full human-readable identity, with bounded terminal lines. Native version
  // and package internals remain in the frozen request, not confirmation copy.
  const reviewLines = reviewValue.split(/\r?\n/u).flatMap((line) => {
    const characters = [...line],
      lines: string[] = [];
    for (let at = 0; at < characters.length; at += 36)
      lines.push(characters.slice(at, at + 36).join(''));
    return lines.length ? lines : [''];
  });
  const pageSize = 7;
  const pages = Math.max(1, Math.ceil(reviewLines.length / pageSize));
  const page = Math.min(state.reviewPage, pages - 1);
  const confirmation = ui.box(
    'form-confirmation',
    [
      ui.text(
        'confirm-question',
        state.confirmation === 'leave'
          ? state.current
            ? text.connectionLeaveUnknown
            : text.discardConnection
          : state.confirmation === 'remove'
            ? text.confirmRemoveConnection
            : text.connectionRebaseHelp,
      ),
      ui.box(
        'confirm-actions',
        [
          ui.button('form-cancel', text.keepEditing),
          ...(state.confirmation === 'leave'
            ? [ui.button('form-discard', text.discard)]
            : [
                ...(state.confirmation === 'remove'
                  ? [
                      ui.button(
                        'form-confirm-remove',
                        text.removeConnection,
                        !canRemove(state, props),
                      ),
                    ]
                  : [
                      ui.button(
                        'form-rebase',
                        text.useConnection,
                        (!state.reviewed && state.reviewedCatalog === null) || locked,
                      ),
                    ]),
              ]),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
      ...(state.confirmation === 'rebase' || state.confirmation === 'remove'
        ? [
            presentation.reading(
              'connection-review-reading',
              {
                content: ui.box(
                  'review-lines',
                  reviewLines
                    .slice(page * pageSize, (page + 1) * pageSize)
                    .map((line, at) => ui.text('review-line-' + at, line)),
                  { scroll: 'manual', terminal: { grow: 1, basis: 0 } },
                ),
              },
              { observed: 'review-observed' },
            ),
          ]
        : []),
    ],
    { visible: !!state.confirmation, gap: 1 },
  );
  const earlier = state.historyOpen
    ? presentation.reading(
        'connection-history-reading',
        {
          content: ui.box(
            'form-earlier',
            [
              ui.button('form-history-back', text.back, false, false, {
                background: 0,
                align: 'start',
              }),
              ui.text('earlier-heading', text.earlierConnectionOperations, props.theme.text, 550),
              ui.text('earlier-help', text.earlierConnectionHelp, props.theme.muted),
              ui.box(
                'earlier-actions',
                [
                  ui.button(
                    'form-history-refresh',
                    props.theme.ascii ? 'r' : '↻',
                    false,
                    false,
                    { background: 0 },
                    undefined,
                    text.refreshConnections,
                  ),
                ],
                { row: true, gap: 1, terminal: { wrap: true } },
              ),
              ...(state.historyFailed ? [ui.text('earlier-failed', text.historyUnavailable)] : []),
              ...visibleRows(state, 'connection-history-reading', state.earlier).map(
                ({ entry, at }) => {
                  const receipt = entry.result?.receipt;
                  const value =
                    receipt?.kind === 'recorded' && receipt.outcome.kind === 'returned'
                      ? (receipt.outcome.value as CatalogMutationResult)
                      : null;
                  return ui.box(
                    'earlier-entry-' + at,
                    [
                      ui.text(
                        'earlier-result-' + at,
                        value?.kind === 'committed'
                          ? (entry.reference.method === 'remove'
                              ? text.connectionRemoved
                              : text.connectionSaved) +
                              ' ' +
                              (value.connection?.connectionId ?? '')
                          : value
                            ? text.historyRejected
                            : text.historyUnknown,
                      ),
                      ui.button(
                        'form-earlier-' + at,
                        text.check,
                        Object.values(state.pending).some(
                          (r) => r.kind === 'earlier' && r.cursor === entry.cursor,
                        ),
                      ),
                    ],
                    { gap: 0 },
                  );
                },
              ),
            ],
            { visible: state.historyOpen, scroll: 'anchor', terminal: { grow: 1 }, gap: 1 },
          ),
        },
        { input: 'history-input', observed: 'history-observed' },
      )
    : ui.box('history-hidden', [], { visible: false });

  const out = output(
    state,
    ui.box(
      'connection-form',
      [
        ui.dialog(
          'form-auxiliary',
          state.historyOpen ? text.earlierConnectionOperations : text.editConnection,
          main,
          ui.box('form-auxiliary-content', [confirmation, earlier], { gap: 0 }),
          !!state.confirmation || state.historyOpen,
          { width: 72, height: state.confirmation === 'leave' ? 8 : 24 },
        ),
      ],
      {
        autoFocus: true,
        shortcuts: [
          {
            key: 'escape',
            modifiers: [],
            input: state.confirmation
              ? 'form-cancel'
              : state.historyOpen
                ? 'form-history-back'
                : 'form-back',
            enabled: true,
          },
        ],
        gap: 0,
        padding: props.shell ? 0 : 1,
        scroll: 'none',
        terminal: { grow: 1 },
      },
    ),
    events,
  );
  out.actions = actions;
  return out;
}
