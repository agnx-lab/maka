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
  AgentWorkspace,
  ActionOutcome,
  Component,
  ComponentInput,
  EditorObservation,
  FileObservation,
  EditorTextRef,
  Emission,
  ReadingCommand,
  SkillLibraryCursor,
  SkillLibraryChangeKind,
  SkillLibraryPreview,
  SkillLibraryChangeResult,
  SkillLibraryPreferenceResult,
  SkillLibraryOutcome,
  OperationReply,
  OperationCleanup,
  RecoveryRef,
  SkillLibraryDocument,
  SkillLibraryPage,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import { skillLibraryContract, type SkillLibraryProps } from '../../shared/contracts.js';
import {
  sourceImport,
  importReady,
  importConfirmed,
  importView,
  type SourceImport,
} from '../../shared/skill-import.js';
import { scrollBoundary } from '../../shared/scroll.js';
import { sameValue } from '../../shared/value.js';
import { copy, type Copy } from './locale.js';
import { compactLabel, output, type Presentation, type Node } from '../../shared/view.js';

type Page = Extract<SkillLibraryPage, { kind: 'page' }>;
type Document = Extract<SkillLibraryDocument, { kind: 'page' }>;
type Request =
  | { kind: 'query'; key: string; basis: EditorTextRef }
  | {
      kind: 'list';
      key: string;
      cursor: SkillLibraryCursor | null;
      history: number[];
      landing?: 'start' | 'end';
    }
  | { kind: 'read'; key: string; landing?: 'start' | 'end' }
  | { kind: 'preview' | 'mutate' | 'receipt' | 'outcome'; key: string };
interface State {
  importer: SourceImport;
  workspace: AgentWorkspace | null;
  revision: number;
  serial: number;
  open: boolean;
  observation: EditorObservation | null;
  basis: EditorTextRef | null;
  query: string;
  page: Page | null;
  pageKey: string;
  cursor: number;
  history: number[];
  document: Document | null;
  pending: Request | null;
  notice: keyof Copy | null;
  cleanupUnconfirmed: boolean;
  preview: Extract<SkillLibraryPreview, { kind: 'ready' }> | null;
  current: {
    method: 'apply' | 'setPreference' | 'importSource' | 'deleteSource';
    reference: RecoveryRef | null;
  } | null;
  active: number;
  reveal: ReadingCommand | null;
}
function selectWorkspace(state: State, props: SkillLibraryProps) {
  if (state.current || sameValue(state.workspace, props.workspace ?? null)) return false;
  state.workspace = props.workspace ?? null;
  state.page = null;
  state.document = null;
  state.preview = null;
  state.pending = null;
  state.cursor = 0;
  state.history = [];
  state.active = 0;
  state.notice = null;
  return true;
}
function same(a: EditorTextRef | null, b: EditorTextRef | null) {
  return a?.owner === b?.owner && a?.revision === b?.revision;
}
function dirty(state: State) {
  return !!state.observation && !same(state.basis, state.observation.snapshot.text);
}
function readable(state: State) {
  return state.notice !== 'skillsChanged' && state.notice !== 'searchTooLong' && !dirty(state);
}
function list(
  state: State,
  cursor: SkillLibraryCursor | null = null,
  history: number[] = [],
): Action[] {
  const key = `skills-${++state.serial}`;
  state.pending = { kind: 'list', key, cursor, history };
  state.notice = null;
  return [
    {
      key,
      binding: 'skills',
      member: 0,
      method: 'list',
      input: { query: state.query, cursor, workspace: state.workspace },
      resources: {},
    },
  ];
}
function search(state: State): Action[] {
  const basis = state.observation?.snapshot.text;
  if (!basis) return list(state);
  if (basis.bytes > 1024) {
    state.notice = 'searchTooLong';
    return [];
  }
  const key = `skills-${++state.serial}`;
  state.pending = { kind: 'query', key, basis };
  state.notice = null;
  return [
    {
      key,
      binding: 'reader',
      member: 0,
      method: 'read',
      input: { start: 0, end: basis.bytes },
      resources: {
        text: {
          child: 'skills-query',
          method: 'snapshot',
          input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
        },
      },
    },
  ];
}
function read(state: State, reference: string, revision: string, offset: number): Action[] {
  const key = `skills-${++state.serial}`;
  state.pending = { kind: 'read', key };
  state.notice = null;
  return [
    {
      key,
      binding: 'skills',
      member: 0,
      method: 'read',
      input: { reference, revision, offset, workspace: state.workspace },
      resources: {},
    },
  ];
}
function preview(state: State, kind: SkillLibraryChangeKind): Action[] {
  const document = state.document;
  if (!document?.writable || state.current || (!document.entry.valid && kind !== 'delete'))
    return [];
  const key = `skills-${++state.serial}`;
  state.pending = { kind: 'preview', key };
  state.notice = null;
  return [
    {
      key,
      binding: 'skills',
      member: 0,
      method: 'preview',
      resources: {},
      input: {
        kind,
        reference: document.entry.reference,
        revision: document.revision,
        workspace: state.workspace,
      },
    },
  ];
}
function recover(state: State): Action[] {
  if (!state.current?.reference || state.pending) return [];
  const key = `skills-${++state.serial}`;
  state.pending = { kind: 'receipt', key };
  return [
    {
      key,
      binding: 'receipts',
      member: 0,
      method: 'read',
      resources: {},
      input: state.current.reference,
    },
  ];
}
function rejection(reason: string): keyof Copy {
  if (reason === 'source_exists') return 'skillsSourceExists';
  if (reason === 'invalid_source_id') return 'skillsSourceIdInvalid';
  if (reason === 'invalid_source' || reason === 'invalid_skill') return 'skillsImportInvalid';
  return reason === 'already_installed'
    ? 'skillsAlreadyInstalled'
    : reason === 'not_installed'
      ? 'skillsNotInstalled'
      : 'skillsChangeRejected';
}
function settle(
  state: State,
  props: SkillLibraryProps,
  reply: OperationReply,
  readPublished = false,
): Action[] {
  if (
    !state.current ||
    (state.current.reference && !sameValue(state.current.reference, reply.reference)) ||
    reply.reference.contract.name !== 'maka.skills.Library' ||
    reply.reference.method !== state.current.method
  )
    return [];
  state.current.reference = reply.reference;
  if (reply.receipt.kind !== 'recorded' || reply.receipt.outcome.kind !== 'returned') {
    state.notice = 'skillsUnknown';
    if (
      readPublished &&
      state.current.method !== 'setPreference' &&
      reply.receipt.kind === 'recorded'
    ) {
      const key = `skills-${++state.serial}`;
      state.pending = { kind: 'outcome', key };
      return [
        {
          key,
          binding: 'outcomes',
          member: 0,
          method: 'read',
          resources: {},
          input: reply.reference,
        },
      ];
    }
    return [];
  }
  if (state.current.method === 'setPreference') {
    const result = reply.receipt.outcome.value as SkillLibraryPreferenceResult;
    state.current = null;
    const changed = selectWorkspace(state, props);
    state.cleanupUnconfirmed = reply.receipt.cleanup.kind !== 'confirmed';
    if (result.kind === 'applied') {
      state.document = null;
      return state.open && props.open ? list(state) : [];
    }
    const actions = changed && state.open && props.open ? list(state) : [];
    state.notice = result.kind === 'changed' ? 'skillsChanged' : 'skillsChangeRejected';
    return actions;
  }
  return settledResult(
    state,
    props,
    reply.receipt.outcome.value as SkillLibraryChangeResult,
    reply.receipt.cleanup,
  );
}
function settledResult(
  state: State,
  props: SkillLibraryProps,
  result: SkillLibraryChangeResult,
  cleanup: OperationCleanup,
): Action[] {
  state.current = null;
  const changed = selectWorkspace(state, props);
  state.cleanupUnconfirmed = cleanup.kind !== 'confirmed';
  if (result.kind === 'applied') {
    state.importer.open = false;
    state.importer.confirmation = null;
    state.document = null;
    const actions = state.open && props.open ? list(state) : [];
    state.notice = null;
    return actions;
  }
  const actions = changed && state.open && props.open ? list(state) : [];
  state.notice = result.kind === 'changed' ? 'skillsChanged' : rejection(result.reason);
  return actions;
}
function reveal(state: State, key: string) {
  state.reveal = {
    sequence: (state.reveal?.sequence ?? 0) + 1,
    target: { kind: 'reveal_child', key },
  };
}
function rowKey(state: State, at: number) {
  return `${state.pageKey}-row-${at}`;
}
function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: SkillLibraryProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const busy = !!state.pending;
  const available = state.open && props.open && !busy && !state.preview && readable(state);
  const document = state.document;
  const content: N[] = state.workspace
    ? [ui.text('skills-project', state.workspace.path, props.theme.muted)]
    : [];
  if (state.preview) {
    const preview = state.preview;
    const kind = preview.review.change.kind;
    content.push(
      ui.text('skills-preview-title', text.skillsConfirmTitle, props.theme.text, 550),
      ui.text('skills-preview-target', preview.target),
      ui.text(
        'skills-preview-summary',
        kind === 'delete'
          ? preview.target.startsWith('imported:')
            ? text.skillsDeleteSourceHelp
            : text.skillsDeleteDetail
          : kind === 'update'
            ? text.skillsUpdateDetail
            : text.skillsInstallDetail,
      ),
      ui.text('skills-preview-count', `${text.skillsEntryCount}: ${preview.entries}`),
      ...(preview.locallyModified && kind === 'update'
        ? [ui.text('skills-preview-modified', text.skillsLocalModified)]
        : []),
      presentation.slot(
        'skills-preview-paths',
        'source',
        {
          source: preview.paths.map((path) => JSON.stringify(path)).join('\n'),
          appearance: props.theme.markdown.code,
          selection: props.theme.markdown.selection,
          terminal: props.theme.terminal,
          disabled: false,
          focusOnPointer: true,
        },
        {},
      ),
      ...(preview.pathsTruncated
        ? [ui.text('skills-preview-truncated', text.skillsPathsTruncated)]
        : []),
    );
  } else if (document) {
    const entry = document.entry;
    content.push(
      ui.text('skill-name', entry.name, props.theme.text, 550),
      ui.text(
        'skill-source',
        entry.imported
          ? text.skillsImported
          : entry.bundled
            ? text.skillsBundled
            : entry.installed
              ? text.skillsInstalled
              : entry.source,
        props.theme.muted,
      ),
      ui.text('skill-description', entry.description),
      ui.text(
        'skill-preference',
        entry.imported
          ? text.skillsImported
          : `${entry.preference.enabled ? text.skillsEnabled : text.skillsDisabled}${entry.preference.pinned ? ` · ${text.skillsPinned}` : ''}`,
      ),
      ui.text(
        'skill-range',
        `${text.toolByteRange}: ${document.totalBytes ? document.offset + 1 : 0}–${document.after ?? document.totalBytes} / ${document.totalBytes}`,
        props.theme.muted,
      ),
    );
    if (!entry.valid) content.push(ui.text('skill-invalid', text.skillsInvalid));
    if (entry.shadowedBy)
      content.push(ui.text('skill-shadow', `${text.skillsShadowed}: ${entry.shadowedBy}`));
    if (entry.metadataTruncated) content.push(ui.text('skill-truncated', text.skillsTruncated));
    entry.issues.forEach((issue, at) => content.push(ui.text(`skill-issue-${at}`, issue)));
    content.push(
      presentation.slot(
        `skill-body-${state.pageKey}-${state.active}-${document.offset}`,
        'source',
        {
          source: document.text,
          appearance: props.theme.markdown.code,
          selection: props.theme.markdown.selection,
          terminal: props.theme.terminal,
          disabled: false,
          focusOnPointer: true,
        },
        {},
      ),
    );
  } else {
    state.page?.entries.forEach((entry, at) => {
      const key = rowKey(state, at);
      const label = compactLabel(entry.name, 100);
      const summary = compactLabel(
        `${entry.imported ? text.skillsImported : entry.bundled ? text.skillsBundled : entry.installed ? text.skillsInstalled : entry.source} · ${entry.description}`,
        160,
      );
      content.push(
        ui.item(
          key,
          ui.box(
            `${key}-content`,
            [
              ui.text(`${key}-name`, label, props.theme.text, 550),
              ui.text(`${key}-summary`, summary, props.theme.muted),
            ],
            { gap: 0 },
          ),
          at === state.active,
          !available,
          { followFocus: false, submit: false },
        ),
      );
    });
    if (!busy && state.page && !state.page.entries.length)
      content.push(ui.text('skills-empty', text.skillsEmpty));
    state.page?.diagnostics.forEach((diagnostic, at) =>
      content.push(ui.text(`skills-diagnostic-${at}`, diagnostic, props.theme.muted)),
    );
  }
  const view = ui.box(
    'skill-library',
    [
      ui.box(
        'skills-search',
        [
          presentation.slot(
            'skills-query',
            'input',
            {
              appearance: props.theme.input,
              accessibilityLabel: text.skillsSearch,
              placeholder: text.skillsSearch,
              autoFocus: true,
              readOnly: false,
              disabled: !state.open || !props.open,
              tabIndex: 1,
            },
            { changed: 'query', submitted: 'search' },
          ),
          ui.button('search', text.skillsSubmit, busy),
        ],
        { visible: !document, gap: 0 },
      ),
      ...(busy ? [ui.text('skills-loading', text.skillsLoading, props.theme.muted)] : []),
      ...(state.current
        ? [ui.button('check-change', text.skillsCheckResult, busy || !state.current.reference)]
        : []),
      ...(state.notice ? [ui.text('skills-notice', text[state.notice], props.theme.muted)] : []),
      ...(state.cleanupUnconfirmed
        ? [ui.text('skills-cleanup', text.skillsCleanupUnknown, props.theme.muted)]
        : []),
      presentation.reading(
        'skills-reading',
        {
          content: ui.box('skills-viewport', content, {
            scroll: 'manual',
            focusGroup: !document,
            autoFocus: !!document,
            gap: 0,
            terminal: { grow: 1, basis: 0 },
          }),
          command: state.reveal,
        },
        { input: 'reading-input', observed: 'reading-observed' },
      ),
      ...(document?.preferencesWritable && !document.entry.imported && !state.preview
        ? [
            ui.box(
              'skills-preferences',
              [
                ui.button(
                  'toggle-enabled',
                  document.entry.preference.enabled ? text.skillsDisable : text.skillsEnable,
                  !available || !!state.current,
                ),
                ui.button(
                  'toggle-pinned',
                  document.entry.preference.pinned ? text.skillsUnpin : text.skillsPin,
                  !available || !!state.current,
                ),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
          ]
        : []),
      ...(document?.writable && !state.preview
        ? [
            ui.box(
              'skills-management',
              document.entry.installed
                ? [ui.button('delete-skill', text.skillsDelete, !available || !!state.current)]
                : [
                    ui.button(
                      'install-skill',
                      text.skillsInstall,
                      !available || !!state.current || !document.entry.valid,
                    ),
                    ui.button(
                      'update-skill',
                      text.skillsUpdate,
                      !available || !!state.current || !document.entry.valid,
                    ),
                    ...(document.entry.imported
                      ? [
                          ui.button(
                            'delete-skill',
                            text.skillsDeleteSource,
                            !available || !!state.current,
                          ),
                        ]
                      : []),
                  ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
          ]
        : []),
      ui.box(
        'skills-controls',
        state.preview
          ? [
              ui.button('confirm-change', text.skillsConfirm, busy, true),
              ui.button('cancel-change', text.cancel, busy),
            ]
          : [
              ...(document ? [ui.button('back', text.back, busy || !!state.preview)] : []),
              ...((document?.writable ?? state.page?.writable)
                ? [ui.button('open-import', text.skillsImport, busy || !!state.current)]
                : []),
              ui.button('refresh', text.refreshSessions, busy || !!state.preview),
              ui.button('close', text.closeModels),
            ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    { visible: state.open && props.open, gap: 0, terminal: { grow: 1 } },
  );
  const out = output(
    state,
    ui.box(
      'skills-root',
      [
        ui.box('skills-browser', [view], { visible: !state.importer.open, terminal: { grow: 1 } }),
        importView(
          presentation,
          props,
          state.importer,
          busy,
          !!state.current,
          !!state.current?.reference,
          state.notice ? text[state.notice] : null,
          state.cleanupUnconfirmed,
        ),
      ],
      {
        visible: state.open && props.open,
        terminal: { grow: 1 },
        shortcuts: [{ key: 'escape', modifiers: [], input: 'close', enabled: true }],
      },
    ),
    events,
  );
  out.actions = actions;
  return out;
}
export function skillLibrary<N extends Node>(
  presentation: Presentation<N>,
): Component<State, SkillLibraryProps, N> {
  return {
    mount(props) {
      const state: State = {
        importer: sourceImport(),
        workspace: props.workspace ?? null,
        revision: 0,
        serial: 0,
        open: props.open,
        observation: null,
        basis: null,
        query: '',
        page: null,
        pageKey: '',
        cursor: 0,
        history: [],
        document: null,
        pending: null,
        notice: null,
        cleanupUnconfirmed: false,
        preview: null,
        current: null,
        active: 0,
        reveal: null,
      };
      return frame(presentation, state, props, props.open ? list(state) : []);
    },
    update(previous, props) {
      const state = { ...previous, revision: previous.revision + 1 };
      let actions: Action[] = [];
      const changed = selectWorkspace(state, props);
      if (!props.open) {
        state.open = false;
        state.preview = null;
        if (
          state.pending?.kind !== 'mutate' &&
          state.pending?.kind !== 'receipt' &&
          state.pending?.kind !== 'outcome'
        )
          state.pending = null;
      } else if (!state.open || changed) {
        state.open = true;
        state.document = null;
        actions = state.current ? recover(state) : state.pending ? [] : search(state);
      }
      return frame(presentation, state, props, actions);
    },
    event(state, input, props) {
      state.revision++;
      const events: Emission[] = [];
      let actions: Action[] = [];
      if (input.kind === 'component' && input.child === 'skills-reading' && input.name === 'input')
        input = input.event as ComponentInput;
      if (
        (!state.open || !props.open) &&
        !(
          input.kind === 'component' &&
          ['skills-import-file', 'skills-import-id'].includes(input.child)
        ) &&
        !(
          input.kind === 'action' &&
          input.key === state.pending?.key &&
          (state.pending.kind === 'mutate' ||
            state.pending.kind === 'receipt' ||
            state.pending.kind === 'outcome')
        )
      )
        return frame(presentation, state, props);
      const direction = scrollBoundary(state, input, 'skills-reading');
      if (direction && !state.pending && !state.preview && !state.current)
        input = {
          kind: 'raw',
          key: state.document
            ? direction === 'up'
              ? 'body-previous'
              : 'body-next'
            : direction === 'up'
              ? 'previous'
              : 'next',
          event: {},
        };
      if (
        input.kind === 'component' &&
        input.child === 'skills-import-file' &&
        input.name === 'changed'
      ) {
        state.importer.file = input.event as FileObservation;
        if (!importConfirmed(state.importer)) state.importer.confirmation = null;
      } else if (
        input.kind === 'component' &&
        input.child === 'skills-import-id' &&
        input.name === 'changed'
      ) {
        state.importer.id = input.event as EditorObservation;
        if (!importConfirmed(state.importer)) state.importer.confirmation = null;
      } else if (input.kind === 'component' && input.child === 'skills-query') {
        if (input.name === 'changed') {
          state.observation = input.event as EditorObservation;
          // The native editor's initial empty observation already matches the initial query.
          if (!state.basis && !state.query && !state.observation.snapshot.text.bytes)
            state.basis = state.observation.snapshot.text;
        } else if (input.name === 'submitted' && !state.pending) actions = search(state);
      } else if (input.kind === 'action' && input.key === state.pending?.key) {
        const request = state.pending;
        state.pending = null;
        const result = input.event as ActionOutcome;
        if (request.kind === 'mutate') {
          if (result.status === 'operation') actions = settle(state, props, result.reply);
          else if (result.status === 'not_accepted') {
            state.current = null;
            if (selectWorkspace(state, props) && state.open && props.open) actions = list(state);
            state.notice = 'skillsFailed';
          } else state.notice = 'skillsUnknown';
        } else if (request.kind === 'receipt') {
          if (result.status === 'succeeded')
            actions = settle(state, props, result.value as OperationReply, true);
          else state.notice = 'skillsUnknown';
        } else if (request.kind === 'outcome') {
          if (result.status === 'succeeded') {
            const outcome = result.value as SkillLibraryOutcome;
            if (sameValue(state.current?.reference, outcome.original.reference)) {
              actions = settle(state, props, outcome.original);
              if (
                state.current &&
                outcome.published &&
                outcome.original.receipt.kind === 'recorded'
              )
                actions = settledResult(
                  state,
                  props,
                  outcome.published,
                  outcome.original.receipt.cleanup,
                );
            }
          } else state.notice = 'skillsUnknown';
        } else if (result.status !== 'succeeded') state.notice = 'skillsFailed';
        else if (request.kind === 'preview') {
          const preview = result.value as SkillLibraryPreview;
          if (preview.kind === 'ready') {
            state.preview = preview;
            reveal(state, 'skills-preview-title');
          } else
            state.notice = preview.kind === 'changed' ? 'skillsChanged' : rejection(preview.reason);
        } else if (request.kind === 'query') {
          if (!same(request.basis, state.observation?.snapshot.text ?? null))
            state.notice = 'skillsFailed';
          else {
            const query = (result.value as TextChunk).text.trim();
            if ([...query].length > 256) state.notice = 'searchTooLong';
            else {
              state.query = query;
              state.basis = request.basis;
              actions = list(state);
            }
          }
        } else if (request.kind === 'list') {
          const page = result.value as SkillLibraryPage;
          if (page.kind === 'changed') state.notice = 'skillsChanged';
          else {
            state.page = page;
            state.pageKey = request.key;
            state.cursor = request.cursor?.offset ?? 0;
            state.history = request.history;
            state.document = null;
            state.active = 0;
            reveal(state, rowKey(state, 0));
            if (request.landing) state.reveal!.target = { kind: request.landing };
          }
        } else {
          const document = result.value as SkillLibraryDocument;
          if (document.kind !== 'page')
            state.notice = document.kind === 'changed' ? 'skillsChanged' : 'skillsFailed';
          else {
            state.document = document;
            reveal(state, 'skill-name');
            if (request.kind === 'read' && request.landing)
              state.reveal!.target = { kind: request.landing };
          }
        }
      } else if (input.kind === 'raw') {
        if (input.key === 'close' || input.key === 'close-import') {
          if (state.preview) state.preview = null;
          else {
            state.open = false;
            if (
              state.pending?.kind !== 'mutate' &&
              state.pending?.kind !== 'receipt' &&
              state.pending?.kind !== 'outcome'
            )
              state.pending = null;
            events.push({ name: 'closed', value: {} });
          }
        } else if (!state.pending) {
          if (state.importer.open) {
            const importer = state.importer;
            if (input.key === 'check-import') actions = recover(state);
            else if (!state.current) {
              if (input.key === 'back-import') {
                importer.open = false;
                importer.confirmation = null;
              } else if (input.key === 'cancel-import-confirmation') importer.confirmation = null;
              else if (input.key === 'review-import' && importReady(importer)) {
                importer.confirmation = { file: importer.file!, id: importer.id!.snapshot.text };
                state.notice = null;
              } else if (input.key === 'confirm-import' && importConfirmed(importer)) {
                const intent = importer.confirmation!;
                const key = `skills-${++state.serial}`;
                state.pending = { kind: 'mutate', key };
                state.current = { method: 'importSource', reference: null };
                state.cleanupUnconfirmed = false;
                state.notice = null;
                actions = [
                  {
                    key,
                    binding: 'skills',
                    member: 0,
                    method: 'importSource',
                    input: { bytes: intent.file.file!.bytes, digest: intent.file.file!.digest },
                    resources: {
                      file: {
                        child: 'skills-import-file',
                        method: 'snapshot',
                        input: { owner: intent.file.owner, revision: intent.file.revision },
                      },
                      id: {
                        child: 'skills-import-id',
                        method: 'snapshot',
                        input: {
                          kind: 'at_revision',
                          owner: intent.id.owner,
                          revision: intent.id.revision,
                        },
                      },
                    },
                  },
                ];
                importer.confirmation = null;
              }
            }
          } else if (
            input.key === 'open-import' &&
            !state.current &&
            (state.document?.writable ?? state.page?.writable)
          ) {
            state.importer.open = true;
            state.notice = null;
          } else if (input.key === 'check-change') actions = recover(state);
          else if (input.key === 'cancel-change') state.preview = null;
          else if (input.key === 'confirm-change' && state.preview && !state.current) {
            const key = `skills-${++state.serial}`;
            state.pending = { kind: 'mutate', key };
            const method =
              state.preview.review.change.kind === 'delete' &&
              state.preview.target.startsWith('imported:')
                ? 'deleteSource'
                : 'apply';
            state.current = { method, reference: null };
            state.cleanupUnconfirmed = false;
            actions = [
              {
                key,
                binding: 'skills',
                member: 0,
                method,
                input: state.preview.review,
                resources: {},
              },
            ];
            state.preview = null;
          } else if (state.preview) {
            /* Keep the reviewed change until confirmation or cancellation. */
          } else if (input.key === 'back') {
            state.document = null;
            reveal(state, rowKey(state, state.active));
          } else if (input.key === 'refresh' || input.key === 'search') {
            state.document = null;
            actions = search(state);
          } else if (readable(state)) {
            const document = state.document,
              page = state.page;
            if (document) {
              if (
                (input.key === 'toggle-enabled' || input.key === 'toggle-pinned') &&
                document.preferencesWritable &&
                !document.entry.imported &&
                !state.current
              ) {
                const key = `skills-${++state.serial}`;
                state.pending = { kind: 'mutate', key };
                state.current = { method: 'setPreference', reference: null };
                state.cleanupUnconfirmed = false;
                const preference = { ...document.entry.preference };
                if (input.key === 'toggle-enabled') preference.enabled = !preference.enabled;
                else preference.pinned = !preference.pinned;
                actions = [
                  {
                    key,
                    binding: 'skills',
                    member: 0,
                    method: 'setPreference',
                    resources: {},
                    input: {
                      reference: document.entry.reference,
                      revision: document.preferenceRevision,
                      preference,
                    },
                  },
                ];
              }
              if (
                input.key === 'install-skill' ||
                input.key === 'update-skill' ||
                input.key === 'delete-skill'
              )
                actions = preview(
                  state,
                  input.key === 'install-skill'
                    ? 'install'
                    : input.key === 'update-skill'
                      ? 'update'
                      : 'delete',
                );
              const offset =
                input.key === 'body-previous'
                  ? document.before
                  : input.key === 'body-next'
                    ? document.after
                    : null;
              if (offset != null)
                actions = read(state, document.entry.reference, document.revision, offset);
            } else if (page) {
              if (input.key === 'next' && page.after)
                actions = list(state, page.after, [...state.history, state.cursor]);
              else if (input.key === 'previous' && state.history.length)
                actions = list(
                  state,
                  { revision: page.revision, query: state.query, offset: state.history.at(-1)! },
                  state.history.slice(0, -1),
                );
              else {
                const at = page.entries.findIndex((_, at) => rowKey(state, at) === input.key);
                if (at >= 0) {
                  state.active = at;
                  actions = read(state, page.entries[at]!.reference, page.revision, 0);
                }
              }
            }
          }
        }
      }
      if (
        direction &&
        actions.length &&
        state.pending &&
        (state.pending.kind === 'list' || state.pending.kind === 'read')
      )
        state.pending.landing = direction === 'up' ? 'end' : 'start';
      return frame(presentation, state, props, actions, events);
    },
  };
}
export function activate() {
  return {
    SkillLibrary: { kind: 'component', contract: skillLibraryContract, component: ['component'] },
  };
}
