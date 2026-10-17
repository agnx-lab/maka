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
  ActionOutcome,
  AgentDataSource,
  AgentSendResult,
  CallerConfirmation,
  CallerIntent,
  Component,
  EditorCheckpointObservation,
  EditorCommand,
  EditorObservation,
  Emission,
  FilePasteObservation,
  OperationReply,
  RecoveryRef,
  TextChunk,
  TextareaProps,
} from '../../../packages/plugin-sdk/src/index.js';
import type { ComposerBackend, ComposerProps, ComposerSubmission } from '../../shared/contracts.js';
import {
  observeDraft,
  nextDraftStep,
  draftPage,
  draftReceipt,
  draftFailed,
  retryDraft,
  type ComposerReadiness,
  type DraftRecord,
  type DraftStep,
} from '../../shared/drafts.js';
import { tuiText, copy } from './locale.js';
import { output, type Presentation, type Node } from '../../shared/view.js';
import { sameValue } from '../../shared/value.js';
import { editorMenu } from '../../shared/editor-menu.js';

type CapturedDraft = Pick<EditorObservation, 'revision'> & {
  snapshot: Pick<EditorObservation['snapshot'], 'text'>;
};
interface Submission {
  preview?: { key: string; text: string };
  context: string;
  dataSource: AgentDataSource;
  backend: ComposerBackend | null;
  command?: string;
  draft: CapturedDraft;
  attachments: string[];
}
type Delivery =
  | { kind: 'capture' | 'read' | 'send'; key: string; submission: Submission }
  | {
      kind: 'unknown';
      submission: Submission;
      reference: RecoveryRef;
      caller: CallerConfirmation | null;
      checking: string | null;
    }
  | { kind: 'clear'; command: EditorCommand; caller: CallerConfirmation | null };
interface Prefix {
  read: { key: string; observation: EditorObservation } | null;
  attempted: EditorObservation['snapshot']['text'] | null;
  cached: { metadata: EditorObservation['snapshot']['text']; text: string } | null;
  selected: number;
  dismissed: number | null;
}
interface DraftEntry {
  prefix: Prefix;
  knownSkillLiterals: string[];
  dataSource: AgentDataSource;
  checkpoint: EditorCheckpointObservation | null;
  retry: number;
  context: string;
  key: string;
  observation: EditorObservation | null;
  files: FilePasteObservation | null;
  history: { after: number; scope: string | null };
  delivery: Delivery | null;
  notice: 'draftKept' | 'empty' | 'skillInputUnavailable' | 'unknownCommand' | null;
}
interface SkillCandidate {
  reference: string;
  name: string;
  description: string;
  invocation: string;
}
interface SkillSearch {
  scope: string;
  query: string;
  key: string;
  observation: EditorObservation;
  items: SkillCandidate[];
}
interface Draft {
  skills: SkillSearch | null;
  revision: number;
  active: string | null;
  serial: number;
  entries: DraftEntry[];
  records: DraftRecord[];
  recovery: { key: string; step: DraftStep } | null;
  reported: ComposerReadiness[];
}
// Only a single slash token is command input. Paths and multiline drafts stay prose.
function slashQuery(text: string): string | null {
  if (/^\/skill:[A-Za-z0-9._-]+$/u.test(text)) return null;
  return /^\/[^\s/\\]*$/u.test(text) ? text.slice(1).toLocaleLowerCase() : null;
}
function rememberSkills(entry: DraftEntry, literals: Iterable<string>) {
  for (const literal of literals) {
    if (entry.knownSkillLiterals.length === 4096) break;
    if (
      literal.length <= 512 &&
      /^\/skill:[A-Za-z0-9._-]+$/u.test(literal) &&
      !entry.knownSkillLiterals.includes(literal)
    )
      entry.knownSkillLiterals.push(literal);
  }
}
function rememberSkillReferences(entry: DraftEntry, text: string) {
  rememberSkills(
    entry,
    [...text.matchAll(/(?:^|\s)(\/skill:[A-Za-z0-9._-]+)(?=\s|$)/gu)].map((match) => match[1]),
  );
}
interface SlashCompletion {
  query: string;
  start: number;
  end: number;
  root: boolean;
}
function slashCompletion(text: string, observation: EditorObservation): SlashCompletion | null {
  const { start, end } = observation.snapshot.selection;
  if (start !== end || start > text.length) return null;
  const before = text.slice(0, start);
  const match = /(?:^|\s)(\/[^\s/\\]*)$/u.exec(before);
  if (!match) return null;
  const tokenStart = start - match[1].length;
  const suffix = /^[^\s]*/u.exec(text.slice(start))![0];
  const tokenEnd = start + suffix.length;
  if (slashQuery(text.slice(tokenStart, tokenEnd)) === null) return null;
  return {
    query: match[1].slice(1).toLocaleLowerCase(),
    start: tokenStart,
    end: tokenEnd,
    root: tokenStart === 0 && tokenEnd === text.length,
  };
}
function completionEdit(
  completion: SlashCompletion,
  text: string,
  invocation: string,
): EditorCommand['edit'] {
  const insert = `${invocation}${/^[ \t]/u.test(text.slice(completion.end)) ? '' : ' '}`;
  return completion.root
    ? { kind: 'replace', text: insert }
    : {
        kind: 'replace_range',
        range: { start: completion.start, end: completion.end },
        text: insert,
      };
}
function candidates(
  props: ComposerProps,
  text: string,
  skills: SkillCandidate[] = [],
  completion?: SlashCompletion | null,
) {
  const query = completion ? completion.query : slashQuery(text);
  return query === null
    ? []
    : [
        ...(completion && !completion.root ? [] : (props.commands ?? [])).map((item) => ({
          ...item,
          invocation: null as string | null,
        })),
        ...skills.map((item) => ({
          id: `skill:${item.reference}`,
          command: item.name,
          label: item.description,
          disabled: false,
          invocation: item.invocation,
        })),
      ].filter(
        (item) =>
          !item.disabled &&
          (item.command.toLocaleLowerCase().includes(query) ||
            item.label.toLocaleLowerCase().includes(query)),
      );
}
function prefixText(entry: DraftEntry) {
  return sameValue(entry.prefix.cached?.metadata, entry.observation?.snapshot.text)
    ? (entry.prefix.cached?.text ?? null)
    : null;
}
function snapshotRead(key: string, entry: DraftEntry, observation: CapturedDraft): Action {
  return {
    ...action(key, 'reader', 'read', { start: 0, end: observation.snapshot.text.bytes }),
    resources: {
      text: {
        child: entry.key,
        method: 'snapshot',
        input: {
          kind: 'at_revision',
          owner: observation.snapshot.text.owner,
          revision: observation.snapshot.text.revision,
        },
      },
    },
  };
}
function action(key: string, binding: string, method: string, input: unknown): Action {
  return { key, binding, member: 0, method, input, resources: {}, continuations: [] };
}
function advance(entry: DraftEntry, caller: CallerConfirmation | null) {
  if (caller)
    entry.history = {
      after: caller.cursor,
      scope: JSON.stringify([caller.caller.stateDomain, caller.caller.deployment, caller.consumer]),
    };
}
function submissionEvent(
  submission: Submission,
  status: ComposerSubmission['status'],
  messageId: string | null = null,
): Emission[] {
  return submission.preview
    ? [
        {
          name: 'submission',
          value: {
            context: submission.context,
            dataSource: submission.dataSource,
            ...submission.preview,
            attachments: submission.attachments,
            status,
            messageId,
          } satisfies ComposerSubmission,
        },
      ]
    : [];
}
function admitted(
  state: Draft,
  entry: DraftEntry,
  submission: Submission,
  reply: OperationReply,
  caller: CallerConfirmation | null,
  events: Emission[],
) {
  if (reply.receipt.kind === 'recorded' && reply.receipt.outcome.kind === 'returned') {
    const result = reply.receipt.outcome.value as AgentSendResult;
    if (result.sessionId === submission.context) {
      if (result.kind === 'rejected') {
        entry.delivery = null;
        entry.notice = 'skillInputUnavailable';
        events.push(...submissionEvent(submission, 'removed'));
      } else {
        events.push(
          ...submissionEvent(
            submission,
            result.kind === 'started' ? 'accepted' : 'removed',
            result.messageId,
          ),
        );
        entry.notice = null;
        entry.delivery = {
          kind: 'clear',
          caller,
          command: {
            owner: submission.draft.snapshot.text.owner,
            sequence: ++state.serial,
            basedOn: submission.draft.revision,
            edit: { kind: 'reset', text: '' },
          },
        };
        events.push({
          name: 'sent',
          value: {
            context: submission.context,
            dataSource: submission.dataSource,
            attachments: submission.attachments,
          },
        });
      }
      return;
    }
  }
  events.push(...submissionEvent(submission, 'unknown'));
  entry.delivery = {
    kind: 'unknown',
    submission,
    reference: reply.reference,
    caller,
    checking: null,
  };
}
export function composer<N extends Node>(
  presentation: Presentation<N>,
): Component<Draft, ComposerProps, N> {
  const composerFrame = (
    state: Draft,
    props: ComposerProps,
    events: Emission[] = [],
    actions: Action[] = [],
  ) => {
    const ui = presentation.paint(props.theme),
      text = copy(props.locale);
    // Each session retains its original native mount, including selection and
    // undo. Only observed empty, idle editors can make room at the original limit.
    if (
      props.context &&
      props.dataSource &&
      !state.entries.some(
        (entry) => entry.context === props.context && sameValue(entry.dataSource, props.dataSource),
      )
    ) {
      if (state.entries.length === 32 && !props.busy) {
        const empty = state.entries.findIndex(
          (entry) =>
            !entry.delivery &&
            entry.checkpoint?.state.kind === 'ready' &&
            entry.checkpoint.state.saved &&
            entry.observation?.snapshot.text.bytes === 0 &&
            !entry.observation.snapshot.marked &&
            (!entry.files || entry.files.selection.phase === 'empty'),
        );
        if (empty >= 0) state.entries.splice(empty, 1);
      }
      if (state.entries.length < 32)
        state.entries.push({
          prefix: { read: null, attempted: null, cached: null, selected: 0, dismissed: null },
          knownSkillLiterals: [],
          context: props.context,
          dataSource: props.dataSource,
          checkpoint: null,
          retry: 0,
          key: state.serial++ === 0 ? 'draft' : `draft-${state.serial}`,
          observation: null,
          files: null,
          history: { after: 0, scope: null },
          delivery: null,
          notice: null,
        });
    }
    const current = state.entries.find(
      (entry) => entry.context === props.context && sameValue(entry.dataSource, props.dataSource),
    );
    const observation = current?.observation;
    if (state.active !== (current?.key ?? null)) {
      state.active = current?.key ?? null;
      if (current?.checkpoint?.state.kind === 'restore') current.retry++;
    }
    const pending = !!current?.delivery;
    if (
      current &&
      observation &&
      !pending &&
      !current.prefix.read &&
      observation.snapshot.text.bytes > 0 &&
      observation.snapshot.text.bytes <= 65536 &&
      !observation.snapshot.marked &&
      !sameValue(current.prefix.attempted, observation.snapshot.text)
    ) {
      const key = `prefix-${++state.serial}`;
      current.prefix.read = { key, observation };
      current.prefix.attempted = observation.snapshot.text;
      actions.push(snapshotRead(key, current, observation));
    }
    const shortText = current ? prefixText(current) : null;
    const completion =
      shortText !== null && observation ? slashCompletion(shortText, observation) : null;
    const menuOpen =
      !!current &&
      !!observation &&
      !pending &&
      props.enabled &&
      !props.busy &&
      !observation.snapshot.marked &&
      current.prefix.dismissed !== observation.revision &&
      shortText !== null &&
      completion !== null;
    const skillScope = JSON.stringify([
      props.context,
      props.dataSource,
      props.backend,
      props.skillContext,
    ]);
    const skillQuery = completion?.query ?? null;
    if (
      !props.enabled ||
      !props.backend ||
      skillQuery === null ||
      current?.prefix.dismissed === observation?.revision ||
      state.skills?.scope !== skillScope ||
      state.skills.query !== skillQuery ||
      !sameValue(state.skills.observation, observation)
    )
      state.skills = null;
    if (menuOpen && props.backend && !state.skills) {
      const key = `skills-${++state.serial}`;
      state.skills = {
        scope: skillScope,
        query: skillQuery!,
        key,
        observation: observation!,
        items: [],
      };
      actions.push(
        action(key, 'agent', 'skillCandidates', {
          sessionId: props.context,
          backend: props.backend,
          query: skillQuery,
        }),
      );
    }
    const choices = menuOpen ? candidates(props, shortText!, state.skills?.items, completion) : [];
    if (current)
      current.prefix.selected = Math.min(current.prefix.selected, Math.max(0, choices.length - 1));
    const selected = current?.prefix.selected ?? 0;
    const first = Math.max(0, selected - 5);
    const attached = props.attachments.filter(
      (file) => file.sessionId === props.context && sameValue(file.dataSource, props.dataSource),
    );
    const retained = { files: [...props.attachments], selection: null, paste: null };
    if (!state.recovery) {
      const step = nextDraftStep(state.records, retained);
      if (step) {
        const key = `restore-${++state.serial}`;
        state.recovery = { key, step };
        actions.push(
          action(
            key,
            step.kind === 'draftPage' ? 'history' : 'receipts',
            step.kind === 'draftPage' ? 'page' : 'read',
            step.kind === 'draftPage' ? { after: step.after, limit: 1 } : step.original.reference,
          ),
        );
      }
    }
    const restored = retained.files.slice(props.attachments.length);
    if (restored.length) events.push({ name: 'restored', value: { files: restored } });
    const readiness = state.entries.map((entry) => ({
      context: entry.context,
      dataSource: entry.dataSource,
      ready:
        entry.checkpoint?.state.kind === 'ready' &&
        entry.checkpoint.state.initialized &&
        !state.records.some(
          (record) => record.observation.owner === entry.checkpoint?.owner && record.work,
        ) &&
        entry.delivery?.kind !== 'unknown',
    }));
    if (!sameValue(state.reported, readiness)) {
      state.reported = readiness;
      events.push({ name: 'readiness', value: readiness });
    }
    const available =
      current?.checkpoint?.state.kind === 'ready' &&
      current.checkpoint.state.initialized &&
      props.enabled &&
      !props.busy &&
      true &&
      !pending &&
      !!observation &&
      !observation.snapshot.marked &&
      true;
    const textarea: TextareaProps = {
      appearance: { ...props.theme.textarea, borderWidth: 0 },
      menu: editorMenu(props.theme, props.locale),
      accessibilityLabel: text.draft,
      readOnly: false,
      disabled: false,
      softWrap: true,
      terminalRows: { min: 1, max: 8, maxViewportFraction: 1 / 3 },
      submit: 'enter',
      tabIndex: 1,
      autoFocus: props.autoFocus,
      initialText: '',
      placeholder: tuiText(props.locale, 'composer-placeholder'),
      command: null,
      inlineTokens: [],
      filePaste: null,
      fileCommand: null,
      persistence: null,
    };
    const editor = ui.box(
      'editor-area',
      state.entries.map((entry) =>
        ui.box(
          `retained-${entry.key}`,
          [
            presentation.slot(
              entry.key,
              'editor',
              {
                ...textarea,
                inlineTokens: entry.knownSkillLiterals,
                persistence: {
                  key: entry.context,
                  format: 'maka.agent.draft@2',
                  dataSource: entry.dataSource,
                  metadata: {
                    historyAfter: entry.history.after,
                    historyScope: entry.history.scope,
                    attachments: props.attachments
                      .filter(
                        (file) =>
                          file.sessionId === entry.context &&
                          sameValue(file.dataSource, entry.dataSource),
                      )
                      .map(({ dataSource: _, ...file }) => file),
                  },
                  restore:
                    state.records.find(
                      (record) => record.observation.owner === entry.checkpoint?.owner,
                    )?.work?.restore ?? null,
                  retry: entry.retry,
                },
                autoFocus: entry === current && props.autoFocus,
                navigation:
                  entry === current && menuOpen && observation
                    ? { owner: observation.snapshot.text.owner, basedOn: observation.revision }
                    : null,
                command: entry.delivery?.kind === 'clear' ? entry.delivery.command : null,
                filePaste:
                  entry === current
                    ? (props.filePaste ?? null)
                    : { context: entry.context, allowFiles: false },
                fileCommand:
                  props.fileCommand?.selection.owner === entry.files?.selection.owner
                    ? props.fileCommand
                    : null,
              },
              {
                changed: 'changed',
                navigated: 'navigated',
                submitted: 'submitted',
                files: 'files',
                checkpoint: 'checkpoint',
              },
            ),
          ],
          { visible: entry === current, terminal: { grow: 1, shrink: true } },
        ),
      ),
      { terminal: { basis: 0, grow: 1, shrink: true } },
    );
    const controls = ui.box(
      'composer-controls',
      [
        ...(props.running
          ? [
              ui.button(
                'stop',
                text.stop,
                props.busy,
                false,
                { background: 0 },
                undefined,
                text.stop,
              ),
              ui.button(
                'send',
                props.sendLabel ?? text.send,
                !available,
                true,
                { background: 0 },
                undefined,
                text.send,
              ),
            ]
          : [
              ui.button(
                'send',
                props.sendLabel ?? text.send,
                !available,
                true,
                { background: 0 },
                undefined,
                text.send,
              ),
            ]),
      ],
      { row: true, gap: 0, terminal: { shrink: false } },
    );
    const recovering = state.records.find(
      (record) => record.observation.owner === current?.checkpoint?.owner,
    )?.work;
    const failed = current?.checkpoint?.state.kind === 'failed' || recovering?.status === 'failed';
    const unknown = recovering?.status === 'unknown' || current?.delivery?.kind === 'unknown';
    const loading =
      !!current &&
      (!current.checkpoint ||
        current.checkpoint.state.kind === 'loading' ||
        current.checkpoint.state.kind === 'restore' ||
        (current.checkpoint.state.kind === 'ready' && !current.checkpoint.state.initialized));
    const status = failed
      ? text.draftPersistenceFailed
      : unknown
        ? text.draftUncertain
        : loading
          ? text.draftRestoring
          : current?.notice
            ? text[current.notice]
            : null;
    const body = ui.box(
      'composer-main',
      [
        ...(props.context && !current
          ? [ui.text('draft-capacity', text.draftCapacity, props.theme.muted)]
          : []),
        ui.box(
          'composer-add-cell',
          [
            ui.button(
              'attach',
              '+',
              props.busy || !props.context || loading || failed || unknown,
              false,
              { background: 0 },
              undefined,
              text.attachmentTitle,
            ),
          ],
          { terminal: { basis: 4 } },
        ),
        editor,
        controls,
      ],
      {
        row: true,
        border: true,
        borderColor: props.approvalPending ? props.theme.warning : undefined,
        busy: props.running && !props.approvalPending,
        gap: 0,
        terminal: { shrink: true },
      },
    );
    const out = output(
      state,
      ui.box(
        'composer',
        [
          ...(status
            ? [
                ui.box(
                  'draft-status-row',
                  [
                    ui.text('draft-status', status, props.theme.muted),
                    ...(failed || unknown
                      ? [
                          ui.button(
                            'draft-retry',
                            text.sourceRetry,
                            current?.delivery?.kind === 'unknown' &&
                              current.delivery.checking !== null,
                          ),
                        ]
                      : []),
                  ],
                  { row: true, terminal: { wrap: true } },
                ),
              ]
            : []),
          ...(menuOpen
            ? [
                ui.box(
                  'slash-menu',
                  [
                    ...choices
                      .slice(first, first + 6)
                      .map((item, index) =>
                        ui.button(
                          `slash:${item.id}`,
                          `${first + index === selected ? '› ' : '  '}/${item.command}  ${item.label}`,
                          false,
                          first + index === selected,
                          { align: 'start' },
                        ),
                      ),
                    ...(!choices.length
                      ? [ui.text('slash-empty', text.unknownCommand, props.theme.muted)]
                      : []),
                    ui.text('slash-hint', text.commandHint, props.theme.muted),
                  ],
                  { gap: 0, terminal: { shrink: false } },
                ),
              ]
            : []),
          body,
        ],
        { terminal: { shrink: true } },
      ),
      events,
    );
    out.actions = actions;
    return out;
  };
  return {
    mount: (props) =>
      composerFrame(
        {
          skills: null,
          revision: 0,
          serial: 0,
          active: null,
          entries: [],
          records: [],
          recovery: null,
          reported: [],
        },
        props,
      ),
    update(state, props) {
      state.revision++;
      return composerFrame(state, props);
    },
    event(state, input, props) {
      state.revision++;
      const events: Emission[] = [],
        actions: Action[] = [];
      if (input.kind === 'action') {
        const outcome = input.event as ActionOutcome;
        if (state.skills?.key === input.key) {
          const scope = JSON.stringify([
            props.context,
            props.dataSource,
            props.backend,
            props.skillContext,
          ]);
          if (state.skills.scope === scope && outcome.status === 'succeeded') {
            const result = outcome.value as { skills: SkillCandidate[] };
            state.skills.items = result.skills.filter((item) =>
              /^\/skill:[A-Za-z0-9._-]+$/u.test(item.invocation),
            );
            const entry = state.entries.find(
              (entry) =>
                entry.observation?.snapshot.text.owner ===
                state.skills!.observation.snapshot.text.owner,
            );
            if (entry)
              rememberSkills(
                entry,
                state.skills.items.map((item) => item.invocation),
              );
          }
          return composerFrame(state, props, events, actions);
        }
        const prefixEntry = state.entries.find((entry) => entry.prefix.read?.key === input.key);
        if (prefixEntry?.prefix.read) {
          const read = prefixEntry.prefix.read;
          prefixEntry.prefix.read = null;
          if (outcome.status === 'succeeded') {
            const chunk = outcome.value as TextChunk;
            if (
              sameValue(chunk.metadata, read.observation.snapshot.text) &&
              sameValue(chunk.metadata, prefixEntry.observation?.snapshot.text) &&
              new TextEncoder().encode(chunk.text).length ===
                read.observation.snapshot.text.bytes &&
              read.observation.snapshot.text.bytes <= 65536
            ) {
              prefixEntry.prefix.cached = {
                metadata: read.observation.snapshot.text,
                text: chunk.text,
              };
              rememberSkillReferences(prefixEntry, chunk.text);
            }
          }
          return composerFrame(state, props, events, actions);
        }
        if (state.recovery?.key === input.key) {
          const step = state.recovery.step;
          state.recovery = null;
          if (outcome.status === 'succeeded') {
            if (step.kind === 'draftPage')
              draftPage(state.records, step, outcome.value as CallerIntent[]);
            else draftReceipt(state.records, step, outcome.value as OperationReply);
          } else draftFailed(state.records, step);
        } else {
          const entry = state.entries.find((entry) => {
            const delivery = entry.delivery;
            return (
              delivery &&
              (delivery.kind === 'unknown'
                ? delivery.checking === input.key
                : delivery.kind !== 'clear' && delivery.key === input.key)
            );
          });
          const delivery = entry?.delivery;
          if (entry && delivery?.kind === 'capture') {
            entry.delivery = null;
            if (outcome.status === 'succeeded') {
              const chunk = outcome.value as TextChunk;
              const metadata = chunk.metadata as EditorObservation['snapshot']['text'] | null;
              if (
                metadata !== null &&
                typeof metadata === 'object' &&
                metadata.owner === delivery.submission.draft.snapshot.text.owner &&
                Number.isSafeInteger(metadata.revision) &&
                metadata.revision >= 0 &&
                Number.isSafeInteger(metadata.bytes) &&
                metadata.bytes >= 0 &&
                metadata.bytes <= 65536
              ) {
                const submission = delivery.submission;
                submission.draft = { revision: metadata.revision, snapshot: { text: metadata } };
                if (!metadata.bytes && !submission.attachments.length) entry.notice = 'empty';
                else {
                  const key = `read-${++state.serial}`;
                  entry.delivery = { kind: 'read', key, submission };
                  actions.push(snapshotRead(key, entry, submission.draft));
                }
              } else entry.notice = 'draftKept';
            } else entry.notice = 'draftKept';
          } else if (entry && delivery?.kind === 'read') {
            entry.delivery = null;
            if (outcome.status === 'succeeded') {
              const chunk = outcome.value as TextChunk;
              const submission = delivery.submission;
              if (
                sameValue(chunk.metadata, submission.draft.snapshot.text) &&
                new TextEncoder().encode(chunk.text).length === submission.draft.snapshot.text.bytes
              ) {
                rememberSkillReferences(entry, chunk.text);
                if (!chunk.text.trim() && !submission.attachments.length) entry.notice = 'empty';
                else if (slashQuery(chunk.text) !== null) {
                  const command = submission.command
                    ? candidates(props, chunk.text).find((item) => item.id === submission.command)
                    : (props.commands ?? []).find(
                        (item) => !item.disabled && `/${item.command}` === chunk.text,
                      );
                  if (
                    command &&
                    submission.context === props.context &&
                    sameValue(submission.dataSource, props.dataSource) &&
                    props.enabled &&
                    !props.busy &&
                    !submission.attachments.length &&
                    sameValue(entry.observation?.snapshot.text, submission.draft.snapshot.text)
                  ) {
                    events.push({
                      name: 'command',
                      value: {
                        context: submission.context,
                        dataSource: submission.dataSource,
                        id: command.id,
                      },
                    });
                    entry.delivery = {
                      kind: 'clear',
                      caller: null,
                      command: {
                        owner: submission.draft.snapshot.text.owner,
                        sequence: ++state.serial,
                        basedOn: submission.draft.revision,
                        edit: { kind: 'reset', text: '' },
                      },
                    };
                  } else entry.notice = command ? 'draftKept' : 'unknownCommand';
                } else if (!submission.backend) {
                  events.push({ name: 'setup', value: { context: submission.context } });
                } else {
                  const key = `send-${++state.serial}`,
                    backend = submission.backend;
                  submission.preview = {
                    key,
                    text: chunk.text.slice(0, 24000).replace(/[\uD800-\uDBFF]$/u, ''),
                  };
                  events.push(...submissionEvent(submission, 'pending'));
                  entry.delivery = { kind: 'send', key, submission };
                  actions.push({
                    ...action(key, 'agent', backend.kind === 'executor' ? 'execute' : 'send', {
                      sessionId: submission.context,
                      message: chunk.text,
                      attachments: submission.attachments,
                      ...(backend.kind === 'executor'
                        ? { executor: backend.selection }
                        : { model: backend.model, thinkingLevel: backend.thinkingLevel }),
                    }),
                    recovery: {
                      format: 'maka.agent.draft-send@2',
                      sessionId: submission.context,
                      editor: {
                        owner: submission.draft.snapshot.text.owner,
                        revision: submission.draft.revision,
                      },
                      attachments: submission.attachments,
                    },
                  });
                }
              } else entry.notice = 'draftKept';
            } else entry.notice = 'draftKept';
          } else if (entry && delivery?.kind === 'send') {
            if (outcome.status === 'operation')
              admitted(state, entry, delivery.submission, outcome.reply, outcome.caller, events);
            else {
              events.push(...submissionEvent(delivery.submission, 'removed'));
              entry.delivery = null;
              entry.notice = 'draftKept';
            }
          } else if (entry && delivery?.kind === 'unknown') {
            delivery.checking = null;
            if (
              outcome.status === 'succeeded' &&
              sameValue((outcome.value as OperationReply).reference, delivery.reference)
            )
              admitted(
                state,
                entry,
                delivery.submission,
                outcome.value as OperationReply,
                delivery.caller,
                events,
              );
          }
        }
        return composerFrame(state, props, events, actions);
      }
      const source =
        input.kind === 'component'
          ? state.entries.find((entry) => entry.key === input.child)
          : null;
      if (input.kind === 'component' && !source) return composerFrame(state, props);
      if (input.kind === 'component' && source && input.name === 'checkpoint') {
        const observation = input.event as EditorCheckpointObservation;
        const recovered = state.records.find(
          (record) => record.observation.owner === observation.owner,
        )?.work;
        if (observation.state.kind === 'ready' && recovered?.status === 'applying')
          source.history = { after: recovered.after, scope: recovered.scope };
        source.checkpoint =
          observation.state.kind === 'restore'
            ? { ...observation, state: { ...observation.state, metadata: null } }
            : observation;
        state.records = observeDraft(state.records, {
          context: source.context,
          dataSource: source.dataSource,
          observation,
          owners: state.entries.flatMap((entry) =>
            entry.checkpoint ? [entry.checkpoint.owner] : [],
          ),
        });
        return composerFrame(state, props);
      }
      if (input.kind === 'component' && source && input.name === 'files') {
        source.files = input.event as FilePasteObservation;
        return composerFrame(state, props, [
          { name: 'files', value: { ...source.files, dataSource: source.dataSource } },
        ]);
      }
      if (
        input.kind === 'component' &&
        source &&
        (input.name === 'changed' || input.name === 'submitted')
      ) {
        const next = input.event as EditorObservation;
        if (
          source.observation &&
          next.snapshot.text.owner === source.observation.snapshot.text.owner &&
          next.revision < source.observation.revision
        )
          return composerFrame(state, props);
        if (!sameValue(source.observation?.snapshot.selection, next.snapshot.selection))
          source.prefix.selected = 0;
        if (!sameValue(source.observation?.snapshot.text, next.snapshot.text)) {
          source.prefix.selected = 0;
          source.prefix.cached = null;
        }
        source.observation = next;
        const delivery = source.delivery,
          receipt = source.observation.receipt;
        if (
          delivery?.kind === 'clear' &&
          receipt?.owner === delivery.command.owner &&
          receipt.sequence === delivery.command.sequence
        ) {
          if (receipt.outcome.status === 'applied') advance(source, delivery.caller);
          else source.notice = 'draftKept';
          source.delivery = null;
        }
        if (input.name === 'changed') return composerFrame(state, props);
      }
      const current = state.entries.find(
        (entry) => entry.context === props.context && sameValue(entry.dataSource, props.dataSource),
      );
      const observation = current?.observation;
      let command: string | undefined;
      const shortText = current ? prefixText(current) : null;
      const completion =
        shortText !== null && observation ? slashCompletion(shortText, observation) : null;
      const skillScope = JSON.stringify([
        props.context,
        props.dataSource,
        props.backend,
        props.skillContext,
      ]);
      const choices =
        shortText === null
          ? []
          : candidates(
              props,
              shortText,
              state.skills?.scope === skillScope && state.skills.query === completion?.query
                ? state.skills.items
                : [],
              completion,
            );
      const menuOpen =
        current &&
        observation &&
        shortText !== null &&
        completion !== null &&
        current.prefix.dismissed !== observation.revision &&
        !observation.snapshot.marked &&
        props.enabled &&
        !props.busy &&
        !current.delivery;
      if (input.kind === 'component' && source === current && input.name === 'navigated') {
        const navigation = input.event as {
          key: 'up' | 'down' | 'enter' | 'tab' | 'escape';
          observation: EditorObservation;
        };
        if (!menuOpen || !sameValue(navigation.observation, observation))
          return composerFrame(state, props);
        const selected = Math.min(current.prefix.selected, Math.max(0, choices.length - 1));
        if (navigation.key === 'escape') current.prefix.dismissed = observation.revision;
        else if (navigation.key === 'up' || navigation.key === 'down')
          current.prefix.selected = choices.length
            ? (selected + (navigation.key === 'up' ? -1 : 1) + choices.length) % choices.length
            : 0;
        else if (choices[selected] && (navigation.key === 'tab' || choices[selected].invocation)) {
          if (choices[selected].invocation) rememberSkills(current, [choices[selected].invocation]);
          current.delivery = {
            kind: 'clear',
            caller: null,
            command: {
              owner: observation.snapshot.text.owner,
              sequence: ++state.serial,
              basedOn: observation.revision,
              edit: choices[selected].invocation
                ? completionEdit(completion!, shortText!, choices[selected].invocation)
                : { kind: 'replace', text: `/${choices[selected].command}` },
            },
          };
        } else if (choices[selected] && navigation.key === 'enter') command = choices[selected].id;
        if (!command) return composerFrame(state, props);
      }
      if (input.kind === 'raw' && input.key.startsWith('slash:')) {
        if (!menuOpen) return composerFrame(state, props);
        const choice = choices.find((item) => `slash:${item.id}` === input.key);
        if (choice?.invocation) {
          rememberSkills(current, [choice.invocation]);
          current.delivery = {
            kind: 'clear',
            caller: null,
            command: {
              owner: observation.snapshot.text.owner,
              sequence: ++state.serial,
              basedOn: observation.revision,
              edit: completionEdit(completion!, shortText!, choice.invocation),
            },
          };
          return composerFrame(state, props);
        }
        command = choice?.id;
        if (!command) return composerFrame(state, props);
      }
      if (input.kind === 'raw' && input.key === 'draft-retry' && current?.checkpoint) {
        if (current.delivery?.kind === 'unknown') {
          if (!current.delivery.checking) {
            const key = `receipt-${++state.serial}`;
            current.delivery.checking = key;
            actions.push(action(key, 'receipts', 'read', current.delivery.reference));
          }
        } else if (current.checkpoint.state.kind === 'failed') current.retry++;
        else retryDraft(state.records, current.checkpoint.owner);
        return composerFrame(state, props, [], actions);
      }
      if (
        (command !== undefined ||
          (input.kind === 'raw' && input.key === 'send') ||
          (input.kind === 'component' && source === current && input.name === 'submitted')) &&
        current?.checkpoint?.state.kind === 'ready' &&
        current.checkpoint.state.initialized &&
        props.enabled &&
        !props.busy &&
        !current.delivery &&
        true &&
        observation &&
        !observation.snapshot.marked
      ) {
        const attachments = props.attachments
          .filter(
            (file) =>
              file.sessionId === current.context && sameValue(file.dataSource, current.dataSource),
          )
          .map((file) => file.id);
        // Native text can paint before its changed observation reaches this component.
        const captureCurrent = input.kind === 'raw' && input.key === 'send';
        if (!captureCurrent && !observation.snapshot.text.bytes && !attachments.length)
          return composerFrame(state, props);
        const key = `${captureCurrent ? 'capture' : 'read'}-${++state.serial}`;
        current.notice = null;
        current.delivery = {
          kind: captureCurrent ? 'capture' : 'read',
          key,
          submission: {
            context: current.context,
            dataSource: current.dataSource,
            backend: props.backend,
            ...(command ? { command } : {}),
            draft: observation,
            attachments,
          },
        };
        if (captureCurrent)
          actions.push({
            ...action(key, 'reader', 'read', { start: 0, end: 0 }),
            resources: {
              text: { child: current.key, method: 'snapshot', input: { kind: 'current' } },
            },
          });
        else actions.push(snapshotRead(key, current, observation));
        return composerFrame(state, props, [], actions);
      }
      if (input.kind === 'raw' && input.key === 'attach' && props.context && !props.busy)
        return composerFrame(state, props, [{ name: 'attachments', value: {} }]);
      if (input.kind === 'raw' && input.key === 'stop' && props.running && !props.busy)
        return composerFrame(state, props, [{ name: 'stop', value: {} }]);
      return composerFrame(state, props);
    },
    capture(state, props, method, input) {
      const owner = (input as { owner?: string }).owner;
      const entry = state.entries.find((entry) =>
        method === 'file'
          ? entry.files?.selection.owner === owner
          : owner
            ? entry.observation?.snapshot.text.owner === owner
            : entry.context === props.context,
      );
      if (!entry) throw new Error('Original draft is unavailable');
      return { kind: 'forward', source: { child: entry.key, method, input } };
    },
  };
}
