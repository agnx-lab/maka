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

import type { AppsConfiguration } from '../../shared/contracts.js';

import type {
  Action,
  ActionOutcome,
  BindingCandidatesPage,
  CallerIntent,
  ComponentInput,
  EditorObservation,
  Emission,
  FileObservation,
  OperationReply,
  PackageSettingsPage,
  TextChunk,
  ReadingObservation,
} from '../../../packages/plugin-sdk/src/index.js';
import { type AppsProps } from '../../shared/contracts.js';
import { canInstall } from '../../shared/deployment-import.js';
import { settingOptions } from '../../shared/instance-settings.js';
import { scrollBoundary } from '../../shared/scroll.js';
import { sameValue } from '../../shared/value.js';
import { type Node, type Presentation } from '../../shared/view.js';
import {
  bindings,
  capture,
  catalog,
  change,
  choose,
  edit,
  entries,
  history,
  initialize,
  locked,
  mutate,
  request,
  reset,
  settings,
  settle,
  value,
} from './operations.js';
import { type Mutation, type State } from './state.js';
import { frame } from './view.js';

export function handleEvent<N extends Node>(
  presentation: Presentation<N>,
  configuration: AppsConfiguration,
  state: State,
  input: ComponentInput,
  props: AppsProps,
) {
  state.revision++;
  const actions: Action[] = [],
    events: Emission[] = [];
  if (
    input.kind === 'component' &&
    ['apps-list', 'apps-settings-reading'].includes(input.child) &&
    input.name === 'input'
  )
    input = input.event as ComponentInput;
  const settingsDirection = scrollBoundary(state, input, 'apps-settings-reading');
  if (settingsDirection && state.route === 'configure' && state.draft && !locked(state, props)) {
    const draft = state.draft;
    if (settingsDirection === 'up') draft.window = Math.max(0, draft.window - 32);
    else if (draft.window + 64 < (draft.page?.fields.length ?? 0)) draft.window += 32;
    else if (draft.page?.next) {
      draft.after = draft.page.next;
      settings(state, actions);
    }
  }
  const extendList = (direction: 'up' | 'down') => {
    const page =
      state.route === 'choose'
        ? state.catalog
        : state.route === 'bindings'
          ? state.draft?.choices
          : null;
    if (!page) return;
    if (direction === 'up') state.listWindow = Math.max(0, state.listWindow - 32);
    else if (state.listWindow + 64 < page.candidates.length) state.listWindow += 32;
    else if (page.next && !state.pending) {
      if (state.route === 'choose') catalog(state, actions, configuration, page.source, page.next);
      else bindings(state, actions, page.next);
    }
  };
  if (input.kind === 'component' && input.child === 'apps-list' && input.name === 'observed') {
    const observed = input.event as ReadingObservation;
    if (
      observed.scroll &&
      observed.scroll.readingRevision > state.scrollRevision &&
      observed.viewport?.readingRevision === observed.readingRevision &&
      observed.pendingCommand == null
    ) {
      state.scrollRevision = observed.scroll.readingRevision;
      if (
        !locked(state, props) &&
        (observed.scroll.direction === 'up' ? observed.viewport.atStart : observed.viewport.atEnd)
      )
        extendList(observed.scroll.direction);
    }
  }
  if (input.kind === 'component') {
    const name =
      input.child === 'apps-name' ? 'name' : input.child === 'apps-setting' ? 'setting' : null;
    if (name && input.name === 'changed') {
      const f = state.fields[name];
      f.observation = input.event as EditorObservation;
      const receipt = f.observation.receipt;
      if (
        f.command &&
        receipt?.owner === f.command.owner &&
        receipt.sequence === f.command.sequence
      ) {
        f.ready = receipt.outcome.status === 'applied';
        f.command = null;
        f.reset = null;
        if (!f.ready) state.notice = 'appsUnavailable';
      } else initialize(f);
    } else if (name === 'name' && input.name === 'submitted')
      input = { kind: 'raw', key: 'review', event: {} } as ComponentInput;
    else if (input.child === 'apps-file' && input.name === 'changed')
      state.installSource.observation = input.event as FileObservation;
    else if (input.child === 'apps-url' && ['changed', 'submitted'].includes(input.name)) {
      state.installSource.urlObservation = input.event as EditorObservation;
      if (input.name === 'submitted') input = { kind: 'raw', key: 'install', event: {} };
    } else if (input.name === 'closed' && input.child.startsWith('app-')) {
      const id = input.child.slice(4);
      state.opened = state.opened.filter((o) => o !== id);
      if (state.selected === id) {
        state.selected = null;
        state.route = 'list';
      }
    }
  }
  if (input.kind === 'action' && input.key === state.pending?.key) {
    const r = state.pending.request,
      result = input.event as ActionOutcome;
    state.pending = null;
    if (r.kind === 'history') {
      if (result.status !== 'succeeded') {
        state.restoring = false;
        state.historyFailed = true;
        state.notice = 'historyUnavailable';
      } else {
        const items = result.value as CallerIntent[];
        for (const item of items) {
          const recovery = item.recovery as { kind?: string; input?: Mutation } | null;
          if (
            item.reference.contract.name === 'maka.application.DeploymentManager' &&
            ['install', 'installUrl', 'editComposition'].includes(item.reference.method) &&
            recovery?.kind === 'application-apps/v1' &&
            recovery.input
          ) {
            state.original = item.reference;
            state.current = { input: recovery.input, reference: item.reference };
          }
        }
        const last = items.at(-1);
        if (last && last.cursor > r.after) history(state, actions, last.cursor);
        else if (last) {
          state.restoring = false;
          state.historyFailed = true;
          state.notice = 'historyUnavailable';
        } else {
          state.restoring = false;
          if (state.original)
            request(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.original);
          else catalog(state, actions, configuration);
        }
      }
    } else if (r.kind === 'mutation') {
      if (result.status === 'operation') settle(state, result.reply, actions, configuration);
      else if (result.status === 'not_accepted') {
        state.current = null;
        state.notice = 'appsUnavailable';
      } else state.notice = 'appsUnknown';
    } else if (r.kind === 'receipt') {
      if (result.status === 'succeeded')
        settle(state, result.value as OperationReply, actions, configuration);
      else state.notice = 'appsUnknown';
    } else if (r.kind === 'catalog' || r.kind === 'bindings') {
      if (result.status !== 'succeeded') state.notice = 'appsUnavailable';
      else {
        const page = result.value as BindingCandidatesPage;
        if (r.input.source && !sameValue(page.source, r.input.source)) state.notice = 'appsChanged';
        else if (r.kind === 'catalog')
          state.catalog =
            r.input.after && state.catalog
              ? { ...page, candidates: [...state.catalog.candidates, ...page.candidates] }
              : page;
        else if (state.draft)
          state.draft.choices =
            r.input.after && state.draft.choices
              ? { ...page, candidates: [...state.draft.choices.candidates, ...page.candidates] }
              : page;
      }
    } else if (r.kind === 'urlText') {
      const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null;
      if (
        chunk &&
        sameValue(chunk.metadata, r.captured.snapshot.text) &&
        new TextEncoder().encode(chunk.text).length === r.captured.snapshot.text.bytes &&
        sameValue(r.captured, state.installSource.urlObservation) &&
        sameValue(r.source, state.catalog?.source) &&
        state.installSource.mode === 'url' &&
        state.route === 'file'
      ) {
        const url = chunk.text.trim();
        if (/^https?:\/\/[^\s]+$/.test(url))
          state.confirm = { kind: 'install', source: r.source, captured: r.captured, url };
        else state.notice = 'deploymentUrlInvalid';
      } else state.notice = 'deploymentFileFailed';
    } else if (r.kind === 'settings') {
      if (result.status !== 'succeeded') state.notice = 'appsUnavailable';
      else if (
        state.draft &&
        sameValue(state.draft.path, r.path) &&
        state.draft.after === r.after
      ) {
        const page = result.value as PackageSettingsPage;
        if (
          !sameValue(page.source, state.catalog?.source) ||
          state.draft.candidate.kind !== 'package' ||
          !sameValue(page.package, state.draft.candidate.package)
        )
          state.notice = 'appsChanged';
        else {
          if (!r.after) state.draft.window = 0;
          state.draft.page =
            r.after && state.draft.page
              ? { ...page, fields: [...state.draft.page.fields, ...page.fields] }
              : page;
        }
      }
    } else if (r.kind === 'field') {
      const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null;
      if (
        !chunk ||
        !sameValue(chunk.metadata, r.observation.snapshot.text) ||
        new TextEncoder().encode(chunk.text).length !== r.observation.snapshot.text.bytes
      )
        state.notice = 'appsUnavailable';
      else if (state.draft) {
        if (r.name === 'name') {
          const label = chunk.text.trim();
          if (!label || [...label].length > 256 || /[\u0000-\u001f\u007f-\u009f]/u.test(label))
            state.notice = 'appsInvalid';
          else {
            state.draft.label = label;
            edit(state, configuration, label);
          }
        } else if (state.draft.setting) {
          const { descriptor, path } = state.draft.setting;
          const value = descriptor.kind === 'number' ? chunk.text.trim() : chunk.text;
          if (
            [...value].length > 4096 ||
            (descriptor.kind === 'number' &&
              (value.length > 128 || !/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/.test(value)))
          )
            state.notice = 'appsInvalid';
          else {
            change(state.draft, path, {
              kind: descriptor.kind === 'number' ? 'number' : 'text',
              text: value,
            });
            state.draft.setting = null;
            settings(state, actions);
          }
        }
      }
    }
  } else if (input.kind === 'raw' && props.open) {
    const key = input.key,
      d = state.draft;
    if (key === 'back' && !state.confirm && !d?.setting) {
      if (state.current || state.restoring || state.route === 'list')
        events.push({ name: 'closed', value: {} });
      else if (!state.pending) {
        state.notice = null;
        state.diagnostic = null;
        if (state.route === 'bindings') state.route = 'configure';
        else {
          state.route = 'list';
          state.draft = null;
          state.selected = null;
        }
      }
    } else if (
      ['receipt', 'confirm-receipt'].includes(key) &&
      state.current?.reference &&
      !state.pending
    )
      request(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.current.reference);
    else if (key === 'history-retry' && state.historyFailed && !state.pending)
      history(state, actions);
    else if (key === 'confirm-cancel' && !state.current && !state.pending) state.confirm = null;
    else if (!locked(state, props) && (!state.confirm || key === 'confirm')) {
      if (key === 'confirm') mutate(state, actions);
      else if (key === 'setting-cancel' && d) d.setting = null;
      else if (key === 'setting-apply' && d?.setting) capture(state, actions, 'setting');
      else if (!d?.setting) {
        if (key === 'refresh') {
          state.notice = null;
          state.diagnostic = null;
          catalog(state, actions, configuration);
        } else if (key === 'manage') events.push({ name: 'manage', value: {} });
        else if (key === 'add') {
          state.route = 'choose';
          state.notice = null;
          state.diagnostic = null;
          catalog(state, actions, configuration);
        } else if (key === 'file') {
          state.route = 'file';
          state.notice = null;
          state.diagnostic = null;
          if (!state.catalog) catalog(state, actions, configuration);
        } else if (key === 'apps-local' || key === 'apps-remote') {
          state.installSource.mode = key === 'apps-local' ? 'file' : 'url';
          state.notice = null;
          state.diagnostic = null;
        } else if (
          key === 'install' &&
          state.catalog &&
          canInstall(state.installSource, false, false)
        ) {
          if (state.installSource.mode === 'file')
            state.confirm = {
              kind: 'install',
              source: state.catalog.source,
              file: state.installSource.observation!,
            };
          else {
            const captured = state.installSource.urlObservation!,
              basis = captured.snapshot.text;
            request(
              state,
              actions,
              { kind: 'urlText', captured, source: state.catalog.source },
              'reader',
              'read',
              { start: 0, end: basis.bytes },
              {
                text: {
                  child: 'apps-url',
                  method: 'snapshot',
                  input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
                },
              },
            );
          }
        } else if (key.startsWith('open-')) {
          const id = key.slice(5);
          if (configuration.entries.some((e) => e.id === id)) {
            state.selected = id;
            state.route = 'app';
            if (!state.opened.includes(id)) state.opened.push(id);
          }
        } else if (key.startsWith('close-')) {
          const id = key === 'close-current' ? state.selected : key.slice(6);
          state.opened = state.opened.filter((o) => o !== id);
          if (state.selected === id) {
            state.selected = null;
            state.route = 'list';
          }
        } else if (
          key.startsWith('remove-') &&
          state.catalog &&
          sameValue(entries(state), configuration.entries)
        ) {
          const index = Number(key.slice(7));
          if (configuration.entries[index])
            state.confirm = {
              kind: 'edit',
              outcome: 'remove',
              input: {
                expectedRevision: state.catalog.source.revision,
                edits: [
                  {
                    kind: 'update',
                    name: configuration.root,
                    configuration: {
                      entries: entries(state).filter((_, at) => at !== index),
                    },
                    bindings: {
                      pages: state.catalog.current.filter((_, at) => at !== index),
                    },
                  },
                ],
              },
            };
        } else if (
          (key.startsWith('candidate-') || key.startsWith('binding-')) &&
          true &&
          (input.event as { kind?: string }).kind === 'activate'
        ) {
          state.focused = key;
          const at = Number(key.slice(key.startsWith('candidate-') ? 10 : 8));
          const count =
            state.route === 'choose'
              ? (state.catalog?.candidates.length ?? 0)
              : (state.draft?.choices?.candidates.length ?? 0);
          if (at < state.listWindow + 3 && state.listWindow > 0) extendList('up');
          else if (at >= Math.min(state.listWindow + 61, count - 3)) extendList('down');
        } else if (key.startsWith('candidate-') && state.route === 'choose') {
          const candidate = state.catalog?.candidates[Number(key.slice(10))];
          if (candidate) choose(state, candidate, actions);
        } else if (key === 'review' && state.route === 'configure') capture(state, actions, 'name');
        else if (key.startsWith('setting-choice-') && d?.page) {
          const f = d.page.fields[Number(key.slice(15))];
          const option = f && settingOptions(f)[Number((input.event as { key?: string }).key)];
          if (option) {
            change(d, [...d.path, f.key], option);
            settings(state, actions);
          }
        } else if (key.startsWith('setting-') && d?.page) {
          const f = d.page.fields[Number(key.slice(8))];
          if (f?.editable) {
            if (f.kind === 'object' || f.kind === 'array') {
              d.path = [...d.path, f.key];
              d.after = null;

              settings(state, actions);
            } else if (settingOptions(f).length) {
              const options = settingOptions(f),
                index = options.findIndex((v) => sameValue(v, value(d, f)));
              change(d, [...d.path, f.key], options[(index + 1) % options.length]);
              settings(state, actions);
            } else {
              d.setting = { descriptor: f, path: [...d.path, f.key] };
              const v = value(d, f);
              reset(state, 'setting', v?.kind === 'text' || v?.kind === 'number' ? v.text : '');
            }
          }
        } else if (key === 'settings-up' && d?.path.length) {
          d.path = d.path.slice(0, -1);
          d.after = null;

          settings(state, actions);
        } else if (key.startsWith('choose-binding-') && d?.page) {
          const i = d.page.imports[Number(key.slice(15))];
          if (i) {
            d.choosing = i;
            d.choices = null;
            state.route = 'bindings';
            bindings(state, actions);
          }
        } else if (key.startsWith('binding-') && d?.choosing && d.choices) {
          const candidate = d.choices.candidates[Number(key.slice(8))];
          if (candidate?.kind === 'bound') {
            const i = d.choosing,
              current = d.bindings[i.name] ?? [],
              has = current.some((t) => sameValue(t, candidate.target));
            d.bindings[i.name] = has
              ? current.filter((t) => !sameValue(t, candidate.target))
              : i.cardinality.kind === 'many'
                ? [...current, candidate.target]
                : [candidate.target];
          }
        } else if (key === 'bindings-clear' && d?.choosing) d.bindings[d.choosing.name] = [];
        else if (key === 'bindings-done') state.route = 'configure';
      }
    }
  }
  return frame(presentation, configuration, state, props, actions, events);
}
