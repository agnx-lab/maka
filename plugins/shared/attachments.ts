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
  AgentAttachment,
  AgentDataSource,
  FileCapture,
  FileObservation,
  FilePasteCommand,
  FilePasteObservation,
  ReadingCommand,
} from '../../packages/plugin-sdk/src/index.js';
import { sameValue } from './value.js';
import type { Theme } from './contracts.js';
import { copy, tuiText } from './locale-catalogs/attachments.js';
import { type Node, type Presentation } from './view.js';
export type DraftFilePaste = FilePasteObservation & { dataSource: AgentDataSource };

export interface AttachmentDrafts {
  files: (AgentAttachment & { dataSource: AgentDataSource })[];
  selection: {
    attempt: number;
    sessionId: string;
    dataSource: AgentDataSource;
    file: FileObservation | null;
    browse: boolean;
    selected: string | null;
    focus: ReadingCommand | null;
  } | null;
  paste: {
    observed: DraftFilePaste;
    attempted: FileCapture | null;
    command: FilePasteCommand | null;
  } | null;
}
export const selectedFile = (file: FileObservation): FileCapture => ({
  owner: file.owner,
  revision: file.revision,
});
export const sameFile = (a: FileCapture | null, b: FileCapture) =>
  a?.owner === b.owner && a.revision === b.revision;
export function pastedFiles(drafts: AttachmentDrafts, observed: DraftFilePaste) {
  const prior = drafts.paste;
  if (
    prior?.observed.selection.owner === observed.selection.owner &&
    !sameValue(prior.observed.dataSource, observed.dataSource)
  )
    return;
  if (
    prior &&
    prior.observed.selection.owner === observed.selection.owner &&
    prior.observed.selection.revision >= observed.selection.revision
  )
    return;
  if (observed.remaining === 0) {
    if (!prior || prior.observed.selection.owner === observed.selection.owner) drafts.paste = null;
    return;
  }
  drafts.paste = {
    observed,
    attempted: prior?.attempted ?? null,
    command: null,
  };
}
export function pasteView<N extends Node>(
  presentation: Presentation<N>,
  theme: Theme,
  locale: string,
  paste: NonNullable<AttachmentDrafts['paste']>,
  busy: boolean,
) {
  const ui = presentation.paint(theme),
    text = copy(locale),
    file = paste.observed.selection;
  const failed = file.phase === 'failed';
  const stopped = failed || (file.phase === 'ready' && !busy && !paste.command);
  const retryable = (failed && paste.observed.retryable) || (stopped && !failed);
  const status =
    file.error === 'paste_rejected'
      ? text.clipboardPasteRejected
      : file.error === 'clipboard_unavailable'
        ? text.clipboardUnavailable
        : stopped
          ? text.attachmentFailed
          : text.attachmentPasting;
  return ui.box(
    'file-paste',
    [
      ui.text(
        'file-paste-status',
        `${status} (${paste.observed.remaining})${file.file ? ` · ${file.file.name}` : ''}`,
        theme.muted,
        400,
        { truncate: true },
      ),
      ...(retryable
        ? [ui.button('file-paste-retry', text.sourceRetry, busy || !!paste.command)]
        : []),
      ...(['ready', 'failed'].includes(file.phase)
        ? [ui.button('file-paste-skip', text.attachmentSkip, busy || !!paste.command)]
        : []),
      ui.button('file-paste-cancel', text.cancel, busy || !!paste.command),
    ],
    { row: true, terminal: { wrap: true } },
  );
}
export const attachmentFiles = (
  drafts: AttachmentDrafts,
  sessionId: string | undefined,
  dataSource: AgentDataSource | undefined,
) =>
  drafts.files.filter(
    (file) => file.sessionId === sessionId && sameValue(file.dataSource, dataSource),
  );
export const attachmentChild = (drafts: AttachmentDrafts) =>
  'attachment-file-' + drafts.selection?.attempt;
export function attachmentView<N extends Node>(
  presentation: Presentation<N>,
  theme: Theme,
  locale: string,
  drafts: AttachmentDrafts,
  busy: boolean,
  enabled: boolean,
  notice: string | null,
  recoverable: boolean,
  checking: boolean,
) {
  const ui = presentation.paint(theme),
    text = copy(locale),
    selection = drafts.selection!;
  const observed = selection.file;
  const files = attachmentFiles(drafts, selection.sessionId, selection.dataSource);
  const fileContent = [
    presentation.slot(
      attachmentChild(drafts),
      'file',
      {
        appearance: theme.input,
        label: text.attachmentChoose,
        pathLabel: text.attachmentPath,
        browseLabel: text.fileBrowse,
        parentLabel: text.directoryParent,
        emptyLabel: text.fileListEmpty,
        truncatedLabel: text.fileListTruncated,
        placeholder: text.attachmentPathPlaceholder,
        disabled: busy || !enabled,
        tabIndex: 0,
      },
      { changed: 'attachment-file-changed' },
    ),
    ...(observed?.phase === 'choosing'
      ? [ui.text('attachment-reading', text.deploymentFileLoading, theme.muted)]
      : []),
    ...(observed?.phase === 'ready' && observed.file
      ? [
          ui.text(
            'attachment-selected',
            `${observed.file.name} · ${observed.file.bytes} B`,
            theme.text,
          ),
        ]
      : []),
    ...(observed?.phase === 'failed'
      ? [
          ui.text(
            'attachment-file-error',
            observed.error === 'limit_exceeded' ? text.attachmentTooLarge : text.attachmentFailed,
            theme.muted,
          ),
        ]
      : []),
    ...(notice ? [ui.text('attachment-notice', notice, theme.muted)] : []),
  ];
  {
    const selected = files.find((file) => file.id === selection.selected);
    const rows = files.map((file, index) =>
      ui.item!(
        'attachment-select-' + index,
        ui.box(
          'attachment-summary-' + index,
          [
            ui.text(
              'attachment-name-' + index,
              `${file.id === selection.selected ? (theme.ascii ? '>' : '›') : ' '} ${file.name}`,
              theme.text,
              400,
              { truncate: true },
            ),
            ui.text(
              'attachment-status-' + index,
              `  ${tuiText(locale, 'attachments-ready')} · ${attachmentSize(file.bytes)}`,
              theme.muted,
              400,
              { truncate: true },
            ),
          ],
          { gap: 0 },
        ),
        file.id === selection.selected,
        busy,
        { followFocus: true, submit: true },
      ),
    );
    rows.push(
      ui.item!(
        'attachment-browse',
        ui.text(
          'attachment-browse-label',
          '+ ' + tuiText(locale, 'attachments-add'),
          theme.accent,
          400,
          { truncate: true },
        ),
        false,
        busy || !enabled,
        { followFocus: false, submit: true },
      ),
    );
    return ui.box(
      'attachment-panel',
      [
        ui.box(
          'attachment-list-view',
          [
            presentation.reading(
              'attachment-list',
              {
                command: selection.focus,
                content: ui.box('attachment-rows', rows, {
                  scroll: 'manual',
                  focusGroup: true,
                  gap: 1,
                  terminal: { grow: 1 },
                }),
              },
              { input: 'attachment-input' },
            ),
            ...(notice ? [ui.text('attachment-list-notice', notice, theme.muted)] : []),
            ui.box(
              'attachment-list-controls',
              [
                ui.button('attachment-close', text.closeModels),
                ...(selected
                  ? [
                      ui.button(
                        'attachment-remove-' + selected.id,
                        tuiText(locale, 'attachments-remove'),
                        busy,
                      ),
                    ]
                  : []),
                ...(recoverable ? [ui.button('attachment-check', text.check, checking)] : []),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
          ],
          { visible: !selection.browse, autoFocus: true, terminal: { grow: 1 } },
        ),
        ui.box(
          'attachment-browser-view',
          [
            ui.box('attachment-browser-body', fileContent, { gap: 0, terminal: { grow: 1 } }),
            ui.box(
              'attachment-browser-controls',
              [
                ui.button(
                  files.length ? 'attachment-list-back' : 'attachment-browser-close',
                  files.length ? text.back : text.closeModels,
                ),
                ...(recoverable
                  ? [ui.button('attachment-browser-check', text.check, checking)]
                  : []),
                ui.button(
                  'attachment-upload',
                  text.attachmentAdd,
                  busy || !enabled || observed?.phase !== 'ready' || !observed.file,
                  true,
                ),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
          ],
          { visible: selection.browse, autoFocus: true, terminal: { grow: 1 } },
        ),
      ],
      { gap: 0, terminal: { grow: 1 } },
    );
  }
}

function attachmentSize(bytes: number) {
  return bytes < 1024
    ? `${bytes} B`
    : bytes < 1024 * 1024
      ? `${(bytes / 1024).toFixed(1)} KiB`
      : `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

export function attachmentSummary<N extends Node>(
  presentation: Presentation<N>,
  theme: Theme,
  locale: string,
  files: AgentAttachment[],
  disabled: boolean,
) {
  const ui = presentation.paint(theme);
  return ui.item!(
    'attachments-summary',
    ui.box(
      'attachment-summary',
      [
        ui.text('attachment-summary-icon', `${theme.ascii ? '+' : '✓'} `, theme.accent),
        ui.box(
          'attachment-summary-name-region',
          [
            ui.text('attachment-summary-name', files[0]!.name, theme.muted, 400, {
              truncate: true,
            }),
          ],
          { gap: 0, terminal: { grow: 1, basis: 0 } },
        ),
        ui.text(
          'attachment-summary-status',
          `${files.length > 1 ? '  +' + (files.length - 1) : ''} · ${tuiText(locale, 'attachments-ready')}`,
          theme.muted,
          400,
          { truncate: true },
        ),
      ],
      { row: true, gap: 0 },
    ),
    false,
    disabled,
    { followFocus: false, submit: true },
  );
}
