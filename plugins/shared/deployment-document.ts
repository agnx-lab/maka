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
  DeploymentDocumentChunk,
  DeploymentRef,
  EditorCommand,
  EditorObservation,
} from '../../packages/plugin-sdk/src/index.js';
import type { ConnectionsProps } from './contracts.js';
import { copy } from './locale-catalogs/deployment-document.js';
import { sameValue } from './value.js';
import { type Presentation, type Node } from './view.js';

export interface DocumentEditor {
  source: DeploymentRef;
  phase: 'loading' | 'ready' | 'failed';
  offset: number;
  total: number | null;
  chunk: DeploymentDocumentChunk | null;
  observation: EditorObservation | null;
  command: EditorCommand | null;
  sequence: number;
  confirmation: EditorObservation | null;
  reloadConfirmation: boolean;
  conflict: boolean;
}
export function documentEditor(source: DeploymentRef): DocumentEditor {
  return {
    source,
    phase: 'loading',
    offset: 0,
    total: null,
    chunk: null,
    observation: null,
    command: null,
    sequence: 0,
    confirmation: null,
    reloadConfirmation: false,
    conflict: false,
  };
}
function command(editor: DocumentEditor) {
  const chunk = editor.chunk,
    observed = editor.observation;
  if (!chunk || !observed || editor.command || editor.phase !== 'loading') return;
  if (
    observed.snapshot.marked ||
    (chunk.offset > 0 && observed.snapshot.text.bytes !== chunk.offset)
  ) {
    editor.phase = 'failed';
    editor.chunk = null;
    return;
  }
  editor.command = {
    owner: observed.snapshot.text.owner,
    sequence: ++editor.sequence,
    basedOn: observed.revision,
    edit: { kind: chunk.offset === 0 ? 'reset' : 'append', text: chunk.text },
  };
}
export function acceptDocumentChunk(editor: DocumentEditor, chunk: DeploymentDocumentChunk) {
  const bytes = new TextEncoder().encode(chunk.text).length,
    end = chunk.offset + bytes;
  if (
    !sameValue(chunk.source, editor.source) ||
    chunk.offset !== editor.offset ||
    /[\uD800-\uDFFF]/u.test(chunk.text) ||
    bytes === 0 ||
    bytes > 65536 ||
    end > chunk.totalBytes ||
    (editor.total !== null && editor.total !== chunk.totalBytes) ||
    chunk.next !== (end === chunk.totalBytes ? null : end)
  ) {
    editor.phase = 'failed';
    return;
  }
  editor.total = chunk.totalBytes;
  editor.chunk = chunk;
  command(editor);
}
export function observeDocument(
  editor: DocumentEditor,
  observation: EditorObservation,
): number | null {
  editor.observation = observation;
  const pending = editor.command,
    chunk = editor.chunk,
    receipt = observation.receipt;
  if (pending && receipt?.owner === pending.owner && receipt.sequence === pending.sequence) {
    editor.command = null;
    editor.chunk = null;
    if (
      receipt.outcome.status !== 'applied' ||
      !chunk ||
      observation.snapshot.text.owner !== pending.owner ||
      observation.snapshot.text.bytes !== chunk.offset + new TextEncoder().encode(chunk.text).length
    ) {
      editor.phase = 'failed';
      return null;
    }
    if (chunk.next == null) editor.phase = 'ready';
    else {
      editor.offset = chunk.next;
      return chunk.next;
    }
  } else command(editor);
  return null;
}
export function canSaveDocument(editor: DocumentEditor, busy: boolean) {
  return (
    editor.phase === 'ready' &&
    !editor.conflict &&
    !busy &&
    !!editor.observation &&
    !editor.observation.snapshot.marked &&
    editor.observation.snapshot.text.bytes > 0
  );
}
export function documentView<N extends Node>(
  presentation: Presentation<N>,
  props: ConnectionsProps,
  editor: DocumentEditor,
  open: boolean,
  busy: boolean,
  notice: string | null,
  diagnostic: string | null,
  recoverable: boolean,
  pending: boolean,
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const confirming = !!editor.confirmation || editor.reloadConfirmation;
  const question = [
    ui.text(
      'deployment-document-confirm-title',
      editor.reloadConfirmation ? text.deploymentReloadQuestion : text.deploymentSaveQuestion,
      props.theme.text,
      550,
    ),
    ui.text(
      'deployment-document-confirm-help',
      editor.reloadConfirmation ? text.deploymentReloadHelp : text.deploymentSaveHelp,
      props.theme.muted,
    ),
  ];
  const panel = ui.box(
    'deployment-document-panel',
    [
      ui.box(
        'deployment-document-header',
        [
          ui.box(
            'deployment-document-heading',
            [
              ui.button('deployment-document-back', text.back),
              ui.text('deployment-document-title', text.deploymentEdit, props.theme.text, 600),
            ],
            {
              visible: !props.shell,
              row: true,
              gap: 1,
              terminal: { wrap: true },
            },
          ),
          ui.text('deployment-document-help', text.deploymentEditHelp, props.theme.muted),
          ...(editor.phase === 'loading'
            ? [ui.text('deployment-document-loading', text.deploymentLoading, props.theme.muted)]
            : []),
          ...(editor.phase === 'failed'
            ? [ui.text('deployment-document-failed', text.deploymentLoadFailed, props.theme.muted)]
            : []),
          ...(editor.conflict && notice !== text.deploymentConflict
            ? [ui.text('deployment-document-conflict', text.deploymentConflict, props.theme.muted)]
            : []),
          ...(notice ? [ui.text('deployment-document-notice', notice, props.theme.muted)] : []),
        ],
        { gap: 1 },
      ),
      ...(diagnostic
        ? [
            ui.box(
              'deployment-document-diagnostic-region',
              [ui.text('deployment-document-diagnostic', diagnostic, props.theme.text)],
              { scroll: 'manual', terminal: { basis: 3 } },
            ),
          ]
        : []),
      ui.box(
        'deployment-document-owner',
        [
          presentation.slot(
            'deployment-document-editor',
            'editor',
            {
              appearance: {
                ...props.theme.textarea,
                text: {
                  ...props.theme.textarea.text,
                  font: props.theme.markdown.code.font,
                },
              },
              menu: {
                appearance: props.theme.menu,
                labels: {
                  title: text.edit,
                  undo: text.undo,
                  redo: text.redo,
                  cut: text.cut,
                  copy: text.copy,
                  paste: text.paste,
                  selectAll: text.selectAll,
                },
              },
              accessibilityLabel: text.deploymentDocument,
              readOnly: busy || confirming || editor.phase !== 'ready',
              disabled: false,
              softWrap: true,
              tabIndex: 0,
              initialText: '',
              placeholder: '',
              command: editor.command,
            },
            { changed: 'deployment-document-changed' },
          ),
        ],
        { visible: true, terminal: { grow: 1 } },
      ),
      ui.box(
        'deployment-document-footer',
        [
          ui.button(
            'deployment-document-save',
            text.deploymentSave,
            !canSaveDocument(editor, busy),
          ),
          ui.button(
            'deployment-document-reload',
            text.deploymentReload,
            busy || editor.phase === 'loading',
          ),
          ...(recoverable ? [ui.button('deployment-document-check', text.check, pending)] : []),
          ui.button('deployment-document-history', text.deploymentHistory, pending),
        ],
        { row: true, gap: 0, terminal: { wrap: true } },
      ),
    ],
    {
      visible: open,
      autoFocus: true,
      shortcuts: [
        {
          key: 'escape',
          modifiers: [],
          input: 'deployment-document-back',
          enabled: true,
        },
      ],
      gap: 1,
      terminal: { grow: 1 },
    },
  );

  return ui.box(
    'deployment-document-page',
    [
      ui.dialog(
        'deployment-document-dialog',
        text.deploymentEdit,
        panel,
        ui.box(
          'deployment-document-dialog-content',
          [
            ui.box('deployment-document-dialog-body', question, {
              scroll: 'manual',
              terminal: { grow: 1 },
              gap: 1,
            }),
            ui.box(
              'deployment-document-dialog-actions',
              [
                ui.button('deployment-document-cancel', text.cancel, busy),
                ui.button(
                  editor.reloadConfirmation
                    ? 'deployment-document-reload-confirm'
                    : 'deployment-document-confirm',
                  text.confirmDefault,
                  busy,
                  true,
                ),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
          ],
          { terminal: { grow: 1 } },
        ),
        confirming,
        { width: 64, height: 12 },
      ),
    ],
    { visible: open, gap: 0, terminal: { grow: 1 } },
  );
}
