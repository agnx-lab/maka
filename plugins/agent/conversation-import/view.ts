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

import { editorMenu } from '../../shared/editor-menu.js';
import type {
  Action,
  AgentImportSource,
  Emission,
  InputProps,
} from '../../../packages/plugin-sdk/src/index.js';
import { type ImportsProps } from '../../shared/contracts.js';
import { visibleRows } from '../../shared/scroll.js';
import { copy } from './locale.js';
import { pageHeader } from '../../shared/page-shell.js';
import { compactLabel, output, type Node, type Presentation } from '../../shared/view.js';
import { locked } from './operations.js';
import { formatName, type FieldName, type State } from './state.js';

export function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: ImportsProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale),
    disabled = locked(state, props);
  const editing = disabled || !!state.confirm || !!state.picker;
  const fieldView = (name: FieldName, label: string, visible: boolean) =>
    ui.box(
      `field-${name}`,
      [
        ui.text(`label-${name}`, label, props.theme.muted),
        presentation.slot(
          `import-${name}`,
          'input',
          {
            menu: editorMenu(props.theme, props.locale),
            appearance: props.theme.input,
            accessibilityLabel: label,
            placeholder: label,
            autoFocus: visible,
            tabIndex: name === 'database' ? 2 : 1,
            readOnly: false,
            disabled: editing || !!state.fields[name].command || !state.fields[name].ready,
            command: state.fields[name].command,
          } satisfies InputProps,
          { changed: `field-${name}`, submitted: `submit-${name}` },
        ),
      ],
      { visible, gap: 0 },
    );
  const row = (id: string, title: string, detail: string, menu: N[] = []) => {
    const body = ui.box(
      'body-' + id,
      [
        ui.text('title-' + id, title, props.theme.text, 500, { truncate: true }),
        ui.text('detail-' + id, detail, props.theme.muted, 400, { truncate: true }),
      ],
      { gap: 0 },
    );
    {
      const item = ui.item(id, body, state.selected === id, disabled, {
        followFocus: true,
        submit: true,
      });
      return menu.length ? ui.contextMenu('menu-' + id, text.projectsActions, item, menu) : item;
    }
  };
  const sourceActions = (source: AgentImportSource, prefix = '') => [
    ui.button(prefix + 'browse-' + source.id, text.importsOpen, disabled),
    ui.button(prefix + 'edit-' + source.id, text.importsEdit, disabled),
    ui.button(prefix + 'remove-' + source.id, text.importsRemove, disabled),
  ];
  const rows =
    state.route === 'sources'
      ? state.sources.map((source) =>
          row(
            'source-' + source.id,
            source.name,
            formatName(source.location.kind) + ' · ' + source.path,
            sourceActions(source),
          ),
        )
      : state.route === 'catalog'
        ? visibleRows(state, 'import-catalog', state.entries).map(({ entry, at: index }) =>
            row('entry-' + index, entry.title, entry.cwd ?? entry.path),
          )
        : state.route === 'copies'
          ? visibleRows(state, 'import-copies', state.copies).map(({ entry: session }) =>
              row('copy-' + session.id, session.title, session.workspace.path),
            )
          : state.route === 'preview'
            ? visibleRows(state, 'import-preview', state.preview?.items ?? []).map(
                ({ entry: item }) =>
                  ui.box(
                    'preview-' + item.index,
                    [
                      ui.text(
                        'role-' + item.index,
                        text[
                          (
                            {
                              user: 'importsUser',
                              assistant: 'importsAssistant',
                              tool_call: 'importsToolCall',
                              tool_result: 'importsToolResult',
                              note: 'importsNote',
                            } as const
                          )[item.kind]
                        ],
                        props.theme.muted,
                        500,
                      ),
                      ui.text('text-' + item.index, item.text, props.theme.text),
                      ...(item.truncated
                        ? [
                            ui.text(
                              'truncated-' + item.index,
                              text.importsTruncated,
                              props.theme.muted,
                            ),
                          ]
                        : []),
                    ],
                    { gap: 1 },
                  ),
              )
            : [];
  const chosenSource = state.sources.find((source) => state.selected === 'source-' + source.id);
  const title =
    state.route === 'form'
      ? state.form?.id
        ? text.importsEdit
        : text.importsAdd
      : state.route === 'preview'
        ? text.importsPreview
        : state.route === 'copies'
          ? text.importsHistory
          : text.importsTitle;
  const form = ui.box(
    'source-form',
    [
      ui.text('source-help', text.importsSourceHelp, props.theme.muted),
      ui.box(
        'source-formats',
        (['codex', 'claude_code', 'open_code'] as const).map((format) =>
          ui.button('format-' + format, formatName(format), editing, state.form?.format === format),
        ),
        { row: true, terminal: { wrap: true } },
      ),
      fieldView('name', text.importsName, state.route === 'form'),
      fieldView(
        'database',
        text.importsDatabase,
        state.route === 'form' && state.form?.format === 'open_code',
      ),
      ...(state.form?.directory
        ? [ui.text('source-path', state.form.directory.path, props.theme.muted)]
        : []),
      ui.button('source-directory', text.importsChooseSource, editing),
    ],
    { visible: state.route === 'form', gap: 1, scroll: 'anchor', terminal: { grow: 1, basis: 0 } },
  );
  const contents = ui.box(
    'imports-content',
    [
      ...(state.route === 'sources' && !rows.length && !state.pending
        ? [ui.text('empty-sources', text.importsEmpty)]
        : []),
      ...(state.route === 'catalog' && !rows.length && !state.pending
        ? [ui.text('empty-catalog', text.importsCatalogEmpty)]
        : []),
      ...(state.route === 'copies' && !rows.length && !state.pending
        ? [ui.text('empty-copies', text.importsHistoryEmpty)]
        : []),
      ...rows,
    ],
    {
      scroll: 'anchor',
      focusGroup:
        state.route === 'sources' || state.route === 'catalog' || state.route === 'copies',
      gap: 1,
      terminal: { grow: 1, basis: 0 },
    },
  );
  const body = ui.box(
    'imports-page',
    [
      ...pageHeader(
        presentation,
        state,
        props,
        events,
        title,
        'back',
        !!state.pending && !state.current,
      ),
      ...(state.restoring || state.pending
        ? [
            ui.text(
              'imports-loading',
              state.restoring ? text.projectsChecking : text.connecting,
              props.theme.muted,
            ),
          ]
        : []),
      ...(state.notice ? [ui.text('imports-notice', text[state.notice], props.theme.muted)] : []),
      ...(state.historyFailed
        ? [ui.button('history-retry', text.sourceRetry, !!state.pending)]
        : []),
      ...(state.current
        ? [ui.button('receipt', text.executionReceipt, !!state.pending || !state.current.reference)]
        : []),
      ...(state.latest && state.notice === 'importsDone'
        ? [ui.button('open-latest', text.importsOpen, disabled)]
        : []),
      ui.box(
        'catalog-search',
        [
          fieldView('search', text.importsSearch, state.route === 'catalog'),
          ui.box(
            'catalog-search-actions',
            [
              ui.button('search', text.importsSearch, editing),
              ui.button('archived', text.importsArchived, editing, state.archived),
            ],
            { row: true, terminal: { wrap: true } },
          ),
        ],
        { visible: state.route === 'catalog', gap: 0 },
      ),
      ui.box(
        'preview-summary',
        [
          fieldView('title', text.importsConversationName, state.route === 'preview'),
          ...(state.preview
            ? [
                ui.text(
                  'preview-count',
                  `${text.importsRecords}: ${state.preview.records} · ${state.preview.bytes} B`,
                  props.theme.muted,
                ),
              ]
            : []),
          ...(state.preview?.incompleteTail
            ? [ui.text('preview-tail', text.importsIncomplete, props.theme.warning)]
            : []),
          ...(state.destination
            ? [ui.text('destination-path', state.destination.path, props.theme.muted)]
            : []),
        ],
        { visible: state.route === 'preview', gap: 0 },
      ),
      form,
      ui.box(
        'import-list-container',
        [
          presentation.reading(
            'import-list',
            { command: state.focus, content: contents },
            { input: 'list-input', observed: 'list-observed' },
          ),
        ],
        { visible: state.route !== 'form', terminal: { grow: 1, basis: 0 } },
      ),
      ...(state.route === 'sources' && chosenSource
        ? [
            ui.box('selected-source-actions', sourceActions(chosenSource, 'footer-'), {
              row: true,
              terminal: { wrap: true },
            }),
          ]
        : []),
      ui.box(
        'imports-actions',
        state.route === 'sources'
          ? [
              ui.button('add', text.importsAdd, disabled),
              ui.button('refresh', text.directoryRefresh, disabled),
              ui.button('copies', text.importsHistory, disabled),
            ]
          : state.route === 'form'
            ? [
                ui.button(
                  'save-source',
                  text.settingsSave,
                  editing ||
                    !state.form?.directory ||
                    !state.fields.name.ready ||
                    !!state.fields.name.command ||
                    (state.form?.format === 'open_code' &&
                      (!state.fields.database.ready || !!state.fields.database.command)),
                ),
              ]
            : state.route === 'catalog'
              ? [ui.button('refresh-catalog', text.directoryRefresh, disabled)]
              : state.route === 'preview'
                ? [
                    ui.button('destination-directory', text.importsChooseDestination, editing),
                    ui.button(
                      'review-copy',
                      text.importsReview,
                      editing ||
                        !state.destination ||
                        !state.preview ||
                        !state.fields.title.ready ||
                        !!state.fields.title.command,
                    ),
                    ui.button('preview-first', text.directoryRefresh, disabled),
                  ]
                : [ui.button('refresh-copies', text.directoryRefresh, disabled)],
        { row: true, terminal: { wrap: true }, gap: 1 },
      ),
    ],
    {
      visible: props.open,
      autoFocus: true,
      shortcuts: [{ key: 'escape', modifiers: [], input: 'back', enabled: true }],
      gap: 1,
      terminal: { grow: 1 },
    },
  );
  let view = body;
  for (const purpose of ['source', 'destination'] as const) {
    const picker = presentation.slot(
      `import-${purpose}-picker`,
      'directories',
      {
        theme: props.theme,
        locale: props.locale,
        purpose: purpose === 'source' ? 'import' : 'workspace',
        chooseLabel: text.projectsUseDirectory,
        open: props.open && state.picker === purpose,
        busy: disabled,
      },
      { selected: purpose + '-selected', closed: purpose + '-closed' },
    );
    view = ui.dialog(
      'import-' + purpose + '-dialog',
      purpose === 'source' ? text.importsChooseSource : text.importsDestination,
      view,
      picker,
      state.picker === purpose,
      { width: 76, height: 24 },
    );
  }
  const confirmation = ui.box(
    'import-confirmation',
    [
      ...(state.confirm?.kind === 'copy'
        ? [
            ui.text('confirm-title', state.confirm.title, props.theme.text, 600),
            ui.text('confirm-directory', compactLabel(state.destination?.path ?? '', 240)),
            ui.text('confirm-records', `${text.importsRecords}: ${state.preview?.records ?? ''}`),
            ui.text('confirm-help', text.importsHelp, props.theme.muted),
          ]
        : [ui.text('confirm-remove', text.importsRemoveHelp)]),
      ...(state.notice ? [ui.text('confirm-notice', text[state.notice], props.theme.muted)] : []),
      ...(state.current
        ? [
            ui.button(
              'confirm-receipt',
              text.executionReceipt,
              !!state.pending || !state.current.reference,
            ),
          ]
        : []),
      ui.box(
        'confirm-actions',
        [
          ui.button('confirm-cancel', text.cancel, !!state.current || !!state.pending),
          ui.button(
            'confirm',
            state.confirm?.kind === 'copy' ? text.importsConfirm : text.importsRemove,
            disabled,
            true,
          ),
        ],
        { row: true },
      ),
    ],
    {
      gap: 1,
      shortcuts: [
        {
          key: 'escape',
          modifiers: [],
          input: 'confirm-cancel',
          enabled: !state.current && !state.pending,
        },
      ],
    },
  );
  view = ui.dialog(
    'import-confirm-dialog',
    state.confirm?.kind === 'copy' ? text.importsReview : text.confirmDefault,
    view,
    confirmation,
    !!state.confirm,
    { width: 68 },
  );
  const result = output(
    state,
    ui.box('imports-root', [view], { visible: props.open, terminal: { grow: 1 } }),
    events,
  );
  result.actions = actions;
  return result;
}
