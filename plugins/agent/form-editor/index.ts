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
  ActionOutcome,
  Component,
  EditorObservation,
  Emission,
  InteractionAnswer,
  InteractionRecord,
  TextChunk,
  TextareaProps,
} from '../../../packages/plugin-sdk/src/index.js';
import { formEditorContract, type QuestionEditorProps } from '../../shared/contracts.js';
import { copy } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { output, type Node, type Presentation } from '../../shared/view.js';

type Request = Extract<InteractionRecord['request'], { kind: 'form' }>;
type Field = Request['fields'][number];
type Value = string | number | boolean | string[];
type Draft = { included: boolean; value: Value | null; observation: EditorObservation | null };
type Read = { index: number; captured: EditorObservation };
type State = {
  revision: number;
  serial: number;
  request: Request | null;
  requestId: string;
  invalidField: number | null;
  drafts: Draft[];
  pending: Record<string, Read>;
  reads: Read[];
  submission: Record<string, Value> | null;
  notice: 'formInvalid' | 'formReadFailed' | null;
};
const textual = (field: Field) => ['string', 'number', 'integer'].includes(field.kind);
const bytes = (value: string) =>
  [...value].reduce(
    (n, c) =>
      n +
      (c.codePointAt(0)! <= 0x7f
        ? 1
        : c.codePointAt(0)! <= 0x7ff
          ? 2
          : c.codePointAt(0)! <= 0xffff
            ? 3
            : 4),
    0,
  );
function available(state: State, props: QuestionEditorProps) {
  return (
    props.open &&
    !props.busy &&
    !props.record.outcome &&
    props.record.requestId === state.requestId &&
    sameValue(props.record.request, state.request)
  );
}
function valid(field: Field, value: Value | null): value is Value {
  switch (field.kind) {
    case 'string':
      return (
        typeof value === 'string' &&
        bytes(value) <= 2048 &&
        [...value].length >= (field.minLength ?? 0) &&
        [...value].length <= (field.maxLength ?? 2048)
      );
    case 'number':
    case 'integer':
      return (
        typeof value === 'number' &&
        Number.isFinite(value) &&
        value >= (field.minimum ?? -Infinity) &&
        value <= (field.maximum ?? Infinity) &&
        (field.kind !== 'integer' || Number.isSafeInteger(value))
      );
    case 'boolean':
      return typeof value === 'boolean';
    case 'single_select':
      return typeof value === 'string' && field.options.some((o) => o.value === value);
    case 'multi_select':
      return (
        Array.isArray(value) &&
        value.length >= (field.minItems ?? 0) &&
        value.length <= (field.maxItems ?? field.options.length) &&
        new Set(value).size === value.length &&
        value.every((v) => field.options.some((o) => o.value === v))
      );
  }
}
function finish(state: State, props: QuestionEditorProps, events: Emission[]) {
  if (!state.submission || state.reads.length || Object.keys(state.pending).length) return;
  const values = state.submission;
  state.submission = null;
  if (!available(state, props)) return;
  // Preserve the canonical answer/outcome byte ceiling, including escaped text.
  if (
    bytes(
      JSON.stringify({
        kind: 'form_answer',
        action: 'accept',
        values,
        committedAt: Number.MAX_SAFE_INTEGER,
      }),
    ) > 8192
  ) {
    state.notice = 'formInvalid';
    state.invalidField = null;
    return;
  }
  events.push({
    name: 'submit',
    value: { requestId: state.requestId, answer: { kind: 'form', action: 'accept', values } },
  });
}
function submit(state: State, props: QuestionEditorProps, actions: Action[], events: Emission[]) {
  if (!available(state, props) || state.submission) return;
  const values: Record<string, Value> = Object.create(null);
  const reads: Read[] = [];
  for (const [index, field] of (state.request?.fields ?? []).entries()) {
    const draft = state.drafts[index];
    if (!draft.included) continue;
    if (textual(field)) {
      const captured = draft.observation;
      if (!captured || captured.snapshot.marked || captured.snapshot.text.bytes > 8192) {
        state.notice = 'formInvalid';
        state.invalidField = index;
        return;
      }
      reads.push({ index, captured });
    } else if (valid(field, draft.value)) values[field.name] = draft.value;
    else {
      state.notice = 'formInvalid';
      state.invalidField = index;
      return;
    }
  }
  state.notice = null;
  state.invalidField = null;
  state.submission = values;
  state.reads = reads;
  readNext(state, actions);
  finish(state, props, events);
}
// A form may contain 32 fields; native component output admits at most 16 actions.
// Reading one immutable editor snapshot at a time also keeps response buffers bounded.
function readNext(state: State, actions: Action[]) {
  const read = state.reads.shift();
  if (!read) return;
  const key = `form-read-${++state.serial}`;
  state.pending[key] = read;
  actions.push({
    key,
    binding: 'reader',
    member: 0,
    method: 'read',
    input: { start: 0, end: read.captured.snapshot.text.bytes },
    resources: {
      text: {
        child: `field-${read.index}`,
        method: 'snapshot',
        input: {
          kind: 'at_revision',
          owner: read.captured.snapshot.text.owner,
          revision: read.captured.snapshot.text.revision,
        },
      },
    },
  });
}
export function formEditor<N extends Node>(
  presentation: Presentation<N>,
): Component<State, QuestionEditorProps, N> {
  function frame(
    state: State,
    props: QuestionEditorProps,
    actions: Action[] = [],
    events: Emission[] = [],
  ) {
    const ui = presentation.paint(props.theme),
      text = copy(props.locale);
    const enabled = available(state, props) && !state.submission;
    const fields = state.request?.fields ?? [];
    const outcome = props.record.outcome;
    const result = output(
      state,
      ui.box(
        'form-editor',
        [
          ui.box(
            'controls',
            [ui.button('close', text.closeModels), ui.button('check', text.check, props.checking)],
            { row: true, gap: 1, terminal: { wrap: true } },
          ),
          ...(props.status ? [ui.text('status', props.status, props.theme.muted)] : []),
          ...(state.notice
            ? [
                ui.text(
                  'notice',
                  text[state.notice] +
                    (state.invalidField !== null ? ': ' + fields[state.invalidField].label : ''),
                  props.theme.muted,
                ),
              ]
            : []),
          ...(outcome
            ? [
                ui.text(
                  'outcome',
                  outcome.kind === 'form_answer'
                    ? outcome.action === 'accept'
                      ? text.questionAnswered
                      : outcome.action === 'decline'
                        ? text.formDecline
                        : text.formCancel
                    : text.questionClosed,
                  props.theme.muted,
                ),
              ]
            : []),
          ui.box(
            'pages',
            [
              ui.text('requester', state.request?.requester.name ?? '', props.theme.muted),
              ...(state.request?.requester.source
                ? [ui.text('requester-source', state.request.requester.source, props.theme.muted)]
                : []),
              ui.text('message', state.request?.message ?? ''),
              ...fields.map((field, index) => {
                const draft = state.drafts[index];
                const editing = enabled && draft.included;
                const children: N[] = [
                  ui.text(
                    `label-${index}`,
                    `${field.label}${field.required ? ` · ${text.formRequired}` : ''}`,
                  ),
                ];
                if (field.description)
                  children.push(
                    ui.text(`description-${index}`, field.description, props.theme.muted),
                  );
                const limits = textual(field)
                  ? [
                      ...('minLength' in field && field.minLength !== undefined
                        ? [`≥ ${field.minLength}`]
                        : []),
                      ...('maxLength' in field && field.maxLength !== undefined
                        ? [`≤ ${field.maxLength}`]
                        : []),
                      ...('minimum' in field && field.minimum !== undefined
                        ? [`≥ ${field.minimum}`]
                        : []),
                      ...('maximum' in field && field.maximum !== undefined
                        ? [`≤ ${field.maximum}`]
                        : []),
                      ...('format' in field && field.format ? [field.format] : []),
                    ].join(' · ')
                  : field.kind === 'multi_select'
                    ? `${field.minItems ?? 0}–${field.maxItems ?? field.options.length}`
                    : '';
                if (limits) children.push(ui.text(`limits-${index}`, limits, props.theme.muted));
                if (!field.required)
                  children.push(
                    ui.button(
                      `include-${index}`,
                      draft.included ? text.formOmit : text.formInclude,
                      !enabled,
                    ),
                  );
                if (textual(field))
                  children.push(
                    ui.box(
                      `editor-box-${index}`,
                      [
                        presentation.slot(
                          `field-${index}`,
                          'editor',
                          {
                            menu: editorMenu(props.theme, props.locale),
                            appearance: props.theme.textarea,
                            accessibilityLabel: field.label,
                            placeholder: field.label,
                            readOnly: !editing,
                            disabled: false,
                            autoFocus: false,
                            initialText: field.default === undefined ? '' : String(field.default),
                            softWrap: true,
                            terminalRows: { min: 2, max: 4 },
                            tabIndex: 0,
                            submit: 'primary_enter',
                            command: null,
                          } satisfies TextareaProps,
                          { changed: 'changed', submitted: 'submit' },
                        ),
                      ],
                      { terminal: { shrink: true } },
                    ),
                  );
                else if (field.kind === 'boolean')
                  children.push(
                    ui.box(
                      `boolean-${index}`,
                      [true, false].map((v) =>
                        ui.button(
                          `boolean-${index}-${v}`,
                          `${draft.value === v ? '● ' : '○ '}${v ? text.formYes : text.formNo}`,
                          !editing,
                        ),
                      ),
                      { row: true, gap: 1 },
                    ),
                  );
                else if (field.kind === 'single_select' || field.kind === 'multi_select')
                  children.push(
                    ui.box(
                      `options-${index}`,
                      field.options.map((option, at) => {
                        const selected =
                          field.kind === 'multi_select'
                            ? Array.isArray(draft.value) && draft.value.includes(option.value)
                            : draft.value === option.value;
                        return ui.button(
                          `option-${index}-${at}`,
                          `${selected ? '● ' : '○ '}${option.label}`,
                          !editing,
                        );
                      }),
                      { gap: 1, focusGroup: true },
                    ),
                  );
                if (
                  outcome?.kind === 'form_answer' &&
                  outcome.action === 'accept' &&
                  Object.hasOwn(outcome.values, field.name)
                )
                  children.push(
                    ui.text(
                      `recorded-${index}`,
                      JSON.stringify(outcome.values[field.name]),
                      props.theme.muted,
                    ),
                  );
                return ui.box(`page-content-${index}`, children, { gap: 1 });
              }),
            ],
            { scroll: 'anchor', terminal: { grow: 1, basis: 0 } },
          ),
          ui.box(
            'decisions',
            [
              ui.button('submit', text.formSubmit, !enabled, true),
              ui.button('decline', text.formDecline, !enabled),
              ui.button('cancel', text.formCancel, !enabled),
            ],
            { row: true, gap: 1, terminal: { wrap: true } },
          ),
        ],
        { visible: props.open, gap: 1, terminal: { grow: 1 } },
      ),
      events,
    );
    result.actions = actions;
    return result;
  }
  return {
    mount(props) {
      const request = props.record.request.kind === 'form' ? props.record.request : null;
      return frame(
        {
          revision: 0,
          serial: 0,
          request,
          requestId: props.record.requestId,
          invalidField: null,
          drafts: (request?.fields ?? []).map((f) => ({
            included: f.required || f.default !== undefined,
            value: f.default ?? (f.kind === 'multi_select' ? [] : null),
            observation: null,
          })),
          pending: {},
          reads: [],
          submission: null,
          notice: null,
        },
        props,
      );
    },
    update(state, props) {
      state.revision++;
      if (!available(state, props)) {
        state.submission = null;
        state.pending = {};
        state.reads = [];
      }
      return frame(state, props);
    },
    event(state, input, props) {
      state.revision++;
      const actions: Action[] = [],
        events: Emission[] = [];
      if (input.kind === 'component' && /^field-\d+$/.test(input.child)) {
        const index = Number(input.child.slice(6)),
          draft = state.drafts[index];
        if (draft && (input.name === 'changed' || input.name === 'submit')) {
          draft.observation = input.event as EditorObservation;
          if (input.name === 'submit') submit(state, props, actions, events);
        }
      } else if (input.kind === 'action') {
        const read = state.pending[input.key];
        if (read && state.submission) {
          delete state.pending[input.key];
          const outcome = input.event as ActionOutcome;
          const chunk = outcome.status === 'succeeded' ? (outcome.value as TextChunk) : null;
          if (
            !chunk ||
            !sameValue(chunk.metadata, read.captured.snapshot.text) ||
            bytes(chunk.text) !== read.captured.snapshot.text.bytes ||
            !sameValue(
              state.drafts[read.index].observation?.snapshot.text,
              read.captured.snapshot.text,
            ) ||
            state.drafts[read.index].observation?.snapshot.marked
          ) {
            state.submission = null;
            state.pending = {};
            state.reads = [];
            state.notice = 'formReadFailed';
          } else {
            const field = state.request!.fields[read.index];
            const value =
              field.kind === 'string'
                ? chunk.text
                : /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/.test(chunk.text.trim())
                  ? Number(chunk.text)
                  : null;
            if (!valid(field, value)) {
              state.submission = null;
              state.pending = {};
              state.reads = [];
              state.notice = 'formInvalid';
              state.invalidField = read.index;
            } else {
              Object.defineProperty(state.submission, field.name, {
                value,
                enumerable: true,
                configurable: true,
                writable: true,
              });
              readNext(state, actions);
              finish(state, props, events);
            }
          }
        }
      } else if (input.kind === 'raw' && props.open) {
        if (input.key === 'close') {
          state.submission = null;
          state.pending = {};
          state.reads = [];
          events.push({ name: 'closed', value: {} });
        } else if (input.key === 'check' && !props.checking)
          events.push({ name: 'check', value: {} });
        else if (available(state, props) && !state.submission) {
          const include = /^include-(\d+)$/.exec(input.key),
            boolean = /^boolean-(\d+)-(true|false)$/.exec(input.key),
            option = /^option-(\d+)-(\d+)$/.exec(input.key);
          if (include) {
            const index = Number(include[1]);
            if (state.request?.fields[index] && !state.request.fields[index].required)
              state.drafts[index].included = !state.drafts[index].included;
          } else if (boolean) {
            const index = Number(boolean[1]);
            if (state.request?.fields[index]?.kind === 'boolean' && state.drafts[index].included)
              state.drafts[index].value = boolean[2] === 'true';
          } else if (option) {
            const index = Number(option[1]),
              field = state.request?.fields[index],
              draft = state.drafts[index];
            if (
              draft?.included &&
              (field?.kind === 'single_select' || field?.kind === 'multi_select')
            ) {
              const chosen = field.options[Number(option[2])]?.value;
              if (chosen !== undefined)
                draft.value =
                  field.kind === 'single_select'
                    ? chosen
                    : Array.isArray(draft.value) && draft.value.includes(chosen)
                      ? draft.value.filter((v) => v !== chosen)
                      : [...(Array.isArray(draft.value) ? draft.value : []), chosen];
            }
          } else if (input.key === 'submit') submit(state, props, actions, events);
          else if (input.key === 'decline' || input.key === 'cancel')
            events.push({
              name: 'submit',
              value: {
                requestId: state.requestId,
                answer: { kind: 'form', action: input.key } satisfies InteractionAnswer,
              },
            });
        }
      }
      return frame(state, props, actions, events);
    },
  };
}
export function activate() {
  return {
    FormEditor: { kind: 'component', contract: formEditorContract, component: ['component'] },
  };
}
