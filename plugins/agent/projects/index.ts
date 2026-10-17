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
  AgentProject,
  AgentProjectRef,
  AgentProjectChange,
  AgentProjectsPage,
  AgentProjectResult,
  AgentProjectCursor,
  AgentProjectLocation,
  AgentDirectorySelection,
  CallerIntent,
  Component,
  ComponentInput,
  EditorCommand,
  EditorObservation,
  InputProps,
  Emission,
  OperationReply,
  RecoveryRef,
  ReadingCommand,
  ReadingObservation,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import { projectsContract, type ProjectsProps } from '../../shared/contracts.js';
import { pageHeader, withPageShell, type PageState } from '../../shared/page-shell.js';
import { copy, type Copy } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { compactLabel, output, type Node, type Presentation } from '../../shared/view.js';

type Request =
  | { kind: 'page'; append: boolean }
  | { kind: 'directory' | 'name' | 'mutate' | 'receipt' }
  | { kind: 'history'; after: number };
interface State extends PageState {
  revision: number;
  open: boolean;
  focus: ReadingCommand | null;
  serial: number;
  projects: AgentProject[];
  windowStart: number;
  scrollRevision: number;
  next: AgentProjectCursor | null;
  selected: string | null;
  pending: { key: string; request: Request } | null;
  restoring: boolean;
  historyFailed: boolean;
  original: RecoveryRef | null;
  current: { input: AgentProjectChange; reference: RecoveryRef | null } | null;
  picker: boolean;
  source: AgentProject | null;
  location: AgentProjectLocation | null;
  confirm: AgentProjectChange | null;
  rename: AgentProject | null;
  observation: EditorObservation | null;
  command: EditorCommand | null;
  sequence: number;
  nameReady: boolean;
  captured: EditorObservation | null;
  notice: keyof Copy | null;
}
function request(
  state: State,
  actions: Action[],
  r: Request,
  binding: string,
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  if (state.pending) return;
  const key = `projects-${++state.serial}`;
  state.pending = { key, request: r };
  actions.push({ key, binding, member: 0, method, input, resources });
}
function page(state: State, actions: Action[], cursor: AgentProjectCursor | null = null) {
  request(state, actions, { kind: 'page', append: cursor !== null }, 'agent', 'projects', {
    kind: 'list',
    cursor,
  });
}
function history(state: State, actions: Action[], after = 0) {
  state.restoring = true;
  state.historyFailed = false;
  if (after === 0) state.original = null;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 16 });
}
function locked(state: State, props: ProjectsProps) {
  return !props.open || props.busy || !!state.pending || !!state.current || state.restoring;
}
function initialize(state: State) {
  if (state.rename && state.observation && !state.nameReady && !state.command)
    state.command = {
      owner: state.observation.snapshot.text.owner,
      basedOn: state.observation.revision,
      sequence: ++state.sequence,
      edit: { kind: 'reset', text: state.rename.name },
    };
}
function mutate(state: State, actions: Action[], input: AgentProjectChange) {
  state.current = { input, reference: null };
  state.notice = null;
  request(state, actions, { kind: 'mutate' }, 'agent', 'project', input);
  actions.at(-1)!.recovery = { kind: 'projects/v1', input };
}
function settle(state: State, reply: OperationReply, actions: Action[], events: Emission[]) {
  if (
    !state.current ||
    (state.current.reference && !sameValue(reply.reference, state.current.reference))
  )
    return;
  state.current.reference = reply.reference;
  if (reply.receipt.kind !== 'recorded' || reply.receipt.outcome.kind !== 'returned') {
    state.notice = 'projectsUnknown';
    return;
  }
  const result = reply.receipt.outcome.value as AgentProjectResult;
  state.current = null;
  state.confirm = null;
  state.notice =
    result.kind === 'committed'
      ? 'projectsSaved'
      : result.kind === 'changed'
        ? 'projectsChanged'
        : result.kind === 'busy'
          ? 'projectsBusy'
          : result.kind === 'invalid'
            ? 'projectsInvalid'
            : 'projectsUnavailable';
  if (result.kind === 'committed') {
    state.rename = null;
    state.location = null;
    state.source = null;
    events.push({ name: 'changed', value: {} });
    page(state, actions);
  }
}
export function projects<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ProjectsProps, N> {
  function frame(
    state: State,
    props: ProjectsProps,
    actions: Action[] = [],
    events: Emission[] = [],
  ) {
    const ui = presentation.paint(props.theme),
      text = copy(props.locale),
      disabled = locked(state, props);
    const controls = (project: AgentProject, prefix = '') => [
      ui.button(
        prefix + 'create-' + project.reference.id,
        text.new,
        disabled || project.archived || !project.selection,
      ),
      ui.button(prefix + 'rename-' + project.reference.id, text.projectsRename, disabled),
      ui.button(
        prefix + 'archive-' + project.reference.id,
        project.archived ? text.projectsRestore : text.projectsArchive,
        disabled,
      ),
      ui.button(prefix + 'relocate-' + project.reference.id, text.projectsRelocate, disabled),
    ];
    const rows = state.projects.slice(state.windowStart, state.windowStart + 64).map((project) => {
      const id = project.reference.id;
      const content = ui.box(
        'summary-' + id,
        [
          ui.text(
            'name-' + id,
            project.name + (project.archived ? ' · ' + text.projectsArchived : ''),
            props.theme.text,
            500,
            { truncate: true },
          ),
          ui.text('path-' + id, project.path ?? text.projectsUnavailable, props.theme.muted, 400, {
            truncate: true,
          }),
        ],
        { gap: 0 },
      );
      return ui.contextMenu(
        'project-' + id,
        text.projectsActions,
        ui.item(
          'select-' + id,
          content,
          state.selected === id,
          disabled && state.pending?.request.kind !== 'page',
          {
            followFocus: true,
            submit: true,
          },
        ),
        controls(project),
      );
    });
    const selected = state.projects.find((project) => project.reference.id === state.selected);
    const body = ui.box(
      'projects-body',
      [
        ...pageHeader(presentation, state, props, events, text.projectsTitle, 'back', false),
        ...(state.restoring ? [ui.text('checking', text.projectsChecking, props.theme.muted)] : []),
        ...(state.pending && !state.restoring
          ? [ui.text('loading', text.connecting, props.theme.muted)]
          : []),
        ...(state.notice ? [ui.text('notice', text[state.notice], props.theme.muted)] : []),
        ...(state.historyFailed
          ? [ui.button('history-retry', text.sourceRetry, !!state.pending)]
          : []),
        ...(state.current
          ? [
              ui.button(
                'receipt',
                text.executionReceipt,
                !!state.pending || !state.current.reference,
              ),
            ]
          : []),
        ...(!state.projects.length && !state.pending
          ? [ui.text('empty', text.projectsEmpty, props.theme.muted)]
          : []),
        presentation.reading(
          'project-list',
          {
            command: state.focus,
            content: ui.box('project-rows', rows, {
              scroll: 'anchor',
              focusGroup: true,
              gap: 1,
              terminal: { grow: 1, basis: 0 },
            }),
          },
          { input: 'list-input', observed: 'list-viewport' },
        ),
        ...(selected
          ? [
              ui.box('selected-actions', controls(selected, 'footer-'), {
                row: true,
                terminal: { wrap: true },
              }),
            ]
          : []),
        ui.box(
          'projects-controls',
          [
            ui.button('add', text.projectsAdd, disabled),
            ui.button('refresh', text.directoryRefresh, disabled),
          ],
          { row: true, terminal: { wrap: true } },
        ),
      ],
      {
        gap: 1,
        terminal: { grow: 1 },
        shortcuts: [{ key: 'escape', modifiers: [], input: 'back', enabled: true }],
      },
    );
    const picker = presentation.slot(
      'project-directory',
      'directories',
      {
        theme: props.theme,
        locale: props.locale,
        open: props.open && state.picker,
        busy: disabled,
        chooseLabel: text.projectsUseDirectory,
      },
      { selected: 'directory', closed: 'directory-closed' },
    );
    let view = ui.dialog(
      'project-directory-dialog',
      text.directoryTitle,
      body,
      picker,
      state.picker,
      { width: 76, height: 24 },
    );
    const rename = ui.box(
      'project-rename',
      [
        presentation.slot(
          'project-name',
          'input',
          {
            menu: editorMenu(props.theme, props.locale),
            appearance: props.theme.input,
            accessibilityLabel: text.projectsName,
            placeholder: text.projectsName,
            autoFocus: true,
            tabIndex: 1,
            readOnly: false,
            disabled: disabled || !!state.command || !state.nameReady,
            command: state.command,
          } satisfies InputProps,
          { changed: 'name-changed', submitted: 'rename-save' },
        ),
        ...(state.notice ? [ui.text('rename-notice', text[state.notice], props.theme.muted)] : []),
        ...(state.current
          ? [
              ui.button(
                'rename-receipt',
                text.executionReceipt,
                !!state.pending || !state.current.reference,
              ),
            ]
          : []),
        ui.box(
          'rename-actions',
          [
            ui.button(
              'rename-save',
              text.settingsSave,
              disabled || !state.nameReady || !!state.command,
            ),
            ui.button('rename-cancel', text.cancel, !!state.current),
          ],
          { row: true },
        ),
      ],
      {
        shortcuts: [
          { key: 'escape', modifiers: [], input: 'rename-cancel', enabled: !state.current },
        ],
      },
    );
    view = ui.dialog('project-rename-dialog', text.projectsRename, view, rename, !!state.rename, {
      width: 64,
    });
    const confirmation = ui.box(
      'project-confirm',
      [
        ...(state.source ? [ui.text('source-name', state.source.name)] : []),
        ...(state.location
          ? [
              ui.text('destination', compactLabel(state.location.path, 200)),
              ...(state.location.project
                ? [ui.text('destination-project', state.location.project.name)]
                : []),
            ]
          : []),
        ...(state.confirm?.kind === 'relocate'
          ? [
              ui.text('relocate-help', text.projectsRelocateHelp),
              ...(state.location?.project &&
              state.location.project.reference.id !== state.source?.reference.id
                ? [ui.text('merge-help', text.projectsMergeHelp, props.theme.warning)]
                : []),
            ]
          : []),
        ...(state.confirm?.kind === 'archive'
          ? [ui.text('archive-help', text.projectsArchiveHelp)]
          : []),
        ...(state.notice ? [ui.text('confirm-notice', text[state.notice], props.theme.muted)] : []),
        ...(state.current
          ? [
              ui.button(
                'confirm-receipt',
                text.executionReceipt,
                !!state.pending || !state.current.reference,
              ),
            ]
          : []),
        ui.box(
          'confirm-actions',
          [
            ui.button('confirm', text.skillsConfirm, disabled),
            ui.button('confirm-cancel', text.cancel, !!state.current),
          ],
          { row: true },
        ),
      ],
      {
        shortcuts: [
          { key: 'escape', modifiers: [], input: 'confirm-cancel', enabled: !state.current },
        ],
      },
    );
    view = ui.dialog(
      'project-confirm-dialog',
      text.projectsTitle,
      view,
      confirmation,
      !!state.confirm,
      { width: 70 },
    );
    const result = output(
      state,
      ui.box('projects-root', [view], { visible: props.open, terminal: { grow: 1 } }),
      events,
    );
    result.actions = actions;
    return result;
  }
  return withPageShell<State, ProjectsProps, N>(
    {
      mount(props) {
        const state: State = {
          revision: 0,
          open: props.open,
          focus: null,
          serial: 0,
          projects: [],
          windowStart: 0,
          scrollRevision: 0,
          next: null,
          selected: null,
          pending: null,
          restoring: true,
          historyFailed: false,
          original: null,
          current: null,
          picker: false,
          source: null,
          location: null,
          confirm: null,
          rename: null,
          observation: null,
          command: null,
          sequence: 0,
          nameReady: false,
          captured: null,
          notice: null,
        };
        const actions: Action[] = [];
        history(state, actions);
        return frame(state, props, actions);
      },
      update(state, props) {
        state.revision++;
        const actions: Action[] = [];
        if (
          !state.open &&
          props.open &&
          !state.pending &&
          !state.current &&
          !state.restoring &&
          !state.rename &&
          !state.confirm
        )
          page(state, actions);
        state.open = props.open;
        return frame(state, props, actions);
      },
      event(state, input, props) {
        state.revision++;
        const actions: Action[] = [],
          events: Emission[] = [];
        if (input.kind === 'component' && input.child === 'project-list' && input.name === 'input')
          input = input.event as ComponentInput;
        if (
          input.kind === 'component' &&
          input.child === 'project-list' &&
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
            if (!locked(state, props)) {
              if (observed.scroll.direction === 'up' && observed.viewport.atStart)
                state.windowStart = Math.max(0, state.windowStart - 32);
              else if (observed.scroll.direction === 'down' && observed.viewport.atEnd) {
                if (state.windowStart + 64 < state.projects.length) state.windowStart += 32;
                else if (state.next) page(state, actions, state.next);
              }
            }
          }
        }
        if (input.kind === 'component' && input.child === 'project-name') {
          if (input.name === 'changed') {
            state.observation = input.event as EditorObservation;
            const receipt = state.observation.receipt;
            if (
              state.command &&
              receipt?.owner === state.command.owner &&
              receipt.sequence === state.command.sequence
            ) {
              state.nameReady = receipt.outcome.status === 'applied';
              state.command = null;
              if (!state.nameReady) state.notice = 'projectsFailed';
            } else initialize(state);
          } else if (input.name === 'submitted')
            input = { kind: 'raw', key: 'rename-save', event: {} } as ComponentInput;
        }
        if (
          input.kind === 'component' &&
          input.child === 'project-directory' &&
          props.open &&
          state.picker
        ) {
          if (input.name === 'closed') state.picker = false;
          else if (input.name === 'selected' && !locked(state, props)) {
            state.picker = false;
            request(state, actions, { kind: 'directory' }, 'agent', 'projects', {
              kind: 'directory',
              selection: input.event as AgentDirectorySelection,
            });
          }
        } else if (input.kind === 'action' && input.key === state.pending?.key) {
          const r = state.pending.request,
            result = input.event as ActionOutcome;
          state.pending = null;
          if (r.kind === 'history') {
            if (result.status !== 'succeeded') {
              state.historyFailed = true;
              state.notice = 'historyUnavailable';
            } else {
              const items = result.value as CallerIntent[];
              for (const item of items) {
                const recovery = item.recovery as {
                  kind?: string;
                  input?: AgentProjectChange;
                } | null;
                if (
                  item.reference.method === 'project' &&
                  item.reference.contract.name === 'maka.agent.Agent' &&
                  recovery?.kind === 'projects/v1' &&
                  recovery.input
                ) {
                  state.original = item.reference;
                  state.current = { input: recovery.input, reference: item.reference };
                }
              }
              const last = items.at(-1);
              if (last && last.cursor > r.after) history(state, actions, last.cursor);
              else if (last) {
                state.historyFailed = true;
                state.notice = 'historyUnavailable';
              } else {
                state.restoring = false;
                if (state.original)
                  request(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.original);
                else page(state, actions);
              }
            }
          } else if (r.kind === 'mutate') {
            if (result.status === 'operation') settle(state, result.reply, actions, events);
            else if (result.status === 'not_accepted') {
              state.current = null;
              state.notice = 'projectsFailed';
            } else state.notice = 'projectsUnknown';
          } else if (r.kind === 'receipt') {
            if (result.status === 'succeeded')
              settle(state, result.value as OperationReply, actions, events);
            else state.notice = 'projectsUnknown';
          } else if (r.kind === 'name') {
            const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null;
            if (
              !chunk ||
              !state.rename ||
              !sameValue(chunk.metadata, state.captured?.snapshot.text) ||
              new TextEncoder().encode(chunk.text).length !== state.captured?.snapshot.text.bytes
            )
              state.notice = 'projectsFailed';
            else if (
              !chunk.text.trim() ||
              [...chunk.text].length > 256 ||
              /[\u0000-\u001f\u007f-\u009f]/u.test(chunk.text)
            )
              state.notice = 'projectsInvalid';
            else
              mutate(state, actions, {
                kind: 'rename',
                project: state.rename.reference,
                name: chunk.text,
              });
          } else if (result.status !== 'succeeded') state.notice = 'projectsFailed';
          else {
            const value = result.value as AgentProjectsPage;
            if (value.kind === 'projects' && r.kind === 'page') {
              if (!r.append) state.windowStart = 0;
              state.projects = r.append
                ? [
                    ...state.projects,
                    ...value.projects.filter(
                      (p) => !state.projects.some((old) => old.reference.id === p.reference.id),
                    ),
                  ]
                : value.projects;
              state.next = value.next;
              state.selected = state.projects.some((p) => p.reference.id === state.selected)
                ? state.selected
                : (value.projects[0]?.reference.id ?? null);
              if (state.selected && !r.append)
                state.focus = {
                  sequence: (state.focus?.sequence ?? 0) + 1,
                  target: { kind: 'focus_item', key: 'select-' + state.selected },
                };
            } else if (value.kind === 'directory' && r.kind === 'directory') {
              state.location = value.location;
              state.confirm = state.source
                ? {
                    kind: 'relocate',
                    project: state.source.reference,
                    selection: value.location.selection,
                    destination: value.location.project?.reference ?? null,
                  }
                : {
                    kind: 'register',
                    selection: value.location.selection,
                    expected: value.location.project?.reference ?? null,
                  };
            } else
              state.notice = value.kind === 'changed' ? 'projectsChanged' : 'projectsUnavailable';
          }
        } else if (input.kind === 'raw' && props.open) {
          const key = input.key.startsWith('footer-') ? input.key.slice(7) : input.key;
          if (key === 'back') events.push({ name: 'closed', value: {} });
          else if (key === 'history-retry' && state.historyFailed && !state.pending)
            history(state, actions);
          else if (
            ['receipt', 'rename-receipt', 'confirm-receipt'].includes(key) &&
            state.current?.reference &&
            !state.pending
          )
            request(
              state,
              actions,
              { kind: 'receipt' },
              'receipts',
              'read',
              state.current.reference,
            );
          else if (
            ['rename-cancel', 'confirm-cancel'].includes(key) &&
            !state.current &&
            !state.pending
          ) {
            state.confirm = null;
            state.rename = null;
            state.location = null;
            state.source = null;
          } else if (
            !locked(state, props) &&
            !state.picker &&
            (!state.rename || key === 'rename-save') &&
            (!state.confirm || key === 'confirm')
          ) {
            if (key === 'add') {
              state.notice = null;
              state.source = null;
              state.location = null;
              state.picker = true;
            } else if (key === 'refresh') {
              state.notice = null;
              page(state, actions);
            } else if (key === 'confirm' && state.confirm) mutate(state, actions, state.confirm);
            else if (
              key === 'rename-save' &&
              state.rename &&
              state.nameReady &&
              !state.command &&
              state.observation &&
              !state.observation.snapshot.marked
            ) {
              const observation = state.observation;
              if (observation.snapshot.text.bytes > 1024) state.notice = 'projectsInvalid';
              else {
                state.captured = observation;
                request(
                  state,
                  actions,
                  { kind: 'name' },
                  'reader',
                  'read',
                  { start: 0, end: observation.snapshot.text.bytes },
                  {
                    text: {
                      child: 'project-name',
                      method: 'snapshot',
                      input: {
                        kind: 'at_revision',
                        owner: observation.snapshot.text.owner,
                        revision: observation.snapshot.text.revision,
                      },
                    },
                  },
                );
              }
            } else {
              for (const project of state.projects) {
                const id = project.reference.id;
                if (key === 'select-' + id) {
                  state.selected = id;
                  const at = state.projects.indexOf(project);
                  if (at < state.windowStart + 3 && state.windowStart > 0)
                    state.windowStart = Math.max(0, state.windowStart - 32);
                  else if (
                    at >= state.windowStart + 61 &&
                    state.windowStart + 64 < state.projects.length
                  )
                    state.windowStart += 32;
                  else if (
                    (input.event as { kind?: string }).kind !== 'submit' &&
                    at >= state.projects.length - 3 &&
                    state.next
                  )
                    page(state, actions, state.next);
                  if (
                    (input.event as { kind?: string }).kind === 'submit' &&
                    !project.archived &&
                    project.selection
                  )
                    events.push({
                      name: 'selected',
                      value: { project: project.reference, selection: project.selection },
                    });
                } else if (key === 'create-' + id && !project.archived && project.selection)
                  events.push({
                    name: 'selected',
                    value: {
                      project: project.reference as AgentProjectRef,
                      selection: project.selection,
                    },
                  });
                else if (key === 'rename-' + id) {
                  state.rename = project;
                  state.nameReady = false;
                  state.notice = null;
                  initialize(state);
                } else if (key === 'archive-' + id) {
                  state.notice = null;
                  state.source = project;
                  state.location = null;
                  state.confirm = {
                    kind: project.archived ? 'restore' : 'archive',
                    project: project.reference,
                  };
                } else if (key === 'relocate-' + id) {
                  state.notice = null;
                  state.source = project;
                  state.location = null;
                  state.picker = true;
                }
              }
            }
          }
        }
        return frame(state, props, actions, events);
      },
    },
    () => 'back',
  );
}
export function activate() {
  return { Projects: { kind: 'component', contract: projectsContract, component: ['component'] } };
}
