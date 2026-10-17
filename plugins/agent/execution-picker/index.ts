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
  AgentExecutorSelection,
  AgentExecutors,
  Component,
  EditorObservation,
  EditorCommand,
  InputProps,
  EditorTextRef,
  Emission,
  TextChunk,
  ThinkingLevel,
  ExecutorSetupAction,
  ExecutorSetupResult,
  ExecutorSetupProgress,
  OperationReply,
  RecoveryRef,
} from '../../../packages/plugin-sdk/src/index.js';
import { executionPickerContract, type ExecutionPickerProps } from '../../shared/contracts.js';
import { copy, type Copy } from './locale.js';
import { output, type Node, type Presentation } from '../../shared/view.js';
interface State {
  revision: number;
  serial: number;
  openRequest: number | null;
  modelRequest: number | null;
  open: boolean;
  names: string[];
  setups: string[];
  setup: {
    executor: string;
    key: string | null;
    receiptKey: string | null;
    reference: RecoveryRef | null;
    result: ExecutorSetupResult | null;
    url: string | null;
    cancelling: boolean;
    unresolved: boolean;
    settled: boolean;
  } | null;
  listing: string | null;
  failed: boolean;
  draft: AgentExecutorSelection | null;
  editor: string;
  observation: EditorObservation | null;
  prefill: string | null;
  command: EditorCommand | null;
  capture: { key: string; basis: EditorTextRef; selection: AgentExecutorSelection } | null;
  error: boolean;
}
const levels: Record<ThinkingLevel, keyof Copy> = {
  off: 'thinkingOff',
  minimal: 'thinkingMinimal',
  low: 'thinkingLow',
  medium: 'thinkingMedium',
  high: 'thinkingHigh',
  xhigh: 'thinkingXhigh',
  max: 'thinkingMax',
  ultra: 'thinkingUltra',
};
function list(state: State): Action[] {
  if (state.listing) return [];
  state.failed = false;
  state.listing = `backends-${++state.serial}`;
  return [
    {
      key: state.listing,
      binding: 'agent',
      member: 0,
      method: 'executors',
      input: {},
      resources: {},
    },
  ];
}
function close(state: State) {
  state.open = false;
  state.capture = null;
}
function draft(state: State, selection: AgentExecutorSelection | null) {
  state.draft = selection ? { ...selection, settings: { ...selection.settings } } : null;
  state.editor = `backend-model-${++state.serial}`;
  state.observation = null;
  state.prefill = selection?.settings.model || null;
  state.command = null;
  state.capture = null;
  state.error = false;
}
function open(state: State, props: ExecutionPickerProps) {
  state.open = true;
  if (!state.setup?.key && !state.setup?.receiptKey && !state.setup?.unresolved)
    draft(state, props.executor);
}
function utf8Bytes(text: string) {
  let bytes = 0;
  for (const point of text) {
    const code = point.codePointAt(0)!;
    bytes += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
  }
  return bytes;
}
function same(a: EditorTextRef, b: EditorTextRef | undefined) {
  return !!b && a.owner === b.owner && a.revision === b.revision && a.bytes === b.bytes;
}
function capture(state: State): Action[] {
  const observation = state.observation,
    selection = state.draft;
  if (!selection || state.capture || state.prefill !== null || state.command || !observation)
    return [];
  const basis = observation.snapshot.text;
  if (observation.snapshot.marked || basis.bytes > 512) {
    state.error = true;
    return [];
  }
  const key = `settings-${++state.serial}`;
  state.capture = { key, basis, selection: { ...selection, settings: { ...selection.settings } } };
  return [
    {
      key,
      binding: 'reader',
      member: 0,
      method: 'read',
      input: { start: 0, end: basis.bytes },
      resources: {
        text: {
          child: state.editor,
          method: 'snapshot',
          input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
        },
      },
    },
  ];
}
function setupAction(state: State, action: ExecutorSetupAction): Action[] {
  const executor = state.draft?.executor;
  if (
    !executor ||
    !state.setups.includes(executor) ||
    state.setup?.key ||
    state.setup?.receiptKey ||
    (state.setup?.unresolved && !(action.kind === 'check' && state.setup.settled))
  )
    return [];
  const key = `setup-${++state.serial}`;
  state.setup = {
    executor,
    key,
    receiptKey: null,
    reference: null,
    result: null,
    url: null,
    cancelling: false,
    unresolved: false,
    settled: false,
  };
  return [
    {
      key,
      binding: 'agent',
      member: 0,
      method: 'executorSetup',
      input: { executor, action },
      resources: {},
      recovery: { executor, action },
    },
  ];
}
function setupReceipt(state: State, reply: OperationReply) {
  const setup = state.setup;
  if (!setup) return;
  if (setup.reference && JSON.stringify(setup.reference) !== JSON.stringify(reply.reference))
    return;
  setup.reference = reply.reference;
  const receipt = reply.receipt;
  setup.settled =
    receipt.kind === 'recorded' &&
    receipt.cleanup.kind === 'confirmed' &&
    receipt.outcome.kind !== 'unresolved';
  setup.result =
    receipt.kind === 'recorded' && receipt.outcome.kind === 'returned'
      ? (receipt.outcome.value as ExecutorSetupResult)
      : null;
  setup.unresolved =
    !setup.result ||
    setup.result.kind === 'unresolved' ||
    receipt.kind !== 'recorded' ||
    receipt.cleanup.kind !== 'confirmed';
}
function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: ExecutionPickerProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const disabled =
    props.busy ||
    !!state.capture ||
    !!state.setup?.key ||
    !!state.setup?.receiptKey ||
    (!!state.setup?.unresolved && !state.setup.settled);
  const model = presentation.slot(
    'models',
    'models',
    {
      catalogEpoch: props.catalogEpoch,
      openRequest: state.modelRequest,
      theme: props.theme,
      locale: props.locale,
      selection: props.selection,
      selectionKey: props.selectionKey ?? '',
      preferredModel: props.preferredModel ?? null,
      suggestDefault: props.suggestDefault ?? true,
      busy: props.busy || state.open || props.executor !== null,
    },
    { selected: 'selected', resolved: 'resolved', setup: 'setup', connections: 'connections' },
  );
  const background = ui.box(
    'backend-control',
    [
      ui.box('connected-model', [model], { visible: props.executor === null, gap: 0 }),
      ...(state.names.length || state.failed || props.executor
        ? [
            ui.button(
              'backends',
              props.executor?.executor ?? text.executionBackend,
              props.busy && !state.setup?.key && !state.setup?.unresolved,
            ),
          ]
        : []),
    ],
    { row: true, gap: 1 },
  );
  const options = [
    { key: 'default', label: text.executionDefault },
    ...Object.entries(levels).map(([key, value]) => ({ key, label: text[value] })),
  ];
  const current = state.draft?.settings.thinkingLevel ?? 'default';
  const fields = ui.box(
    'backend-fields',
    [
      ui.button('connected', text.executionModel, disabled),
      ...state.names.map((name, i) =>
        ui.button(
          `backend-${i}`,
          `${state.draft?.executor === name ? (props.theme.ascii ? '> ' : '✓ ') : ''}${name}`,
          disabled,
        ),
      ),
      ...(state.failed
        ? [ui.text('backend-error', text.executionUnavailable, props.theme.error)]
        : []),
      ...(state.draft
        ? [
            ...(state.setups.includes(state.draft.executor)
              ? [
                  ui.button(
                    'setup-check',
                    text.executionCheck,
                    disabled || (!!state.setup?.unresolved && !state.setup.settled),
                  ),
                  ...(state.setup?.executor === state.draft.executor
                    ? [
                        ...(state.setup.result?.kind === 'checked'
                          ? [
                              ...(state.setup.result.info
                                ? [
                                    ui.text(
                                      'setup-info',
                                      `${state.setup.result.info.title ?? state.setup.result.info.name} ${state.setup.result.info.version}`,
                                    ),
                                  ]
                                : []),
                              ...(state.setup.result.methods.length
                                ? state.setup.result.methods.flatMap((method, index) => [
                                    ui.button(
                                      `setup-auth-${index}`,
                                      `${text.executionAuthenticate}: ${method.name}`,
                                      disabled || !method.supported || !!state.setup?.unresolved,
                                    ),
                                    ...(method.description
                                      ? [
                                          ui.text(
                                            `setup-description-${index}`,
                                            method.description,
                                            props.theme.muted,
                                          ),
                                        ]
                                      : []),
                                    ...(!method.supported
                                      ? [
                                          ui.text(
                                            `setup-unsupported-${index}`,
                                            text.executionUnsupported,
                                            props.theme.muted,
                                          ),
                                        ]
                                      : []),
                                  ])
                                : [ui.text('setup-none', text.executionNoAuth)]),
                            ]
                          : []),
                        ...(state.setup.key
                          ? [
                              ui.text(
                                'setup-pending',
                                state.setup.cancelling
                                  ? text.executionCancelling
                                  : text.executionChecking,
                              ),
                            ]
                          : []),
                        ...(state.setup.key && !state.setup.cancelling
                          ? [ui.button('setup-cancel', text.executionCancel)]
                          : []),
                        ...(state.setup.url
                          ? [
                              ui.text('setup-url', state.setup.url),
                              ui.button(
                                'setup-copy',
                                text.executionCopyUrl,
                                false,
                                false,
                                undefined,
                                state.setup.url,
                              ),
                            ]
                          : []),
                        ...(state.setup.result?.kind === 'authenticated'
                          ? [ui.text('setup-authenticated', text.executionAuthenticated)]
                          : []),
                        ...(state.setup.unresolved
                          ? [
                              ui.text(
                                'setup-unresolved',
                                text.executionUnresolved,
                                props.theme.error,
                              ),
                            ]
                          : []),
                        ...(state.setup.result?.kind === 'unresolved'
                          ? [
                              ui.text(
                                'setup-diagnostic',
                                state.setup.result.diagnostic,
                                props.theme.error,
                              ),
                            ]
                          : []),
                        ...(state.setup.reference
                          ? [
                              ui.button(
                                'setup-receipt',
                                text.executionReceipt,
                                !!state.setup.key || !!state.setup.receiptKey,
                              ),
                            ]
                          : []),
                      ]
                    : []),
                ]
              : []),
            ui.text('backend-model-label', text.executionModelName, props.theme.text),
            presentation.slot<InputProps>(
              state.editor,
              'input',
              {
                menu: editorMenu(props.theme, props.locale),
                appearance: props.theme.input,
                accessibilityLabel: text.executionModelName,
                placeholder: text.executionDefault,
                command: state.command,
                tabIndex: 0,
                maxBytes: 512,
                readOnly: state.prefill !== null || state.command !== null,
                disabled,
                autoFocus: true,
              },
              { changed: 'model', submitted: 'apply' },
            ),
            ui.text('backend-model-hint', text.executionModelHint, props.theme.muted),
            ui.choice?.('thinking', text.thinking, options, current, disabled),
            ui.button(
              'apply',
              text.modelApply,
              disabled ||
                state.prefill !== null ||
                !!state.command ||
                !state.observation ||
                !state.names.includes(state.draft.executor),
            ),
          ]
        : []),
      ...(state.error
        ? [ui.text('settings-error', text.executionSettingsFailed, props.theme.error)]
        : []),
    ],
    { gap: 0, scroll: 'manual', autoFocus: true, terminal: { grow: 1, basis: 0 } },
  );
  const content = ui.box(
    'backend-content',
    [
      fields,
      ui.box(
        'backend-actions',
        [
          ui.button('refresh', text.refreshModels, disabled || !!state.listing),
          ui.button('close', text.closeModels),
        ],
        { row: true, gap: 1 },
      ),
    ],
    { gap: 0, terminal: { grow: 1 } },
  );
  const view = ui.dialog('backend-popup', text.executionBackend, background, content, state.open, {
    width: 64,
    anchorToBackground: true,
  });
  const out = output(state, view, events);
  out.actions = actions;
  return out;
}
export function executionPicker<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ExecutionPickerProps, N> {
  return {
    mount(props) {
      const state: State = {
        revision: 0,
        serial: 0,
        openRequest: props.openRequest,
        modelRequest: null,
        open: false,
        names: [],
        setups: [],
        setup: null,
        listing: null,
        failed: false,
        draft: null,
        editor: 'backend-model',
        observation: null,
        prefill: null,
        command: null,
        capture: null,
        error: false,
      };
      return frame(presentation, state, props, list(state));
    },
    update(previous, props) {
      const state = { ...previous, revision: previous.revision + 1 };
      const actions: Action[] = [];
      if (!props.busy && props.openRequest !== null && props.openRequest !== state.openRequest) {
        state.openRequest = props.openRequest;
        if (props.executor || state.names.length) {
          open(state, props);
          actions.push(...list(state));
        } else state.modelRequest = ++state.serial;
      }
      return frame(presentation, state, props, actions);
    },
    event(state, input, props) {
      state.revision++;
      const actions: Action[] = [],
        events: Emission[] = [];
      const cancel: string[] = [];
      if (input.kind === 'action' && input.key === state.setup?.key) {
        const result = input.event as ActionOutcome;
        if (result.status === 'updated') {
          const progress = result.value as ExecutorSetupProgress;
          if (progress.kind === 'authorization_url') state.setup.url = progress.url;
        } else {
          state.setup.key = null;
          state.setup.url = null;
          if (result.status === 'operation') setupReceipt(state, result.reply);
          else state.setup.unresolved = result.status !== 'not_accepted';
          if (result.status === 'not_accepted') {
            state.setup.result = { kind: 'unresolved', diagnostic: result.reason };
            state.setup.settled = true;
          }
        }
      } else if (input.kind === 'action' && input.key === state.setup?.receiptKey) {
        state.setup.receiptKey = null;
        const result = input.event as ActionOutcome;
        if (result.status === 'succeeded') setupReceipt(state, result.value as OperationReply);
      } else if (input.kind === 'action' && input.key === state.listing) {
        state.listing = null;
        const result = input.event as ActionOutcome;
        if (result.status === 'succeeded') {
          state.names = (result.value as AgentExecutors).executors;
          state.setups = (result.value as AgentExecutors).setup;
          events.push({ name: 'backends', value: { available: state.names.length > 0 } });
        } else state.failed = true;
      } else if (input.kind === 'action' && input.key === state.capture?.key) {
        const request = state.capture;
        state.capture = null;
        const result = input.event as ActionOutcome;
        const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null;
        if (
          !props.busy &&
          state.open &&
          chunk &&
          same(request.basis, chunk.metadata as EditorTextRef) &&
          same(request.basis, state.observation?.snapshot.text) &&
          !state.observation?.snapshot.marked &&
          utf8Bytes(chunk.text) === request.basis.bytes &&
          state.names.includes(request.selection.executor)
        ) {
          events.push({
            name: 'executor',
            value: {
              ...request.selection,
              settings: { ...request.selection.settings, model: chunk.text.trim() || null },
            },
          });
          close(state);
        } else state.error = true;
      } else if (input.kind === 'component' && input.child === 'models') {
        if (input.name !== 'selected' || (!props.executor && !state.open && !props.busy))
          events.push({ name: input.name, value: input.event });
      } else if (input.kind === 'component' && input.child === state.editor && state.open) {
        if (input.name === 'changed') {
          const observed = input.event as EditorObservation;
          state.observation = observed;
          if (
            state.command &&
            observed.receipt?.owner === state.command.owner &&
            observed.receipt.sequence === state.command.sequence
          ) {
            state.error = observed.receipt.outcome.status !== 'applied';
            state.command = null;
            state.prefill = null;
          } else if (state.prefill !== null && !state.command) {
            state.command = {
              owner: observed.snapshot.text.owner,
              sequence: ++state.serial,
              basedOn: observed.revision,
              edit: { kind: 'reset', text: state.prefill },
            };
          }
        } else if (input.name === 'submitted' && !props.busy) actions.push(...capture(state));
      } else if (input.kind === 'raw') {
        if (input.key === 'setup-cancel' && state.setup?.key && !state.setup.cancelling) {
          cancel.push(state.setup.key);
          state.setup.cancelling = true;
        } else if (
          input.key === 'setup-receipt' &&
          state.setup?.reference &&
          !state.setup.key &&
          !state.setup.receiptKey
        ) {
          state.setup.receiptKey = `receipt-${++state.serial}`;
          actions.push({
            key: state.setup.receiptKey,
            binding: 'receipts',
            member: 0,
            method: 'read',
            input: state.setup.reference,
            resources: {},
          });
        } else if (!props.busy && state.open && input.key === 'setup-check')
          actions.push(...setupAction(state, { kind: 'check' }));
        else if (
          !props.busy &&
          state.open &&
          /^setup-auth-(0|[1-9][0-9]*)$/.test(input.key) &&
          state.setup?.executor === state.draft?.executor &&
          state.setup?.result?.kind === 'checked'
        ) {
          const method = state.setup.result.methods[Number(input.key.slice(11))];
          if (method?.supported)
            actions.push(...setupAction(state, { kind: 'authenticate', methodId: method.id }));
        } else if (input.key === 'close' || input.key === 'backend-popup') close(state);
        else if (!props.busy && input.key === 'backends') {
          open(state, props);
          actions.push(...list(state));
        } else if (
          state.open &&
          !props.busy &&
          !state.capture &&
          !state.setup?.key &&
          !state.setup?.receiptKey
        ) {
          if (input.key === 'connected') {
            events.push({ name: 'executor', value: null });
            close(state);
            state.modelRequest = ++state.serial;
          } else if (input.key === 'refresh') actions.push(...list(state));
          else if (input.key === 'apply') actions.push(...capture(state));
          else if (/^backend-(0|[1-9][0-9]*)$/.test(input.key)) {
            const name = state.names[Number(input.key.slice(8))];
            if (name && name !== state.draft?.executor)
              draft(
                state,
                props.executor?.executor === name
                  ? props.executor
                  : { executor: name, settings: {} },
              );
          } else if (
            state.draft &&
            (input.key === 'thinking' || input.key.startsWith('thinking-'))
          ) {
            const level =
              input.key === 'thinking' ? (input.event as { key?: string }).key : input.key.slice(9);
            if (level === 'default' || (level && Object.hasOwn(levels, level)))
              state.draft.settings.thinkingLevel =
                level === 'default' ? null : (level as ThinkingLevel);
          }
        }
      }
      const out = frame(presentation, state, props, actions, events);
      out.cancel = cancel;
      return out;
    },
  };
}
export function activate() {
  return {
    ExecutionPicker: {
      kind: 'component',
      contract: executionPickerContract,
      component: ['component'],
    },
  };
}
