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
  ComponentInput,
  EditorCommand,
  EditorObservation,
  TextChunk,
  TextareaProps,
} from '../../../packages/plugin-sdk/src/index.js';
import type { Theme } from '../../shared/contracts.js';
import { createCopy } from '../../shared/locale.js';
import { sameValue } from '../../shared/value.js';
import { type Node, type Presentation } from '../../shared/view.js';
import { intent, mutationPending, observe } from './actions.js';
import type { State } from './state.js';

export const compactCopy = createCopy({
  title: ['Compact context', '压缩上下文', '壓縮上下文'],
  help: [
    'This prompt applies to this compression only.',
    '此提示词仅用于本次压缩。',
    '此提示詞僅用於本次壓縮。',
  ],
  prompt: ['Compression prompt', '压缩提示词', '壓縮提示詞'],
  confirm: ['Compact', '开始压缩', '開始壓縮'],
  cancel: ['Cancel', '取消', '取消'],
  check: ['Check operation', '检查操作结果', '檢查操作結果'],
  unknown: [
    'The operation outcome is unknown. Check it before retrying.',
    '操作结果尚未确认，请先检查结果。',
    '操作結果尚未確認，請先檢查結果。',
  ],
  loading: ['Loading prompt…', '正在加载提示词…', '正在載入提示詞…'],
  failed: [
    'Could not compact. Check that the conversation is idle and a model is selected, then retry.',
    '无法压缩。请确认会话空闲且已选择模型，然后重试。',
    '無法壓縮。請確認會話閒置且已選擇模型，然後重試。',
  ],
});
export interface CompactState {
  sessionId: string;
  attempt: number;
  prompt: string | null;
  observation: EditorObservation | null;
  command: EditorCommand | null;
  ready: boolean;
  failed: boolean;
}
export type CompactRequest =
  | { kind: 'compactPrompt'; sessionId: string; attempt: number }
  | { kind: 'compactField'; sessionId: string; attempt: number; draft: EditorObservation }
  | { kind: 'compact'; sessionId: string };
export function canCompact(state: State) {
  return (
    !!state.selected &&
    !state.selected.archived &&
    state.selected.activity === 'idle' &&
    !!state.model &&
    !state.executor &&
    !state.run &&
    !state.compacting &&
    !mutationPending(state)
  );
}
export function openCompact(state: State, actions: Action[]) {
  if (!canCompact(state)) {
    state.notice = 'unavailable';
    return;
  }
  const sessionId = state.selected!.id,
    attempt = ++state.serial;
  state.compact = {
    sessionId,
    attempt,
    prompt: null,
    observation: null,
    command: null,
    ready: false,
    failed: false,
  };
  state.dialog = 'compact';
  intent(
    state,
    actions,
    { kind: 'compactPrompt', sessionId, attempt },
    'agent',
    'compactionPrompt',
    { sessionId },
  );
}
function initialize(state: State) {
  const edit = state.compact;
  if (edit?.prompt !== null && edit?.observation && !edit.command && !edit.ready)
    edit.command = {
      owner: edit.observation.snapshot.text.owner,
      sequence: ++state.serial,
      basedOn: edit.observation.revision,
      edit: { kind: 'reset', text: edit.prompt },
    };
}
export function compactComponent(
  state: State,
  input: Extract<ComponentInput, { kind: 'component' }>,
) {
  const edit = state.compact;
  if (input.child !== 'compact-prompt' || input.name !== 'changed' || !edit) return false;
  const observed = input.event as EditorObservation;
  if (edit.observation && edit.observation.snapshot.text.owner !== observed.snapshot.text.owner) {
    edit.command = null;
    edit.ready = false;
  }
  edit.observation = observed;
  if (
    edit.command &&
    observed.receipt?.owner === edit.command.owner &&
    observed.receipt.sequence === edit.command.sequence
  ) {
    edit.ready = observed.receipt.outcome.status === 'applied';
    edit.command = null;
    edit.failed = !edit.ready;
  } else initialize(state);
  return true;
}
export function compactConfirm(state: State, actions: Action[]) {
  const edit = state.compact,
    draft = edit?.observation;
  if (
    !edit ||
    state.dialog !== 'compact' ||
    state.selected?.id !== edit.sessionId ||
    !canCompact(state) ||
    !edit.ready ||
    edit.command ||
    !draft ||
    draft.snapshot.marked
  )
    return;
  edit.failed = false;
  intent(
    state,
    actions,
    { kind: 'compactField', sessionId: edit.sessionId, attempt: edit.attempt, draft },
    'reader',
    'read',
    { start: 0, end: draft.snapshot.text.bytes },
    {
      text: {
        child: 'compact-prompt',
        method: 'snapshot',
        input: {
          kind: 'at_revision',
          owner: draft.snapshot.text.owner,
          revision: draft.snapshot.text.revision,
        },
      },
    },
  );
}
export function compactQuery(
  state: State,
  actions: Action[],
  request: CompactRequest,
  value: unknown,
) {
  const edit = state.compact;
  if (
    !edit ||
    request.kind === 'compact' ||
    edit.attempt !== request.attempt ||
    edit.sessionId !== request.sessionId ||
    state.selected?.id !== request.sessionId
  )
    return;
  if (request.kind === 'compactPrompt') {
    edit.prompt = (value as { compactionPrompt: string }).compactionPrompt;
    initialize(state);
  } else {
    const chunk = value as TextChunk;
    if (
      !sameValue(chunk.metadata, request.draft.snapshot.text) ||
      !sameValue(edit.observation?.snapshot.text, request.draft.snapshot.text) ||
      !chunk.text.trim() ||
      new TextEncoder().encode(chunk.text).length !== request.draft.snapshot.text.bytes ||
      !canCompact(state)
    ) {
      edit.failed = true;
      return;
    }
    intent(state, actions, { kind: 'compact', sessionId: edit.sessionId }, 'agent', 'compact', {
      sessionId: edit.sessionId,
      prompt: chunk.text,
      model: state.model!.choice.selection,
      thinkingLevel: state.model!.thinkingLevel,
    });
  }
}
export function compactSettled(state: State, actions: Action[], sessionId: string, value: unknown) {
  if (state.selected?.id !== sessionId) return;
  if ((value as { kind: string }).kind === 'started') {
    state.compact = null;
    if (state.dialog === 'compact') state.dialog = null;
    state.notice = null;
  } else if (state.compact) state.compact.failed = true;
  observe(state, actions);
}
export function compactView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
): N {
  const ui = presentation.paint(theme),
    text = compactCopy(locale),
    edit = state.compact;
  const busy = mutationPending(state);
  return ui.box(
    'compact-content',
    [
      ui.text('compact-help', text.help, theme.muted),
      ...(edit?.prompt === null ? [ui.text('compact-loading', text.loading)] : []),
      presentation.slot(
        'compact-prompt',
        'editor',
        {
          menu: editorMenu(theme, locale),
          accessibilityLabel: text.prompt,
          placeholder: '',
          autoFocus: true,
          tabIndex: 1,
          readOnly: false,
          disabled: busy || !edit?.ready || !!edit.command,
          command: edit?.command ?? null,
          appearance: theme.textarea,
          softWrap: true,
          terminalRows: { min: 6, max: 12 },
        } satisfies TextareaProps,
        { changed: 'changed' },
      ),
      ...(edit?.failed ? [ui.text('compact-error', text.failed, theme.warning)] : []),
      ...(state.uncertain?.reference.method === 'compact'
        ? [
            ui.text('compact-unknown', text.unknown, theme.warning),
            ui.button(
              'compact-check',
              text.check,
              Object.values(state.pending).some((request) => request.kind === 'receipt'),
            ),
          ]
        : []),
      ui.box(
        'compact-controls',
        [
          ui.button(
            'compact-confirm',
            text.confirm,
            busy || !edit?.ready || !!edit.command || !canCompact(state),
          ),
          ui.button('compact-cancel', text.cancel, busy),
        ],
        { row: true, gap: 1 },
      ),
    ],
    { gap: 1, terminal: { grow: 1, shrink: true } },
  );
}
