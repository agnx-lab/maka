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
  EditorObservation,
  TextareaProps,
  FileObservation,
  PackageInstalled,
} from '../../packages/plugin-sdk/src/index.js';
import type { ConnectionsProps } from './contracts.js';
import { copy } from './locale-catalogs/deployment-import.js';
import { sameValue } from './value.js';
import { type Presentation, type Node } from './view.js';

export type InstallIntent =
  | { source: DeploymentRef; file: FileObservation }
  | { source: DeploymentRef; url: string; captured: EditorObservation };
export interface PackageSource {
  mode: 'file' | 'url';
  urlObservation: EditorObservation | null;
  observation: FileObservation | null;
}
export interface PackageImport extends PackageSource {
  open: boolean;
  confirmation: InstallIntent | null;
  installed: PackageInstalled | null;
}
export function packageImport(): PackageImport {
  return {
    open: false,
    mode: 'file',
    urlObservation: null,
    observation: null,
    confirmation: null,
    installed: null,
  };
}
export function canInstall(importer: PackageSource, busy: boolean, conflict: boolean) {
  if (busy || conflict) return false;
  if (importer.mode === 'file')
    return importer.observation?.phase === 'ready' && !!importer.observation.file;
  const snapshot = importer.urlObservation?.snapshot;
  return !!snapshot && !snapshot.marked && snapshot.text.bytes > 0 && snapshot.text.bytes <= 8192;
}
export function matchesImport(importer: PackageImport) {
  const intent = importer.confirmation;
  return (
    !!intent &&
    ('file' in intent
      ? importer.mode === 'file' && sameValue(intent.file, importer.observation)
      : importer.mode === 'url' && sameValue(intent.captured, importer.urlObservation))
  );
}
export function packageSourceView<N extends Node>(
  presentation: Presentation<N>,
  props: ConnectionsProps,
  prefix: string,
  importer: PackageSource,
  busy: boolean,
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  return ui.box(
    prefix + '-source-body',
    [
      ui.box(
        prefix + '-source',
        [
          ui.button(prefix + '-local', text.deploymentLocal, busy, importer.mode === 'file'),
          ui.button(prefix + '-remote', text.deploymentUrl, busy, importer.mode === 'url'),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
      ui.box(
        prefix + '-url-owner',
        [
          presentation.slot(
            prefix + '-url',
            'editor',
            {
              appearance: props.theme.textarea,
              accessibilityLabel: text.deploymentUrl,
              placeholder: 'https://example.com/plugin.maka-plugin',
              tabIndex: 0,
              readOnly: busy,
              disabled: false,
              autoFocus: true,
              softWrap: true,
              terminalRows: { min: 1, max: 2 },
              submit: 'enter',
              command: null,
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
            } satisfies TextareaProps,
            { changed: prefix + '-url-changed', submitted: prefix + '-install' },
          ),
        ],
        { visible: importer.mode === 'url' },
      ),
      // The retained native child owns selection. Confirmation suspends it rather
      // than recreating it; snapshot still captures the exact observed revision.
      ui.box(
        prefix + '-file-owner',
        [
          presentation.slot(
            prefix + '-file',
            'file',
            {
              appearance: props.theme.input,
              label: text.deploymentChooseFile,
              pathLabel: text.deploymentFilePath,
              browseLabel: text.fileBrowse,
              parentLabel: text.directoryParent,
              emptyLabel: text.fileListEmpty,
              truncatedLabel: text.fileListTruncated,
              placeholder: text.deploymentFilePlaceholder,
              disabled: busy,
              tabIndex: 0,
            },
            { changed: prefix + '-changed' },
          ),
        ],
        { visible: importer.mode === 'file', terminal: { grow: 1 } },
      ),
    ],
    { gap: 1, terminal: { grow: importer.mode === 'file' && !importer.observation?.file ? 1 : 0 } },
  );
}

export function importView<N extends Node>(
  presentation: Presentation<N>,
  props: ConnectionsProps,
  importer: PackageImport,
  busy: boolean,
  conflict: boolean,
  notice: string | null,
  recoverable: boolean,
  pending: boolean,
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale),
    observed = importer.observation;
  const confirmation = importer.confirmation;
  const file =
    importer.mode === 'file'
      ? confirmation && 'file' in confirmation
        ? confirmation.file.file
        : observed?.file
      : undefined;
  const error = observed?.error;
  const failure =
    error === 'limit_exceeded'
      ? text.deploymentFileTooLarge
      : error === 'budget_exhausted'
        ? text.deploymentFileBusy
        : text.deploymentFileFailed;
  const panel = ui.box(
    'deployment-import-panel',
    [
      ui.box(
        'deployment-import-header',
        [
          ui.box(
            'deployment-import-heading',
            [
              ui.button('deployment-import-back', text.back, false, false, {
                background: 0,
                align: 'start',
              }),
              ui.text('deployment-import-title', text.deploymentInstall, props.theme.text, 600),
            ],
            {
              visible: !props.shell,
              row: true,
              gap: 1,
              terminal: { wrap: true },
            },
          ),
          ui.text('deployment-import-help', text.deploymentInstallHelp, props.theme.muted),
        ],
        { gap: 1 },
      ),
      ui.box(
        'deployment-import-body',
        [
          packageSourceView(
            presentation,
            props,
            'deployment-import',
            importer,
            busy || !!confirmation,
          ),
          ...(notice ? [ui.text('deployment-import-notice', notice, props.theme.muted)] : []),
          ...(file
            ? [
                ui.text('deployment-import-name', file.name, props.theme.text, 550),
                ui.text('deployment-import-size', String(file.bytes) + ' B', props.theme.muted),
              ]
            : []),
          ...(observed?.phase === 'choosing'
            ? [ui.text('deployment-import-loading', text.deploymentFileLoading, props.theme.muted)]
            : []),
          ...(observed?.phase === 'failed'
            ? [ui.text('deployment-import-failed', failure, props.theme.muted)]
            : []),
          ...(importer.installed
            ? [
                ui.text(
                  'deployment-import-package',
                  importer.installed.package.id,
                  props.theme.text,
                  550,
                ),
                ui.text(
                  'deployment-import-package-revision',
                  importer.installed.package.revision,
                  props.theme.muted,
                ),
                ui.text(
                  'deployment-import-activation-help',
                  text.deploymentActivateHelp,
                  props.theme.muted,
                ),
              ]
            : []),
        ],
        {
          scroll: undefined,
          terminal: { grow: importer.mode === 'file' && !importer.observation?.file ? 1 : 0 },
        },
      ),
      ui.box(
        'deployment-import-footer',
        [
          ui.button(
            'deployment-import-install',
            text.deploymentInstall,
            !canInstall(importer, busy, conflict),
            true,
          ),
          ...(conflict
            ? [
                ui.button(
                  'deployment-import-refresh',
                  props.theme.ascii ? 'r' : '↻',
                  busy,
                  false,
                  { background: 0 },
                  undefined,
                  text.deploymentRefresh,
                ),
              ]
            : []),
          ...(recoverable ? [ui.button('deployment-import-check', text.check, pending)] : []),
          ui.button('deployment-import-history', text.deploymentHistory, pending),
          ...(importer.installed
            ? [ui.button('deployment-import-advanced', text.deploymentEdit, busy)]
            : []),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    {
      visible: importer.open,
      autoFocus: true,
      shortcuts: [
        {
          key: 'escape',
          modifiers: [],
          input: 'deployment-import-back',
          enabled: true,
        },
      ],
      terminal: { grow: 1 },
      gap: 1,
    },
  );

  return ui.box(
    'deployment-import-page',
    [
      ui.dialog(
        'deployment-import-dialog',
        text.deploymentInstall,
        panel,
        ui.box(
          'deployment-import-dialog-content',
          [
            ui.box(
              'deployment-import-dialog-body',
              [
                ui.text(
                  'deployment-import-question',
                  text.deploymentInstallQuestion,
                  props.theme.text,
                  550,
                ),
                ui.text(
                  'deployment-import-confirm-help',
                  text.deploymentInstallHelp,
                  props.theme.muted,
                ),
                ...(confirmation && 'url' in confirmation
                  ? [ui.text('deployment-import-confirm-url', confirmation.url, props.theme.text)]
                  : []),
                ...(file
                  ? [
                      ui.text('deployment-import-confirm-name', file.name, props.theme.text, 550),
                      ui.text(
                        'deployment-import-confirm-size',
                        String(file.bytes) + ' B',
                        props.theme.muted,
                      ),
                    ]
                  : []),
              ],
              { scroll: 'manual', terminal: { grow: 1 }, gap: 1 },
            ),
            ui.box(
              'deployment-import-dialog-actions',
              [
                ui.button('deployment-import-cancel', text.cancel, busy),
                ui.button(
                  'deployment-import-confirm',
                  text.confirmDefault,
                  busy || conflict || !confirmation || !matchesImport(importer),
                  true,
                ),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
          ],
          { terminal: { grow: 1 } },
        ),
        !!confirmation,
        { width: 64, height: 14 },
      ),
    ],
    { visible: importer.open, gap: 0, terminal: { grow: 1 } },
  );
}
