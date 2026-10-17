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
  Component,
  EditorObservation,
  Emission,
  TextareaProps,
} from '../../../packages/plugin-sdk/src/index.js';
import { queueEditorContract, type QueueEditorProps } from '../../shared/contracts.js';
import { copy } from './locale.js';
import { output, type Node, type Presentation } from '../../shared/view.js';

type State = { revision: number; observation: EditorObservation | null };
export function queueEditor<N extends Node>(
  presentation: Presentation<N>,
): Component<State, QueueEditorProps, N> {
  const available = (state: State, props: QueueEditorProps) =>
    props.open &&
    props.ready &&
    !props.busy &&
    !props.command &&
    !!state.observation &&
    !state.observation.snapshot.marked &&
    (state.observation.snapshot.text.bytes > 0 || props.hasAttachments) &&
    state.observation.snapshot.text.bytes <= 65536;
  function frame(state: State, props: QueueEditorProps, events: Emission[] = []) {
    const ui = presentation.paint(props.theme),
      text = copy(props.locale);
    const field: TextareaProps = {
      appearance: props.theme.textarea,
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
      accessibilityLabel: text.queueEdit,
      readOnly: !props.ready || props.busy,
      disabled: false,
      softWrap: true,
      terminalRows: { min: 3, max: 10 },
      submit: 'primary_enter',
      tabIndex: 0,
      autoFocus: true,
      initialText: '',
      placeholder: '',
      command: props.command,
    };
    return output(
      state,
      ui.box(
        'queue-editor',
        [
          ui.box(
            'field',
            [
              presentation.slot('draft', 'editor', field, {
                changed: 'changed',
                submitted: 'save',
              }),
            ],
            { terminal: { shrink: true } },
          ),
          ui.text('hint', text.queueEditHint, props.theme.muted),
          ...(props.status ? [ui.text('status', props.status, props.theme.muted)] : []),
          ui.box(
            'controls',
            [
              ui.button('close', text.cancel),
              ui.button('reload', text.detailsReload, props.busy),
              ...(props.uncertain ? [ui.button('check', text.check, props.checking)] : []),
              ui.button('save', text.queueSave, !available(state, props), true),
            ],
            { row: true, terminal: { wrap: true } },
          ),
        ],
        {
          visible: props.open,
          terminal: { grow: 1 },
          shortcuts: [
            { key: 's', modifiers: ['primary'], input: 'save', enabled: available(state, props) },
          ],
        },
      ),
      events,
    );
  }
  return {
    mount: (props) => frame({ revision: 0, observation: null }, props),
    update: (state, props) => frame({ ...state, revision: state.revision + 1 }, props),
    capture(_state, _props, method, input) {
      return { kind: 'forward', source: { child: 'draft', method, input } };
    },
    event(state, input, props) {
      state.revision++;
      const events: Emission[] = [];
      if (input.kind === 'component' && input.child === 'draft') {
        state.observation = input.event as EditorObservation;
        events.push({ name: 'changed', value: state.observation });
        if (input.name === 'submitted' && available(state, props))
          events.push({ name: 'save', value: state.observation });
      } else if (props.open && input.kind === 'raw') {
        if (input.key === 'save' && available(state, props))
          events.push({ name: 'save', value: state.observation });
        else if (input.key === 'close') events.push({ name: 'closed', value: {} });
        else if (input.key === 'reload' && !props.busy) events.push({ name: 'reload', value: {} });
        else if (input.key === 'check' && props.uncertain && !props.checking)
          events.push({ name: 'check', value: {} });
      }
      return frame(state, props, events);
    },
  };
}
export function activate() {
  return {
    QueueEditor: { kind: 'component', contract: queueEditorContract, component: ['component'] },
  };
}
