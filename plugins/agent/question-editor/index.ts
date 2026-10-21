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
  Component,
  EditorObservation,
  Emission,
  TextareaProps,
} from '../../../packages/plugin-sdk/src/index.js';
import {
  questionEditorContract,
  type QuestionEditorProps,
  type QuestionDraft,
} from '../../shared/contracts.js';
import { copy } from './locale.js';
import { output, type Node, type Presentation } from '../../shared/view.js';

type Draft = {
  selection: 'unset' | 'text' | 'skip' | number;
  observation: EditorObservation | null;
};
type State = { revision: number; drafts: Draft[] };
export function questionEditor<N extends Node>(
  presentation: Presentation<N>,
): Component<State, QuestionEditorProps, N> {
  const questions = (props: QuestionEditorProps) =>
    props.record?.request.kind === 'question' ? props.record.request.questions : [];
  const editable = (props: QuestionEditorProps) =>
    props.open && !props.busy && props.record?.request.kind === 'question' && !props.record.outcome;
  const answer = (state: State, index: number): QuestionDraft | null => {
    const draft = state.drafts[index];
    if (typeof draft.selection === 'number') return { kind: 'option', index: draft.selection };
    if (draft.selection === 'skip') return { kind: 'skip' };
    if (
      draft.selection === 'text' &&
      draft.observation &&
      !draft.observation.snapshot.marked &&
      draft.observation.snapshot.text.bytes > 0
    )
      return { kind: 'text', observation: draft.observation };
    return null;
  };
  function frame(state: State, props: QuestionEditorProps, events: Emission[] = []) {
    const ui = presentation.paint(props.theme),
      text = copy(props.locale);
    const available = editable(props),
      qs = questions(props);
    const done = qs.every((_, index) => answer(state, index) !== null);
    const outcome = props.record?.outcome;
    return output(
      state,
      ui.box(
        'question-editor',
        [
          ui.box(
            'question-controls',
            [ui.button('close', text.closeModels), ui.button('check', text.check, props.checking)],
            { row: true, gap: 1, terminal: { wrap: true } },
          ),
          ...(props.status ? [ui.text('question-status', props.status, props.theme.muted)] : []),
          ...(outcome
            ? [
                ui.text(
                  'question-outcome',
                  outcome.kind === 'question_answer' ? text.questionAnswered : text.questionClosed,
                  props.theme.muted,
                ),
              ]
            : []),
          ui.box(
            'question-pages',
            qs.map((question, index) => {
              const draft = state.drafts[index];
              const choices = question.options.map((option, at) => {
                const key = `option-${index}-${at}`,
                  label = `${draft.selection === at ? '●' : '○'} ${option.label}`;
                const content = ui.box(
                  `${key}-content`,
                  [
                    ui.text(`${key}-label`, label),
                    ...(option.description
                      ? [ui.text(`${key}-description`, option.description, props.theme.muted)]
                      : []),
                  ],
                  { gap: 0 },
                );
                return ui.item(key, content, draft.selection === at, !available);
              });
              return ui.box(
                `question-page-${index}`,
                [
                  ui.text(`question-text-${index}`, question.question),
                  ui.box(`question-options-${index}`, choices, { gap: 1, focusGroup: true }),
                  ui.box(
                    `question-modes-${index}`,
                    [
                      ui.button(
                        `text-${index}`,
                        `${draft.selection === 'text' ? '● ' : ''}${text.questionText}`,
                        !available,
                      ),
                      ui.button(
                        `skip-${index}`,
                        `${draft.selection === 'skip' ? '● ' : ''}${text.questionSkip}`,
                        !available,
                      ),
                    ],
                    { row: true, gap: 1, terminal: { wrap: true } },
                  ),
                  ui.box(
                    `question-field-${index}`,
                    [
                      presentation.slot(
                        `answer-${index}`,
                        'editor',
                        {
                          menu: editorMenu(props.theme, props.locale),
                          appearance: props.theme.textarea,
                          accessibilityLabel: text.questionText,
                          placeholder: text.questionText,
                          readOnly: !available,
                          disabled: false,
                          autoFocus: false,
                          initialText: '',
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
                  ...(outcome?.kind === 'question_answer'
                    ? [
                        ui.text(
                          `recorded-${index}`,
                          outcome.answers[index] ?? text.questionSkip,
                          props.theme.muted,
                        ),
                      ]
                    : []),
                ],
                { gap: 1 },
              );
            }),
            { scroll: 'anchor', gap: 0, terminal: { grow: 1, basis: 0 } },
          ),
          ui.button('submit', text.questionSubmit, !available || !done, true),
        ],
        { visible: props.open, gap: 0, terminal: { grow: 1 } },
      ),
      events,
    );
  }
  return {
    mount: (props) =>
      frame(
        {
          revision: 0,
          drafts: questions(props).map(() => ({ selection: 'unset', observation: null })),
        },
        props,
      ),
    update: (state, props) => frame({ ...state, revision: state.revision + 1 }, props),
    capture(_state, props, method, input) {
      const request = input as { requestId: string; index: number; snapshot: unknown };
      if (
        method !== 'snapshot' ||
        request.requestId !== props.record?.requestId ||
        !Number.isInteger(request.index) ||
        !questions(props)[request.index]
      )
        throw new Error('question_capture_changed');
      return {
        kind: 'forward',
        source: { child: `answer-${request.index}`, method: 'snapshot', input: request.snapshot },
      };
    },
    event(state, input, props) {
      state.revision++;
      const events: Emission[] = [];
      let submit = false;
      if (input.kind === 'component' && input.child.startsWith('answer-')) {
        const index = Number(input.child.slice(7)),
          draft = state.drafts[index];
        if (draft && (input.name === 'changed' || input.name === 'submitted')) {
          const observation = input.event as EditorObservation;
          if (
            editable(props) &&
            draft.observation &&
            observation.snapshot.text.revision !== draft.observation.snapshot.text.revision
          )
            draft.selection = 'text';
          draft.observation = observation;
          submit = input.name === 'submitted';
        }
      } else if (input.kind === 'raw' && props.open) {
        if (input.key === 'close') events.push({ name: 'closed', value: {} });
        else if (input.key === 'check' && !props.checking)
          events.push({ name: 'check', value: {} });
        else if (editable(props)) {
          const option = /^option-(\d+)-(\d+)$/.exec(input.key);
          const mode = /^(text|skip)-(\d+)$/.exec(input.key);
          if (option && questions(props)[Number(option[1])]?.options[Number(option[2])])
            state.drafts[Number(option[1])].selection = Number(option[2]);
          else if (mode && questions(props)[Number(mode[2])])
            state.drafts[Number(mode[2])].selection = mode[1] as 'text' | 'skip';
          else submit = input.key === 'submit';
        }
      }
      if (submit && editable(props)) {
        const answers = questions(props).map((_, index) => answer(state, index));
        if (answers.every((value): value is QuestionDraft => value !== null))
          events.push({ name: 'submit', value: { requestId: props.record!.requestId, answers } });
      }
      return frame(state, props, events);
    },
  };
}
export function activate() {
  return {
    QuestionEditor: {
      kind: 'component',
      contract: questionEditorContract,
      component: ['component'],
    },
  };
}
