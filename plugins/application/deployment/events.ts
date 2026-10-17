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
  CallerIntent,
  ComponentInput,
  DeploymentDocumentChunk,
  EditorObservation,
  Emission,
  FileObservation,
  InstanceConfigure,
  InstanceSettingsPage,
  ManagedInstancesPage,
  OperationReply,
  ReadingObservation,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import { bookmarkReady, observeBookmark, retryBookmark } from '../../shared/bookmark.js';
import { type DeploymentProps } from '../../shared/contracts.js';
import {
  acceptDocumentChunk,
  canSaveDocument,
  documentEditor,
  observeDocument,
} from '../../shared/deployment-document.js';
import { canInstall, matchesImport, type InstallIntent } from '../../shared/deployment-import.js';
import {
  acceptSettingText,
  editSetting,
  instanceSettings,
  observeSetting,
  openSetting,
  settingOptions,
  settingValue,
} from '../../shared/instance-settings.js';
import { scrollBoundary, scrollRows, resetScroll } from '../../shared/scroll.js';
import { sameValue } from '../../shared/value.js';
import { type Node, type Presentation } from '../../shared/view.js';
import {
  filteredInstances,
  openSettings,
  page,
  readDocument,
  readSettings,
  request,
  settle,
} from './operations.js';
import {
  settingsBookmark,
  validSettingsBookmark,
  type SettingsBookmark,
  type State,
} from './state.js';
import { frame } from './view.js';

export function handleEvent<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  input: ComponentInput,
  props: DeploymentProps,
) {
  state.revision++;
  if (
    input.kind === 'component' &&
    ['deployment-list', 'instance-settings-reading', 'deployment-history-reading'].includes(
      input.child,
    ) &&
    input.name === 'input'
  )
    input = input.event as ComponentInput;
  const actions: Action[] = [],
    events: Emission[] = [];
  if (
    state.historyOpen &&
    scrollRows(state, input, 'deployment-history-reading', state.earlier.length)
  )
    input = { kind: 'raw', key: 'deployment-history-more', event: {} };
  if (
    input.kind === 'component' &&
    input.child === 'deployment-list' &&
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
      if (!state.pending && !state.current && state.page) {
        if (observed.scroll.direction === 'up' && observed.viewport.atStart)
          state.listWindow = Math.max(0, state.listWindow - 32);
        else if (observed.scroll.direction === 'down' && observed.viewport.atEnd) {
          if (state.listWindow + 64 < filteredInstances(state, props.locale).length)
            state.listWindow += 32;
          else if (state.page.next)
            page(
              state,
              actions,
              { kind: 'continue', source: state.page.source, after: state.page.next },
              'next',
            );
        }
      }
    }
  }
  const settingsDirection = scrollBoundary(state, input, 'instance-settings-reading');
  if (settingsDirection && state.settings?.open && !state.pending && !state.current) {
    const settings = state.settings;
    if (settingsDirection === 'up') settings.window = Math.max(0, settings.window - 32);
    else if (settings.window + 64 < (settings.page?.fields.length ?? 0)) settings.window += 32;
    else if (settings.page?.next) {
      settings.after = settings.page.next;
      readSettings(state, actions);
    }
  }
  {
    const restored = observeBookmark(
      state.bookmark,
      input,
      'settings-checkpoint',
      settingsBookmark(state),
      validSettingsBookmark,
    );
    if (restored) {
      state.settings = {
        ...instanceSettings(restored.source, restored.instance),
        path: restored.path,
        changes: restored.changes,
        open: restored.open,
      };
      state.notice = restored.saved ? 'deploymentSaved' : null;
      state.restoreSettings = true;
    }
    if (input.kind === 'raw' && input.key === 'settings-checkpoint-retry') {
      if (state.bookmark.observed?.state.kind === 'failed') retryBookmark(state.bookmark);
      if (state.restoreFailed) {
        state.restoreFailed = false;
        state.restoreSettings = true;
      }
    }
  }
  if (
    input.kind === 'component' &&
    input.child === 'deployment-query' &&
    input.name === 'changed'
  ) {
    state.queryObservation = input.event as EditorObservation;
    state.notice = state.queryObservation.snapshot.text.bytes > 1024 ? 'searchTooLong' : null;
  } else if (
    input.kind === 'component' &&
    input.child === 'instance-setting-editor' &&
    state.settings
  ) {
    observeSetting(state.settings, input.event as EditorObservation);
  } else if (
    input.kind === 'component' &&
    input.child === 'deployment-import-url' &&
    ['changed', 'submitted'].includes(input.name)
  ) {
    state.importer.urlObservation = input.event as EditorObservation;
    if (!matchesImport(state.importer)) state.importer.confirmation = null;
    if (input.name === 'submitted')
      input = { kind: 'raw', key: 'deployment-import-install', event: {} };
  } else if (
    input.kind === 'component' &&
    input.child === 'deployment-import-file' &&
    input.name === 'changed'
  ) {
    if (!sameValue(state.importer.observation, input.event)) {
      state.importer.installed = null;
      if (
        !state.current &&
        (state.notice === 'deploymentInstalled' || state.notice === 'deploymentInstallRejected')
      )
        state.notice = null;
    }
    state.importer.observation = input.event as FileObservation;
    if (state.importer.confirmation && !matchesImport(state.importer))
      state.importer.confirmation = null;
  } else if (
    input.kind === 'component' &&
    input.child === 'deployment-document-editor' &&
    input.name === 'changed' &&
    state.document
  ) {
    const next = observeDocument(state.document, input.event as EditorObservation);
    if (next !== null) readDocument(state, actions, next);
  } else if (input.kind === 'action') {
    const pending = state.pending;
    if (!pending || pending.key !== input.key) return frame(presentation, state, props);
    state.pending = null;
    const outcome = input.event as ActionOutcome,
      r = pending.request;
    if (r.kind === 'filter') {
      state.queryBasis = r.basis;
      if (
        outcome.status === 'succeeded' &&
        sameValue(r.basis, state.queryObservation?.snapshot.text)
      ) {
        state.query = (outcome.value as TextChunk).text.trim();
        state.listWindow = 0;
        state.focus = { sequence: (state.focus?.sequence ?? 0) + 1, target: { kind: 'start' } };
      } else if (outcome.status !== 'succeeded') state.notice = 'deploymentUnavailable';
    } else if (r.kind === 'restoreSettings') {
      if (outcome.status !== 'succeeded') {
        state.restoreFailed = true;
        state.notice = 'historyUnavailable';
        if (state.settings) readSettings(state, actions);
      } else {
        const items = outcome.value as CallerIntent[];
        for (const entry of items) {
          const recovery = entry.recovery as {
            kind?: string;
            bookmarkKey?: string;
            form?: SettingsBookmark;
          } | null;
          if (
            entry.reference.contract.name === 'maka.application.DeploymentManager' &&
            entry.reference.method === 'configure' &&
            recovery?.kind === 'settings/v1' &&
            recovery.bookmarkKey === (props.bookmarkKey ?? 'deployment') &&
            sameValue(recovery.form, settingsBookmark(state))
          )
            state.restoreOriginal = entry.reference;
        }
        const entry = items.at(-1);
        if (entry) {
          if (entry.cursor > r.after)
            request(
              state,
              actions,
              { kind: 'restoreSettings', after: entry.cursor },
              'history',
              'page',
              { after: entry.cursor, limit: 16 },
            );
          else {
            state.restoreFailed = true;
            state.notice = 'historyUnavailable';
          }
        } else if (state.settings && state.restoreOriginal) {
          const reference = state.restoreOriginal;
          state.current = {
            intent: {
              source: state.settings.source,
              configuration: {
                expectedRevision: state.settings.source.revision,
                instance: state.settings.instance.instance,
                changes: state.settings.changes as InstanceConfigure['changes'],
              },
            },
            reference,
          };
          request(state, actions, { kind: 'receipt', reference }, 'receipts', 'read', reference);
        } else if (state.settings) readSettings(state, actions);
      }
    } else if (r.kind === 'settings') {
      const settings = state.settings;
      if (
        settings &&
        sameValue(r.input.source, settings.source) &&
        sameValue(r.input.path, settings.path) &&
        r.input.after === settings.after
      ) {
        const page =
          outcome.status === 'succeeded' ? (outcome.value as InstanceSettingsPage) : null;
        if (
          page &&
          sameValue(page.source, r.input.source) &&
          sameValue(page.instance, r.input.instance) &&
          sameValue(page.path, r.input.path)
        )
          settings.page =
            r.input.after && settings.page
              ? { ...page, fields: [...settings.page.fields, ...page.fields] }
              : page;
        else settings.notice = 'deploymentUnavailable';
      }
    } else if (r.kind === 'urlText') {
      const chunk = outcome.status === 'succeeded' ? (outcome.value as TextChunk) : null;
      if (
        chunk &&
        sameValue(chunk.metadata, r.captured.snapshot.text) &&
        new TextEncoder().encode(chunk.text).length === r.captured.snapshot.text.bytes &&
        sameValue(r.captured, state.importer.urlObservation) &&
        sameValue(r.source, state.page?.source) &&
        state.importer.mode === 'url'
      ) {
        const url = chunk.text.trim();
        if (/^https?:\/\/[^\s]+$/.test(url)) {
          state.importer.confirmation = { source: r.source, captured: r.captured, url };
          state.notice = null;
        } else state.notice = 'deploymentUrlInvalid';
      } else state.notice = 'deploymentFileFailed';
    } else if (r.kind === 'settingText') {
      if (state.settings)
        acceptSettingText(
          state.settings,
          r.captured,
          outcome.status === 'succeeded' ? (outcome.value as TextChunk) : null,
          r.path,
        );
    } else if (r.kind === 'settingsReload') {
      if (outcome.status === 'succeeded' && state.settings) {
        const next = outcome.value as ManagedInstancesPage;
        state.page = next;
        state.settings = instanceSettings(next.source, state.settings.instance);
        state.notice = null;
        state.diagnostic = null;
        state.conflict = false;
        readSettings(state, actions);
      } else if (state.settings) state.settings.notice = 'deploymentUnavailable';
    } else if (r.kind === 'document') {
      const editor = state.document;
      if (editor && sameValue(editor.source, r.source) && editor.offset === r.offset) {
        if (outcome.status === 'succeeded')
          acceptDocumentChunk(editor, outcome.value as DeploymentDocumentChunk);
        else editor.phase = 'failed';
      }
    } else if (r.kind === 'documentReload') {
      if (outcome.status === 'succeeded' && state.document) {
        const value = outcome.value as ManagedInstancesPage,
          old = state.document;
        state.document = documentEditor(value.source);
        state.document.observation = old.observation;
        state.document.sequence = old.sequence;
        state.page = value;
        state.listWindow = 0;
        state.conflict = false;
        state.notice = null;
        state.diagnostic = null;
        readDocument(state, actions, 0);
      } else state.notice = 'deploymentUnavailable';
    } else if (r.kind === 'page') {
      if (outcome.status === 'succeeded') {
        const value = outcome.value as ManagedInstancesPage;
        if (r.input.kind === 'continue' && !sameValue(value.source, r.input.source))
          state.notice = 'deploymentUnavailable';
        else {
          if (r.movement === 'reset') {
            state.listWindow = 0;
            state.conflict = false;
            state.page = value;
            state.selected =
              value.instances.find((entry) => sameValue(entry.instance, state.selected))
                ?.instance ??
              value.instances[0]?.instance ??
              null;
            if (state.selected)
              state.focus = {
                sequence: (state.focus?.sequence ?? 0) + 1,
                target: {
                  kind: 'focus_item',
                  key:
                    'deployment-select-' +
                    value.instances.findIndex((entry) => sameValue(entry.instance, state.selected)),
                },
              };
          } else {
            state.page = {
              ...value,
              instances: [...(state.page?.instances ?? []), ...value.instances],
            };
          }
          if (
            state.importer.confirmation &&
            !sameValue(state.importer.confirmation.source, value.source)
          )
            state.importer.confirmation = null;
        }
      } else state.notice = 'deploymentUnavailable';
    } else if (r.kind === 'mutate') {
      if (outcome.status === 'operation') settle(state, outcome.reply, actions);
      else if (outcome.status === 'not_accepted') {
        state.current = null;
        state.notice = 'deploymentRejected';
      } else state.notice = 'deploymentUnknown';
    } else if (r.kind === 'receipt') {
      if (
        outcome.status === 'succeeded' &&
        sameValue((outcome.value as OperationReply).reference, r.reference)
      ) {
        const reply = outcome.value as OperationReply;
        if (state.current?.reference && sameValue(state.current.reference, r.reference))
          settle(state, reply, actions);
        const entry = state.earlier.find((entry) => sameValue(entry.reference, r.reference));
        if (entry) entry.reply = reply;
      } else state.notice = 'deploymentUnknown';
    } else if (outcome.status === 'succeeded') {
      const items = outcome.value as CallerIntent[];
      state.historyNext = items.at(-1)?.cursor ?? null;
      state.earlier = [
        ...(r.after ? state.earlier : []),
        ...items
          .filter(
            (entry) =>
              entry.reference.contract.name === 'maka.application.DeploymentManager' &&
              (entry.reference.method === 'setEnabled' ||
                entry.reference.method === 'replace' ||
                entry.reference.method === 'configure' ||
                entry.reference.method === 'install' ||
                entry.reference.method === 'installUrl'),
          )
          .map((entry) => ({ cursor: entry.cursor, reference: entry.reference, reply: null })),
      ];
    } else state.notice = 'historyUnavailable';
  } else if (input.kind === 'raw') {
    // Until the original draft/intent has been reconciled, editing it would
    // break the correlation needed to query an accepted save after reload.
    if (state.restoreFailed || !state.bookmark.restored) return frame(presentation, state, props);
    if (
      state.document?.phase === 'loading' &&
      input.key !== 'deployment-document-back' &&
      input.key !== 'deployment-back'
    )
      return frame(presentation, state, props);
    const settings = state.settings;
    if (settings?.open && input.key.startsWith('instance-')) {
      if (state.pending || state.current) return frame(presentation, state, props);
      const key = input.key;
      if (key === 'instance-setting-cancel' || key === 'instance-setting-dialog') {
        settings.field = null;
        settings.discard = null;
      } else if (key === 'instance-settings-back' && settings.changes.length) {
        settings.discard = 'close';
      } else if (key === 'instance-settings-back') {
        settings.open = false;
        settings.field = null;
        page(state, actions, { kind: 'start' });
      } else if (key === 'instance-settings-reload') {
        if (settings.changes.length) settings.discard = 'reload';
        else
          request(state, actions, { kind: 'settingsReload' }, 'management', 'instances', {
            kind: 'start',
          });
      } else if (key === 'instance-settings-discard' && settings.discard) {
        const target = settings.discard;
        settings.discard = null;
        settings.changes = [];
        if (target === 'reload')
          request(state, actions, { kind: 'settingsReload' }, 'management', 'instances', {
            kind: 'start',
          });
        else if (target === 'close') {
          settings.open = false;
          settings.field = null;
          page(state, actions, { kind: 'start' });
        } else {
          state.settings = null;
          openSettings(state, actions, target);
        }
      } else if (key === 'instance-settings-up' && settings.path.length) {
        settings.path.pop();
        settings.after = null;

        settings.page = null;
        readSettings(state, actions);
      } else if (
        key === 'instance-settings-save' &&
        settings.changes.length &&
        !settings.conflict &&
        !settings.field &&
        !state.restoreFailed &&
        props.navigationReady !== false &&
        bookmarkReady(state.bookmark, settingsBookmark(state))
      ) {
        const configuration: InstanceConfigure = {
          expectedRevision: settings.source.revision,
          instance: settings.instance.instance,
          changes: JSON.parse(JSON.stringify(settings.changes)) as InstanceConfigure['changes'],
        };
        state.current = {
          intent: { source: settings.source, configuration },
          reference: null,
        };
        state.notice = null;
        state.diagnostic = null;
        request(
          state,
          actions,
          { kind: 'mutate' },
          'management',
          'configure',
          configuration,
          {},
          {
            kind: 'settings/v1',
            bookmarkKey: props.bookmarkKey ?? 'deployment',
            form: settingsBookmark(state),
          },
        );
      } else if (
        key === 'instance-setting-apply' &&
        settings.field &&
        settings.initialized &&
        !settings.command &&
        settings.observation &&
        !settings.observation.snapshot.marked
      ) {
        const captured = settings.observation;
        if (captured.snapshot.text.bytes > 16384) settings.notice = 'settingsReadFailed';
        else
          request(
            state,
            actions,
            { kind: 'settingText', captured, path: settings.field.path },
            'reader',
            'read',
            { start: 0, end: captured.snapshot.text.bytes },
            {
              text: {
                child: 'instance-setting-editor',
                method: 'snapshot',
                input: {
                  kind: 'at_revision',
                  owner: captured.snapshot.text.owner,
                  revision: captured.revision,
                },
              },
            },
          );
      } else if (!settings.conflict && !settings.field && !settings.discard) {
        const match = /^instance-setting-(?:(choice|clear|null)-)?(0|[1-9][0-9]*)$/.exec(key);
        const field = match && settings.page?.fields[Number(match[2])];
        if (field?.editable) {
          const path = [...settings.path, field.key];
          if (
            match![1] === 'clear' &&
            !field.required &&
            (settings.path.length > 0 || settings.instance.path.length === 1)
          )
            editSetting(settings, path, null);
          else if (match![1] === 'null' && field.nullable)
            editSetting(settings, path, { kind: 'null' });
          else if (match![1] === 'choice') {
            const at = (input.event as { key: string }).key;
            const options = settingOptions(field);
            if (/^(0|[1-9][0-9]*)$/.test(at) && options[Number(at)])
              editSetting(settings, path, options[Number(at)]!);
          } else if (field.kind === 'object' || field.kind === 'array') {
            if (field.present && path.length < 16) {
              settings.path = path;
              settings.after = null;

              settings.page = null;
              readSettings(state, actions);
            }
          } else if (field.kind === 'boolean') {
            const value = settingValue(settings, field);
            editSetting(settings, path, {
              kind: 'boolean',
              value: !(value?.kind === 'boolean' && value.value),
            });
          } else if (field.kind === 'null') editSetting(settings, path, { kind: 'null' });
          else openSetting(settings, field);
        }
      }
      return frame(presentation, state, props, actions, events);
    }
    let key = input.key;
    if (key === 'deployment-dialog')
      key = state.confirmation ? 'deployment-cancel' : 'deployment-history-back';
    if (key === 'deployment-document-dialog') key = 'deployment-document-cancel';
    if (key === 'deployment-import-dialog') key = 'deployment-import-cancel';
    if (/^deployment-select-(0|[1-9][0-9]*)$/.test(key) && !state.pending && !state.current) {
      const at = Number(key.slice('deployment-select-'.length));
      const entry = state.page?.instances[at];
      if (entry) {
        state.selected = entry.instance;
        if ((input.event as { kind?: string } | null)?.kind === 'submit')
          key = entry.structural ? 'deployment-toggle-' + at : 'deployment-configure-' + at;
      }
    }
    if (key === 'deployment-import-check') key = 'deployment-check';
    if (key === 'deployment-import-refresh') key = 'deployment-refresh';
    if (key === 'deployment-import-advanced') key = 'deployment-advanced';
    const importer = state.importer;
    if (
      (key === 'deployment-import-local' || key === 'deployment-import-remote') &&
      !state.pending &&
      !state.current &&
      !importer.confirmation
    ) {
      importer.mode = key === 'deployment-import-local' ? 'file' : 'url';
      importer.installed = null;
      state.notice = null;
    }
    if (key === 'deployment-import-history') {
      importer.open = false;
      key = 'deployment-history';
    }
    if (key === 'deployment-import-back') {
      importer.open = false;
      importer.confirmation = null;
    } else if (key === 'deployment-import' && !state.pending && !state.current && state.page) {
      importer.open = true;
      importer.confirmation = null;
      state.documentOpen = false;
      state.historyOpen = false;
    } else if (key === 'deployment-import-cancel' && !state.pending && !state.current) {
      importer.confirmation = null;
    } else if (
      key === 'deployment-import-install' &&
      state.page &&
      canInstall(importer, !!state.pending || !!state.current, state.conflict)
    ) {
      if (importer.mode === 'file') {
        importer.confirmation = JSON.parse(
          JSON.stringify({ source: state.page.source, file: importer.observation }),
        ) as InstallIntent;
      } else {
        const captured = importer.urlObservation!,
          basis = captured.snapshot.text;
        request(
          state,
          actions,
          { kind: 'urlText', captured, source: state.page.source },
          'reader',
          'read',
          { start: 0, end: basis.bytes },
          {
            text: {
              child: 'deployment-import-url',
              method: 'snapshot',
              input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
            },
          },
        );
      }
    } else if (
      key === 'deployment-import-confirm' &&
      importer.confirmation &&
      state.page &&
      canInstall(importer, !!state.pending || !!state.current, state.conflict) &&
      matchesImport(importer) &&
      sameValue(importer.confirmation.source, state.page.source)
    ) {
      const intent = importer.confirmation;
      importer.confirmation = null;
      state.current = { intent, reference: null };
      state.notice = null;
      request(
        state,
        actions,
        { kind: 'mutate' },
        'management',
        'url' in intent ? 'installUrl' : 'install',
        {
          expectedRevision: intent.source.revision,
          ...('url' in intent ? { url: intent.url, sha256: null } : {}),
        },
        'url' in intent
          ? {}
          : {
              package: {
                child: 'deployment-import-file',
                method: 'snapshot',
                input: { owner: intent.file.owner, revision: intent.file.revision },
              },
            },
      );
    }
    if (key === 'deployment-document-check') key = 'deployment-check';
    if (key === 'deployment-document-history') {
      state.documentOpen = false;
      key = 'deployment-history';
    }
    const editor = state.document;
    if (key === 'deployment-document-back') {
      state.documentOpen = false;
      if (editor) {
        editor.confirmation = null;
        editor.reloadConfirmation = false;
      }
    } else if (key === 'deployment-advanced' && !state.pending && !state.current && state.page) {
      state.historyOpen = false;
      importer.open = false;
      importer.confirmation = null;
      state.documentOpen = true;
      if (!editor) {
        state.notice = null;
        state.diagnostic = null;
        state.document = documentEditor(state.page.source);
        readDocument(state, actions, 0);
      }
    } else if (key === 'deployment-document-cancel' && editor && !state.pending && !state.current) {
      editor.confirmation = null;
      editor.reloadConfirmation = false;
    } else if (
      key === 'deployment-document-save' &&
      editor &&
      canSaveDocument(editor, !!state.pending || !!state.current)
    ) {
      editor.confirmation = JSON.parse(JSON.stringify(editor.observation)) as EditorObservation;
    } else if (
      key === 'deployment-document-confirm' &&
      editor?.confirmation &&
      canSaveDocument(editor, !!state.pending || !!state.current)
    ) {
      const frozen = editor.confirmation;
      editor.confirmation = null;
      state.current = { intent: { source: editor.source, editor: frozen }, reference: null };
      state.notice = null;
      state.diagnostic = null;
      request(
        state,
        actions,
        { kind: 'mutate' },
        'management',
        'replace',
        { expectedRevision: editor.source.revision },
        {
          document: {
            child: 'deployment-document-editor',
            method: 'snapshot',
            input: {
              kind: 'at_revision',
              owner: frozen.snapshot.text.owner,
              revision: frozen.revision,
            },
          },
        },
      );
    } else if (
      key === 'deployment-document-reload' &&
      editor &&
      !state.pending &&
      !state.current &&
      editor.phase !== 'loading'
    ) {
      editor.reloadConfirmation = true;
    } else if (
      key === 'deployment-document-reload-confirm' &&
      editor?.reloadConfirmation &&
      !state.pending &&
      !state.current
    ) {
      editor.reloadConfirmation = false;
      request(state, actions, { kind: 'documentReload' }, 'management', 'instances', {
        kind: 'start',
      });
    }
    if (
      key === 'deployment-back' &&
      !state.settings?.open &&
      !(state.current && 'configuration' in state.current.intent)
    )
      events.push({ name: 'closed', value: {} });
    else if (key === 'deployment-cancel' && !state.current) state.confirmation = null;
    else if (key === 'deployment-history-back') state.historyOpen = false;
    else if (key === 'deployment-history' && !state.pending) {
      state.historyOpen = true;
      resetScroll(state, 'deployment-history-reading');
      resetScroll(state, 'deployment-list');
      request(state, actions, { kind: 'history', after: 0 }, 'history', 'page', {
        after: 0,
        limit: 2,
      });
    } else if (key === 'deployment-history-refresh' && !state.pending) {
      resetScroll(state, 'deployment-history-reading');
      resetScroll(state, 'deployment-list');
      request(state, actions, { kind: 'history', after: 0 }, 'history', 'page', {
        after: 0,
        limit: 2,
      });
    } else if (key === 'deployment-history-more' && !state.pending && state.historyNext !== null)
      request(state, actions, { kind: 'history', after: state.historyNext }, 'history', 'page', {
        after: state.historyNext,
        limit: 2,
      });
    else if (/^deployment-earlier-check-(0|[1-9][0-9]*)$/.test(key) && !state.pending) {
      const entry = state.earlier[Number(key.slice('deployment-earlier-check-'.length))];
      if (entry)
        request(
          state,
          actions,
          { kind: 'receipt', reference: entry.reference },
          'receipts',
          'read',
          entry.reference,
        );
    } else if (key === 'deployment-check' && !state.pending && state.current?.reference)
      request(
        state,
        actions,
        { kind: 'receipt', reference: state.current.reference },
        'receipts',
        'read',
        state.current.reference,
      );
    else if (!state.pending && !state.current) {
      if (/^deployment-configure-(0|[1-9][0-9]*)$/.test(key) && state.page && !state.conflict) {
        const instance = state.page.instances[Number(key.slice('deployment-configure-'.length))];
        if (instance) openSettings(state, actions, instance);
      } else if (key === 'deployment-refresh') {
        state.confirmation = null;
        page(state, actions, { kind: 'start' });
      } else if (key === 'deployment-confirm' && state.confirmation) {
        const intent = state.confirmation;
        state.confirmation = null;
        state.current = { intent, reference: null };
        state.notice = null;
        request(state, actions, { kind: 'mutate' }, 'management', 'setEnabled', {
          expectedRevision: intent.source.revision,
          instance: intent.instance.instance,
          enabled: intent.enabled,
        });
      } else if (/^deployment-toggle-(0|[1-9][0-9]*)$/.test(key) && state.page && !state.conflict) {
        const instance = state.page.instances[Number(key.slice('deployment-toggle-'.length))];
        if (instance && !(instance.disabled && instance.parentDisabled))
          state.confirmation = {
            source: state.page.source,
            instance,
            enabled: instance.disabled,
          };
      }
    }
  }
  return frame(presentation, state, props, actions, events);
}
