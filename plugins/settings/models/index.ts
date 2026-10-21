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
  Component,
  Emission,
  SettingsModelChoice,
  SettingsModelCursor,
  SettingsModelsPage,
  SettingsDefaultModel,
  EditorObservation,
  EditorTextRef,
  TextChunk,
  ComponentInput,
  ThinkingLevel,
  ReadingCommand,
  ReadingObservation,
} from '../../../packages/plugin-sdk/src/index.js';
import type { ModelsProps, SelectedModel, ModelSetup } from '../../shared/contracts.js';
import { modelsContract } from '../../shared/contracts.js';
import { copy, type Copy } from './locale.js';
import { output, type Presentation, type Node } from '../../shared/view.js';
type Request =
  | { kind: 'page'; key: string; query: string; first: boolean }
  | { kind: 'text'; key: string; basis: EditorTextRef; choice: SelectedModel | null };
interface State {
  preferenceKey: string;
  preferenceScope: string | null;
  resolving: { key: string; scope: string; thinkingLevel: ThinkingLevel | null } | null;
  preferenceUnavailable: boolean;
  catalogEpoch: number;
  openRequest: number | null;
  setup: ModelSetup['kind'];
  publishedSetup: ModelSetup['kind'] | null;
  defaultModel: SettingsDefaultModel | null;
  defaultsOpen: boolean;
  staleSelection: boolean;
  suggestDefault: boolean;
  revision: number;
  serial: number;
  pending: Request | null;
  query: string;
  queryBasis: EditorTextRef | null;
  observation: EditorObservation | null;
  queryError: keyof Copy | null;
  active: number;
  reveal: ReadingCommand | null;
  open: boolean;
  draft: SelectedModel | null;
  thinkingByModel: Record<string, ThinkingLevel | null>;
  choices: SettingsModelChoice[];
  windowStart: number;
  choiceKeys: Record<string, string>;
  nextChoiceKey: number;
  pageQuery: string | null;
  catalogRevision: number | null;
  next: SettingsModelCursor | null;
  scrollRevision: number;
  failed: boolean;
}
function supports(selection: SelectedModel) {
  return (
    selection.thinkingLevel === null ||
    selection.choice.thinkingLevels.some((level) => level === selection.thinkingLevel)
  );
}
function candidate(state: State, props: ModelsProps): SelectedModel | null {
  if (
    props.preferredModel ||
    props.busy ||
    state.pending ||
    state.failed ||
    state.queryError ||
    dirty(state)
  )
    return null;
  if (state.staleSelection && props.selection) {
    const original = props.selection;
    const choice = [
      ...state.choices,
      ...(state.defaultModel?.kind === 'ready' ? [state.defaultModel.choice] : []),
    ].find(
      (choice) =>
        choice.selection.connectionId === original.choice.selection.connectionId &&
        choice.selection.modelId === original.choice.selection.modelId,
    );
    const selection = choice ? { choice, thinkingLevel: original.thinkingLevel } : null;
    return selection && supports(selection) ? selection : null;
  }
  return props.suggestDefault !== false &&
    !props.selection &&
    state.suggestDefault &&
    state.defaultModel?.kind === 'ready'
    ? { choice: state.defaultModel.choice, thinkingLevel: null }
    : null;
}
function resolvePreference(state: State, props: ModelsProps): Action[] {
  const identity = JSON.stringify([
    props.selectionKey ?? '',
    props.preferredModel ?? null,
    props.catalogEpoch,
  ]);
  if (state.preferenceKey === identity) return [];
  state.preferenceKey = identity;
  state.resolving = null;
  state.preferenceUnavailable = false;
  state.suggestDefault = true;
  state.staleSelection = false;
  if (state.preferenceScope !== (props.selectionKey ?? '')) {
    state.preferenceScope = props.selectionKey ?? '';
    state.draft = null;
    close(state);
  }
  const preferred = props.preferredModel;
  if (!preferred) return [];
  const key = `resolve-model-${++state.serial}`;
  state.resolving = {
    key,
    scope: props.selectionKey ?? '',
    thinkingLevel: preferred.thinkingLevel,
  };
  return [
    {
      key,
      binding: 'settings',
      member: 0,
      method: 'resolveModel',
      input: { connectionId: preferred.connectionId, modelId: preferred.modelId },
      resources: {},
    },
  ];
}
function modelKey(state: State, choice: SettingsModelChoice) {
  const identity = JSON.stringify([choice.selection.connectionId, choice.selection.modelId]);
  return (state.choiceKeys[identity] ??= `choose-${state.nextChoiceKey++}`);
}
function sameModel(a: SettingsModelChoice, b: SettingsModelChoice) {
  return (
    a.selection.connectionId === b.selection.connectionId &&
    a.selection.modelId === b.selection.modelId
  );
}
function selectDraft(state: State, choice: SettingsModelChoice) {
  const selection = {
    choice,
    thinkingLevel: state.thinkingByModel[modelKey(state, choice)] ?? null,
  };
  state.draft = selection;
  state.active = state.choices.findIndex((row) => sameModel(row, choice));
}
function open(state: State, props: ModelsProps) {
  state.open = true;
  state.draft = state.staleSelection ? null : props.selection;
  state.thinkingByModel = state.draft
    ? { [modelKey(state, state.draft.choice)]: state.draft.thinkingLevel }
    : {};
  const choice = state.draft
    ? state.choices.find((row) => sameModel(row, state.draft!.choice))
    : state.choices[0];
  if (choice) selectDraft(state, choice);
  else state.active = state.draft ? -1 : 0;
  if (state.active >= 0) reveal(state);
}
const thinkingCopy: Record<ThinkingLevel, keyof Copy> = {
  off: 'thinkingOff',
  minimal: 'thinkingMinimal',
  low: 'thinkingLow',
  medium: 'thinkingMedium',
  high: 'thinkingHigh',
  xhigh: 'thinkingXhigh',
  max: 'thinkingMax',
  ultra: 'thinkingUltra',
};

function sameText(a: EditorTextRef | null, b: EditorTextRef | null) {
  return a?.owner === b?.owner && a?.revision === b?.revision;
}
function dirty(state: State) {
  return !!state.observation && !sameText(state.observation.snapshot.text, state.queryBasis);
}
function interactive(state: State, props: ModelsProps) {
  return state.open && !state.defaultsOpen && !props.busy;
}
function resume(state: State, props: ModelsProps): Action[] {
  if (!interactive(state, props) || state.pending || state.queryError) return [];
  if (dirty(state)) return capture(state);
  if (state.failed) return [];
  if (
    state.pageQuery !== state.query ||
    state.catalogRevision === null ||
    (!state.choices.length && state.next)
  )
    return read(state, !dirty(state) && !!state.next);
  return [];
}
function defaultsInteractive(state: State, props: ModelsProps) {
  return state.open && state.defaultsOpen && !props.busy;
}
function reveal(state: State) {
  if (!state.choices[state.active]) return;
  if (state.active < state.windowStart || state.active >= state.windowStart + 64)
    state.windowStart = Math.max(0, state.active - 32);
  state.reveal = {
    sequence: (state.reveal?.sequence ?? 0) + 1,
    target: { kind: 'reveal_child', key: modelKey(state, state.choices[state.active]!) },
  };
}
function editableChoices(state: State, props: ModelsProps) {
  return (
    state.open &&
    !state.defaultsOpen &&
    !props.busy &&
    (!state.pending || (state.pending.kind === 'page' && !state.pending.first)) &&
    (!state.failed || state.choices.length > 0) &&
    !state.queryError &&
    !dirty(state)
  );
}
function capture(state: State, choice: SelectedModel | null = null): Action[] {
  const basis = state.observation?.snapshot.text;
  if (!basis || (state.pending && !(choice && state.pending.kind === 'page'))) return [];
  const key = `models-${++state.serial}`;
  state.pending = { kind: 'text', key, basis, choice };
  state.queryError = null;
  return [
    {
      key,
      binding: 'reader',
      member: 0,
      method: 'read',
      input: { start: 0, end: basis.bytes },
      resources: {
        text: {
          child: 'model-query',
          method: 'snapshot',
          input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
        },
      },
    },
  ];
}
function read(state: State, more = false): Action[] {
  if (state.pending) return [];
  if (dirty(state)) return capture(state);
  return readPage(state, more);
}
function readPage(state: State, more = false): Action[] {
  if (state.pending) return [];
  const cursor = more ? state.next : null;
  const key = `models-${++state.serial}`;
  state.pending = { kind: 'page', key, query: state.query, first: !cursor };
  state.failed = false;
  return [
    {
      key,
      binding: 'settings',
      member: 0,
      method: 'models',
      input:
        cursor && state.catalogRevision !== null
          ? { kind: 'continue', revision: state.catalogRevision, cursor }
          : { kind: 'start', query: state.query },
      resources: {},
    },
  ];
}
function close(state: State) {
  state.open = false;
  state.draft = null;
  state.thinkingByModel = {};
  if (state.pending?.kind === 'text') state.pending.choice = null;
}
function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: ModelsProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = presentation.paint(props.theme);
  const text = copy(props.locale);
  if (state.publishedSetup !== state.setup) {
    events.push({ name: 'setup', value: { catalogEpoch: state.catalogEpoch, kind: state.setup } });
    state.publishedSetup = state.setup;
  }
  const chosen = state.staleSelection ? null : props.selection?.choice;
  const effort = props.selection?.thinkingLevel;
  const label = chosen
    ? chosen.label + (effort ? ` · ${text[thinkingCopy[effort]]}` : '')
    : state.resolving
      ? text.loadingModels
      : state.preferenceUnavailable
        ? `${text.savedModelUnavailable} · ${props.preferredModel?.modelId ?? ''}`
        : text.chooseModel;
  const trigger = ui.button('toggle', label, props.busy, false, {
    align: 'start',
    ...{ background: 0 },
  });

  const modelRows = state.choices
    .slice(state.windowStart, state.windowStart + 64)
    .map((choice, offset) => {
      const at = state.windowStart + offset;
      const active = !!state.draft && sameModel(state.draft.choice, choice);
      {
        const detail =
          choice.label === choice.selection.modelId
            ? choice.connectionName
            : `${choice.connectionName} · ${choice.selection.modelId}`;
        const item = ui.item(
          modelKey(state, choice),
          ui.box(
            `model-${at}`,
            [
              ui.text(
                `model-name-${at}`,
                `${active ? (props.theme.ascii ? '>' : '›') : ' '} ${choice.label}`,
                props.theme.text,
                400,
                { truncate: true },
              ),
              ui.text(`model-detail-${at}`, `  ${detail}`, props.theme.muted, 400, {
                truncate: true,
              }),
            ],
            { gap: 0 },
          ),
          active,
          !editableChoices(state, props),
          { followFocus: true, submit: true },
        );
        return ui.contextMenu!('menu-' + modelKey(state, choice), choice.label, item, [
          ui.button(
            'settings-' + modelKey(state, choice),
            text.modelSettings,
            !editableChoices(state, props),
          ),
        ]);
      }
    });
  const viewport = presentation.reading(
    'model-rows',
    {
      content: ui.box('model-viewport', modelRows, {
        scroll: 'anchor',
        focusGroup: true,
        gap: 0,
        terminal: { grow: 1 },
      }),
      command: state.reveal,
    },
    { input: 'row-input', observed: 'row-viewport' },
  );
  const controls = ui.box(
    'model-actions',
    [
      ui.button('connections', text.configureModels, props.busy),
      ui.button(
        'refresh',
        props.theme.ascii ? 'r' : '↻',
        !!state.pending || props.busy,
        false,
        { background: 0 },
        undefined,
        text.refreshModels,
      ),
      ui.button('defaults', text.modelSettings),
      ui.button(
        'apply',
        text.modelApply,
        !editableChoices(state, props) || !state.draft || !supports(state.draft),
        true,
      ),
      ui.button('close', text.closeModels, false),
    ],
    { row: true, gap: 1, terminal: { wrap: true } },
  );
  const draft = state.draft;
  const levels = draft?.choice.thinkingLevels ?? [];
  const defaultLevel = draft?.choice.defaultThinkingLevel;
  const options = [
    {
      key: 'default',
      label: text.thinkingDefault + (defaultLevel ? ` (${text[thinkingCopy[defaultLevel]]})` : ''),
    },
    ...levels.map((level) => ({ key: level, label: text[thinkingCopy[level]] })),
  ];
  const available = !draft || supports(draft);
  const current = available ? (draft?.thinkingLevel ?? 'default') : null;
  const currentLabel = available
    ? options.find((option) => option.key === current)!.label
    : `${text[thinkingCopy[draft!.thinkingLevel!]]} · ${text.thinkingUnavailable}`;
  const thinking =
    draft && (levels.length || draft.thinkingLevel !== null)
      ? ui.choice?.(
          'thinking',
          available ? text.thinking : `${text.thinking}: ${currentLabel}`,
          options,
          current,
          !editableChoices(state, props),
        )
      : null;
  const content = ui.box(
    'model-list',
    [
      ui.box(
        'model-choices',
        [
          ui.box(
            'model-query-scope',
            [
              presentation.slot(
                'model-query',
                'input',
                {
                  appearance: props.theme.input,
                  accessibilityLabel: text.searchModels,
                  placeholder: text.searchModels,
                  autoFocus: true,
                  tabIndex: 1,
                  readOnly: false,
                  disabled: props.busy || !state.open || state.defaultsOpen,
                },
                { changed: 'query', submitted: 'choose-active' },
              ),
            ],
            {
              gap: 0,
              focusShortcuts: ['down', 'up'].map((key) => ({
                key,
                modifiers: [],
                child: 'model-rows',
                enabled: editableChoices(state, props),
              })),
              shortcuts: [],
            },
          ),
          ...(state.queryError
            ? [ui.text('query-error', text[state.queryError], props.theme.muted)]
            : []),
          ...(state.pending
            ? [ui.text('model-loading', text.loadingModels, props.theme.muted)]
            : []),
          ...(state.failed
            ? [ui.text('model-error', text.modelsUnavailable, props.theme.muted)]
            : []),
          ...(!state.pending &&
          !state.failed &&
          !state.queryError &&
          !dirty(state) &&
          !state.choices.length
            ? [
                ui.text(
                  'no-models',
                  state.query ? text.noMatchingModels : text.noModels,
                  props.theme.muted,
                ),
              ]
            : []),
          ...(state.draft && state.active < 0
            ? [ui.text('model-draft', state.draft.choice.label)]
            : []),
          viewport,
          ...(thinking ? [thinking] : []),
          controls,
        ],
        {
          visible: !state.defaultsOpen,
          terminal: { grow: 1 },
          gap: 0,
          shortcuts: [{ key: 'escape', modifiers: [], input: 'close', enabled: true }],
        },
      ),
      ui.box(
        'model-default-pane',
        [
          ui.box(
            'default-navigation',
            [
              ui.button('default-back', text.models, false, false, {
                background: 0,
                align: 'start',
              }),
              ui.button(
                'default-refresh',
                props.theme.ascii ? 'r' : '↻',
                !!state.pending,
                false,
                { background: 0 },
                undefined,
                text.refreshModels,
              ),
              ui.button('default-close', text.closeModels),
            ],
            { row: true, gap: 0, terminal: { wrap: true } },
          ),
          ui.box(
            'default-body',
            [
              presentation.slot(
                'default-settings',
                'defaults',
                {
                  theme: props.theme,
                  locale: props.locale,
                  selection: state.draft?.choice ?? chosen ?? null,
                  defaultModel: state.defaultModel,
                  catalogRevision: state.catalogRevision,
                  busy: props.busy || !!state.pending || state.failed,
                },
                { changed: 'default-changed' },
              ),
            ],
            { terminal: { grow: 1, basis: 0 } },
          ),
        ],
        {
          visible: state.defaultsOpen,
          gap: 1,
          autoFocus: true,
          terminal: { grow: 1 },
          shortcuts: [{ key: 'escape', modifiers: [], input: 'default-back', enabled: true }],
        },
      ),
    ],
    {
      visible: state.open,
      terminal: { grow: state.defaultsOpen ? 1 : 0 },
      surface: false,
      padding: 0,
    },
  );
  const view = ui.dialog('model-popup', text.models, trigger, content, state.open, {
    width: 64,
    height: state.defaultsOpen ? 32 : 20,
    anchorToBackground: true,
  });
  const out = output(state, view, events);
  out.actions = actions;
  return out;
}
export function models<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ModelsProps, N> {
  return {
    mount(props) {
      const state: State = {
        revision: 0,
        preferenceKey: '',
        preferenceScope: null,
        resolving: null,
        preferenceUnavailable: false,
        catalogEpoch: props.catalogEpoch,
        openRequest: props.openRequest,
        setup: 'loading',
        publishedSetup: null,
        defaultModel: null,
        defaultsOpen: false,
        staleSelection: false,
        suggestDefault: true,
        serial: 0,
        pending: null,
        query: '',
        queryBasis: null,
        observation: null,
        queryError: null,
        active: 0,
        reveal: null,
        open: false,
        draft: null,
        thinkingByModel: {},
        choices: [],
        windowStart: 0,
        choiceKeys: {},
        nextChoiceKey: 0,
        pageQuery: null,
        catalogRevision: null,
        next: null,
        scrollRevision: 0,
        failed: false,
      };
      return frame(presentation, state, props, [
        ...read(state),
        ...resolvePreference(state, props),
      ]);
    },
    update(previous, props) {
      const resolution = resolvePreference(previous, props);
      const requested = props.openRequest !== previous.openRequest && props.openRequest !== null;
      previous = {
        ...previous,
        openRequest: requested && props.busy ? previous.openRequest : props.openRequest,
      };
      if (requested && !props.busy) open(previous, props);
      if (previous.catalogEpoch !== props.catalogEpoch) {
        const state = {
          ...previous,
          revision: previous.revision + 1,
          catalogEpoch: props.catalogEpoch,
          setup: 'loading' as const,
          publishedSetup: null,
          staleSelection: true,
          suggestDefault: false,
          choices: [],
          windowStart: 0,
          pageQuery: null,
          pending: null,
          catalogRevision: null,
          defaultModel: null,
          next: null,
          scrollRevision: 0,
        };
        return frame(presentation, state, props, [
          ...(state.defaultsOpen ? readPage(state) : read(state)),
          ...resolution,
        ]);
      }
      if (props.selection && !previous.staleSelection) previous.suggestDefault = false;
      const initial = candidate(previous, props);
      if (initial && previous.staleSelection) previous.staleSelection = false;
      const state = { ...previous, revision: previous.revision + 1 };
      return frame(
        presentation,
        state,
        props,
        [
          ...resolution,
          ...(defaultsInteractive(state, props) && state.catalogRevision === null && !state.failed
            ? readPage(state)
            : resume(state, props)),
        ],
        initial ? [{ name: 'selected', value: initial }] : [],
      );
    },
    event(state, input, props) {
      state.revision++;
      const actions: Action[] = [];
      const events: Emission[] = [];
      if (input.kind === 'component' && input.child === 'model-rows' && input.name === 'input')
        input = input.event as ComponentInput;
      if (input.kind === 'action' && input.key === state.resolving?.key) {
        const resolving = state.resolving;
        state.resolving = null;
        const result = input.event as ActionOutcome;
        const value = result.status === 'succeeded' ? (result.value as SettingsDefaultModel) : null;
        const selection =
          value?.kind === 'ready'
            ? { choice: value.choice, thinkingLevel: resolving.thinkingLevel }
            : null;
        const available = selection && supports(selection) ? selection : null;
        state.staleSelection = false;
        state.preferenceUnavailable = !available;
        events.push({
          name: 'resolved',
          value: { selectionKey: resolving.scope, selection: available },
        });
      } else if (input.kind === 'action' && input.key === state.pending?.key) {
        const request = state.pending;
        state.pending = null;
        const result = input.event as ActionOutcome;
        if (request.kind === 'text') {
          if (!sameText(request.basis, state.observation?.snapshot.text ?? null)) {
            if (interactive(state, props)) actions.push(...read(state));
          } else if (result.status !== 'succeeded') state.queryError = 'modelsUnavailable';
          else {
            const query = (result.value as TextChunk).text.trim();
            if (request.choice) {
              if (editableChoices(state, props) && query === state.query) {
                state.staleSelection = false;
                state.preferenceUnavailable = false;
                state.resolving = null;
                state.suggestDefault = false;
                close(state);
                events.push({ name: 'selected', value: request.choice });
              }
            } else {
              const changed = query !== state.query;
              state.query = query;
              state.queryBasis = request.basis;
              state.queryError = null;
              if (changed) {
                state.failed = false;
                state.active = 0;
                state.draft = null;
                state.choices = [];
                state.next = null;
                state.pageQuery = null;
                if (query) state.suggestDefault = false;
              }
              actions.push(...resume(state, props));
            }
          }
        } else if (result.status !== 'succeeded') {
          state.failed = request.query === state.query;
          if (request.first && !request.query) state.setup = 'unavailable';
          actions.push(...resume(state, props));
        } else {
          const page = result.value as SettingsModelsPage;
          if (page.kind === 'changed') {
            state.choices = [];
            state.next = null;
            state.pageQuery = null;
            state.catalogRevision = null;
            state.defaultModel = null;
            state.setup = 'loading';
            if (interactive(state, props)) actions.push(...read(state));
            else if (defaultsInteractive(state, props)) actions.push(...readPage(state));
          } else {
            state.catalogRevision = page.revision;
            state.defaultModel = page.defaultModel;
            // Only an unfiltered first page can prove that no enabled chat
            // models exist. An empty search or final page is not that fact.
            if (request.first && !request.query)
              state.setup =
                page.choices.length || page.next || page.defaultModel.kind === 'ready'
                  ? 'choose'
                  : 'connect';
            else if (state.setup === 'loading') state.setup = 'choose';
            // A page's choices and cursor always travel with its original
            // revision, even while the native search input has newer metadata.
            const current = request.query === state.query;
            if (request.first) state.windowStart = 0;
            state.choices = current
              ? request.first
                ? page.choices
                : [
                    ...state.choices,
                    ...page.choices.filter(
                      (choice) => !state.choices.some((loaded) => sameModel(loaded, choice)),
                    ),
                  ]
              : [];
            state.next = current ? (page.next ?? null) : null;
            state.pageQuery = current ? request.query : null;
            if (request.first && (state.draft || (state.open && !state.defaultsOpen))) {
              const choice = state.draft
                ? [
                    ...state.choices,
                    ...(page.defaultModel.kind === 'ready' ? [page.defaultModel.choice] : []),
                  ].find((row) => sameModel(row, state.draft!.choice))
                : state.active >= 0
                  ? state.choices[0]
                  : undefined;
              if (choice) selectDraft(state, choice);
              else if (state.draft) {
                state.draft = null;
                state.active = -1;
              }
              if (state.active >= 0) reveal(state);
            }
            actions.push(...resume(state, props));
            const initial = candidate(state, props);
            if (initial) {
              state.staleSelection = false;
              events.push({ name: 'selected', value: initial });
            }
          }
        }
      } else if (
        input.kind === 'component' &&
        input.child === 'model-rows' &&
        input.name === 'observed'
      ) {
        const observed = input.event as ReadingObservation;
        if (
          observed.scroll &&
          observed.scroll.readingRevision > state.scrollRevision &&
          observed.viewport?.readingRevision === observed.readingRevision &&
          observed.pendingCommand == null
        ) {
          state.scrollRevision = observed.scroll.readingRevision;
          if (interactive(state, props)) {
            if (observed.scroll.direction === 'up' && observed.viewport.atStart)
              state.windowStart = Math.max(0, state.windowStart - 32);
            else if (observed.scroll.direction === 'down' && observed.viewport.atEnd) {
              if (state.windowStart + 64 < state.choices.length) state.windowStart += 32;
              else if (!state.pending && state.next) actions.push(...read(state, true));
            }
          }
        }
      } else if (input.kind === 'component' && input.child === 'model-query') {
        if (input.name === 'changed') {
          state.observation = input.event as EditorObservation;
          state.queryError = null;
          if (!state.observation.snapshot.text.bytes) {
            const changed = state.query !== '';
            state.query = '';
            state.queryBasis = state.observation.snapshot.text;
            if (changed) {
              state.failed = false;
              state.draft = null;
              state.choices = [];
              state.next = null;
              state.pageQuery = null;
              state.active = 0;
              if (interactive(state, props)) actions.push(...read(state));
            }
          } else if (interactive(state, props)) actions.push(...read(state));
        } else if (input.name === 'submitted' && editableChoices(state, props)) {
          if (state.draft && supports(state.draft)) actions.push(...capture(state, state.draft));
        }
      } else if (
        input.kind === 'component' &&
        input.child === 'default-settings' &&
        input.name === 'changed'
      ) {
        state.suggestDefault = false;
        state.pending = null;
        state.catalogRevision = null;
        state.defaultModel = null;
        state.staleSelection = true;
        state.preferenceKey = '';
        if (props.preferredModel) {
          events.push({
            name: 'resolved',
            value: { selectionKey: props.selectionKey ?? '', selection: null },
          });
          actions.push(...resolvePreference(state, props));
        }
        actions.push(...readPage(state));
      } else if (input.kind === 'raw') {
        if (input.key === 'model-popup' && (input.event as { kind?: string })?.kind === 'dismiss')
          close(state);
        else if (input.key === 'close' || input.key === 'default-close') close(state);
        else if (input.key === 'defaults') state.defaultsOpen = true;
        else if (input.key === 'default-back') {
          state.defaultsOpen = false;
          actions.push(...resume(state, props));
        } else if (!props.busy) {
          if (input.key === 'connections' && interactive(state, props)) {
            close(state);
            events.push({ name: 'connections', value: null });
          } else if (input.key === 'toggle') {
            if (state.open) close(state);
            else {
              open(state, props);
              actions.push(...resume(state, props));
            }
          } else if (
            input.key === 'apply' &&
            editableChoices(state, props) &&
            state.draft &&
            supports(state.draft)
          ) {
            actions.push(...capture(state, state.draft));
          } else if (
            (input.key === 'thinking' || input.key.startsWith('thinking-')) &&
            editableChoices(state, props) &&
            state.draft
          ) {
            const value =
              input.key === 'thinking'
                ? (input.event as { kind?: string; key?: string }).key
                : input.key.slice('thinking-'.length);
            if (
              value === 'default' ||
              state.draft.choice.thinkingLevels.some((level) => level === value)
            ) {
              state.draft = {
                ...state.draft,
                thinkingLevel: value === 'default' ? null : (value as ThinkingLevel),
              };
              state.thinkingByModel[modelKey(state, state.draft.choice)] =
                state.draft.thinkingLevel;
            }
          } else if (input.key === 'default-refresh') actions.push(...readPage(state));
          else if (input.key === 'refresh') actions.push(...read(state));
          else if (editableChoices(state, props) && input.key.startsWith('settings-choose-')) {
            const choice = state.choices.find(
              (choice) => 'settings-' + modelKey(state, choice) === input.key,
            );
            if (choice) {
              selectDraft(state, choice);
              state.defaultsOpen = true;
            }
          } else if (editableChoices(state, props) && input.key.startsWith('choose-')) {
            const choice = state.choices.find((choice) => modelKey(state, choice) === input.key);
            if (choice) {
              state.active = state.choices.indexOf(choice);
              selectDraft(state, choice);
              if (state.active < state.windowStart + 3 && state.windowStart > 0)
                state.windowStart = Math.max(0, state.windowStart - 32);
              else if (
                state.active >= state.windowStart + 61 &&
                state.windowStart + 64 < state.choices.length
              )
                state.windowStart += 32;
              if (state.next && state.active >= state.choices.length - 3)
                actions.push(...read(state, true));
            }
          }
        }
      }
      if (defaultsInteractive(state, props) && state.catalogRevision === null && !state.failed)
        actions.push(...readPage(state));
      const out = frame(presentation, state, props, actions, events);

      return out;
    },
  };
}
export function activate() {
  return { Models: { kind: 'component', contract: modelsContract, component: ['component'] } };
}
