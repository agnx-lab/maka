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
  EditorObservation,
  EditorTextRef,
  FileObservation,
} from '../../packages/plugin-sdk/src/index.js';
import type { SkillLibraryProps } from './contracts.js';
import { copy } from './locale-catalogs/skill-import.js';
import { sameValue } from './value.js';
import { type Node, type Presentation } from './view.js';

export interface SourceImport {
  open: boolean;
  file: FileObservation | null;
  id: EditorObservation | null;
  confirmation: { file: FileObservation; id: EditorTextRef } | null;
}
export function sourceImport(): SourceImport {
  return { open: false, file: null, id: null, confirmation: null };
}
export function importReady(value: SourceImport) {
  return (
    value.file?.phase === 'ready' &&
    !!value.file.file &&
    value.file.file.bytes > 0 &&
    value.file.file.bytes <= 1024 * 1024 &&
    !!value.id &&
    !value.id.snapshot.marked &&
    value.id.snapshot.text.bytes > 0 &&
    value.id.snapshot.text.bytes <= 128
  );
}
export function importConfirmed(value: SourceImport) {
  return (
    importReady(value) &&
    !!value.confirmation &&
    sameValue(value.file, value.confirmation.file) &&
    sameValue(value.id?.snapshot.text, value.confirmation.id)
  );
}
export function importView<N extends Node>(
  presentation: Presentation<N>,
  props: SkillLibraryProps,
  value: SourceImport,
  busy: boolean,
  current: boolean,
  recoverable: boolean,
  notice: string | null,
  cleanup: boolean,
): N {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const disabled = busy || current || !!value.confirmation;
  return ui.box(
    'skills-import-panel',
    [
      ui.text('skills-import-heading', text.skillsImport, props.theme.text, 550),
      ui.text('skills-import-help', text.skillsImportHelp, props.theme.muted),
      ui.text('skills-import-id-label', text.skillsSourceId),
      presentation.slot(
        'skills-import-id',
        'input',
        {
          appearance: props.theme.input,
          accessibilityLabel: text.skillsSourceId,
          placeholder: 'my-skill',
          autoFocus: true,
          readOnly: false,
          disabled,
          tabIndex: 0,
        },
        { changed: 'source-id' },
      ),
      ui.text('skills-import-id-help', text.skillsSourceIdHelp, props.theme.muted),
      presentation.slot(
        'skills-import-file',
        'file',
        {
          appearance: props.theme.input,
          label: text.skillsChooseFile,
          pathLabel: text.skillsFilePath,
          browseLabel: text.fileBrowse,
          parentLabel: text.directoryParent,
          emptyLabel: text.fileListEmpty,
          truncatedLabel: text.fileListTruncated,
          placeholder: 'SKILL.md',
          disabled,
          tabIndex: 1,
        },
        { changed: 'source-file' },
      ),
      ...(value.file?.phase === 'failed' || (value.file?.file?.bytes ?? 0) > 1024 * 1024
        ? [ui.text('skills-import-file-error', text.skillsImportInvalid, props.theme.error)]
        : []),
      ...(notice ? [ui.text('skills-import-notice', notice, props.theme.muted)] : []),
      ...(cleanup
        ? [ui.text('skills-import-cleanup', text.skillsCleanupUnknown, props.theme.muted)]
        : []),
      ...(value.confirmation
        ? [ui.text('skills-import-confirmation', text.skillsImportConfirm, props.theme.text, 550)]
        : []),
      ui.box(
        'skills-import-controls',
        [
          ...(current
            ? [ui.button('check-import', text.skillsCheckResult, busy || !recoverable)]
            : value.confirmation
              ? [
                  ui.button(
                    'confirm-import',
                    text.skillsConfirm,
                    busy || !importConfirmed(value),
                    true,
                  ),
                  ui.button('cancel-import-confirmation', text.cancel, busy),
                ]
              : [ui.button('review-import', text.skillsImport, busy || !importReady(value), true)]),
          ui.button('back-import', text.back, busy || current),
          ui.button('close-import', text.closeModels, false),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    { visible: value.open, gap: 0, terminal: { grow: 1 } },
  );
}
