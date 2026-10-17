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
  Emission,
  InputProps,
  UsageModelTotals,
} from '../../../packages/plugin-sdk/src/index.js';
import { bookmarkView } from '../../shared/bookmark.js';
import { type InsightsProps } from '../../shared/contracts.js';
import { copy, tuiText } from './locale.js';
import { pageHeader } from '../../shared/page-shell.js';
import { sameValue } from '../../shared/value.js';
import { compactLabel, output, type Node, type Presentation } from '../../shared/view.js';
import { refresh } from './operations.js';
import { fields, money, type State, type Tab } from './state.js';

export function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: InsightsProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  if (
    !sameValue(state.scope, props.scope) &&
    !state.pending &&
    !state.current &&
    !state.restoring
  ) {
    state.scope = props.scope;
    state.range.sessionId = props.scope?.sessionId ?? null;
    refresh(state, actions);
  }
  const ui = presentation.paint(props.theme),
    t = (key: string) => tuiText(props.locale, key),
    text = copy(props.locale);
  const busy =
    !props.open ||
    props.busy ||
    !!state.pending ||
    !!state.current ||
    state.restoring ||
    !!state.capture;
  const label = (id: string, value: string, muted = false) =>
    ui.text(id, value, muted ? props.theme.muted : props.theme.text);
  const button = (key: string, value: string, disabled = busy) =>
    ui.button(key, value, disabled, false, { background: 0, align: 'start' });
  const cost = (m: UsageModelTotals) =>
    money(m.cost.knownUsd) +
    (m.cost.unvalued ? ` + ${m.cost.unvalued} ${t('insights-unvalued')}` : '');
  const choose = (
    key: string,
    title: string,
    options: { key: string; label: string }[],
    current: string,
    disabled: boolean,
  ) => ui.choice(key, title, options, current, disabled);
  const rows: N[] = [];
  const tabs: Tab[] = ['overview', 'activity', 'pricing'];
  const tabBar = ui.box(
    'insights-tabs',
    tabs.map((tab) => ui.button('tab-' + tab, t('insights-' + tab), busy, state.tab === tab)),
    { row: true, gap: 1 },
  );
  if (state.tab !== 'pricing') {
    rows.push(
      choose(
        'period',
        t('insights-range'),
        ['1', '7', '30', 'all'].map((key) => ({
          key,
          label: t('insights-range-' + key),
        })),
        state.period,
        busy,
      ),
    );
    rows.push(
      ui.box(
        'insights-scope',
        [
          label(
            'insights-scope-label',
            state.range.sessionId
              ? state.scope?.label || state.range.sessionId
              : t('insights-all-sessions'),
            true,
          ),
          ...(state.scope?.sessionId
            ? [
                button(
                  'scope-toggle',
                  t(state.range.sessionId ? 'insights-all-sessions' : 'insights-this-session'),
                ),
              ]
            : []),
          button('refresh', t('insights-refresh')),
        ],
        { gap: 0 },
      ),
    );
  }
  if (state.tab === 'overview' && state.report) {
    const s = state.report.summary,
      m = s.models;
    rows.push(label('usage-cost', cost(m)), label('usage-estimate', t('insights-estimate'), true));
    const metrics: N[] = [];
    for (const [key, value] of [
      ['input', m.input],
      ['output', m.output],
      ['cacheRead', m.cacheRead],
      ['cacheWrite', m.cacheWrite],
      ['reasoning', m.reasoning],
    ] as const)
      metrics.push(
        label(
          'usage-' + key,
          `${t('insights-' + key)}: ${value.known}${value.missing ? ` + ${value.missing} ${t('insights-unreported')}` : ''}`,
          true,
        ),
      );
    metrics.push(
      label(
        'usage-calls',
        `${t('insights-models')}: ${m.calls} · ${t('insights-tools')}: ${s.tools.calls}`,
        true,
      ),
    );
    metrics.push(
      label(
        'usage-outcomes',
        `${t('insights-error')}: ${m.error + s.tools.error} · ${t('insights-aborted')}: ${m.aborted} · ${t('insights-unknown-status')}: ${m.unknown + s.tools.unknown} · ${t('insights-rejected')}: ${s.tools.rejected}`,
        true,
      ),
    );
    metrics.push(
      label(
        'usage-pending',
        `${t('insights-pending')}: ${s.pending.models + s.pending.tools}`,
        true,
      ),
    );
    rows.push(ui.box('usage-metrics', metrics, { gap: 0 }));
    rows.push(
      choose(
        'group',
        t('insights-group'),
        ['byProvider', 'byModel', 'byTool'].map((key) => ({
          key,
          label: t('insights-' + key),
        })),
        state.group,
        busy,
      ),
    );
    const items =
      state.group === 'byProvider'
        ? s.byProvider.map((x) => [x.providerId ?? t('insights-unknown-status'), cost(x.totals)])
        : state.group === 'byModel'
          ? s.byModel.map((x) => [x.modelId, `${cost(x.totals)} · ${x.totals.calls}`])
          : s.byTool.map((x) => [
              x.name,
              `${x.totals.calls} · ${t('insights-error')}: ${x.totals.error} · ${t('insights-unknown-status')}: ${x.totals.unknown} · ${t('insights-rejected')}: ${x.totals.rejected}`,
            ]);
    items.forEach(([name, value], i) =>
      rows.push(label('group-' + i, `${compactLabel(name!, 100)}  ${value}`)),
    );
    rows.push(
      ui.box(
        'group-paging',
        [
          button('group-prev', t('insights-previous'), busy || state.groupOffsets.length < 2),
          button('group-next', t('insights-next'), busy || state.report.nextOffset == null),
        ],
        { row: true, gap: 1 },
      ),
    );
  } else if (state.tab === 'activity') {
    rows.push(
      choose(
        'kind',
        t('insights-kind'),
        ['all', 'model', 'tool'].map((key) => ({
          key,
          label: t('insights-' + key),
        })),
        state.selection.kind ?? 'all',
        busy,
      ),
    );
    rows.push(
      choose(
        'status',
        t('insights-status'),
        ['all', 'success', 'error', 'aborted', 'unknown', 'rejected'].map((key) => ({
          key,
          label: t('insights-' + (key === 'unknown' ? 'unknown-status' : key)),
        })),
        state.selection.status ?? 'all',
        busy,
      ),
    );
    if (state.detail) {
      const a = state.detail;
      rows.push(button('detail-close', text.back));
      const detail: [string, unknown][] = [
        ['insights-request', a.attempt.requestId],
        ['insights-completed', new Date(a.attempt.completedAt).toISOString()],
        ['insights-binding', a.attempt.binding ? JSON.stringify(a.attempt.binding) : '—'],
      ];
      if (a.kind === 'model')
        detail.push(
          ['insights-model', a.attempt.modelId],
          ['insights-origin', JSON.stringify(a.attempt.origin)],
          ['insights-started', new Date(a.attempt.startedAt).toISOString()],
          [
            'insights-duration',
            a.attempt.outcome === 'unknown'
              ? '—'
              : Math.max(0, a.attempt.completedAt - a.attempt.startedAt),
          ],
          ...(
            [
              ['input', 'input_tokens'],
              ['output', 'output_tokens'],
              ['cacheRead', 'cache_read_tokens'],
              ['cacheWrite', 'cache_write_tokens'],
              ['reasoning', 'reasoning_tokens'],
            ] as const
          ).map(([label, key]): [string, unknown] => [
            'insights-' + label,
            a.attempt.usage[key] ?? t('insights-unreported'),
          ]),
          ['insights-session', a.attempt.sessionId ?? '—'],
          [
            'insights-status',
            t(
              'insights-' +
                (a.attempt.outcome === 'unknown' ? 'unknown-status' : a.attempt.outcome),
            ),
          ],
          [
            'insights-cost',
            a.attempt.costUsd == null ? t('insights-unvalued') : money(a.attempt.costUsd),
          ],
          [
            'insights-quote',
            a.attempt.quote?.pricing
              ? JSON.stringify(a.attempt.quote.pricing)
              : t('insights-unpriced'),
          ],
        );
      else
        detail.push(
          ['insights-tool', a.attempt.name],
          ['insights-session', a.attempt.invocation.session_id],
          ['insights-origin', JSON.stringify(a.attempt.invocation)],
          ['insights-call', JSON.stringify(a.attempt.call)],
          ...(a.attempt.result.kind === 'rejected'
            ? [['insights-rejection', a.attempt.result.reason] as [string, unknown]]
            : ([
                ['insights-started', new Date(a.attempt.result.startedAt).toISOString()],
                [
                  'insights-duration',
                  a.attempt.result.outcome === 'unknown'
                    ? '—'
                    : Math.max(0, a.attempt.completedAt - a.attempt.result.startedAt),
                ],
              ] as [string, unknown][])),
          [
            'insights-status',
            t(
              'insights-' +
                (a.attempt.result.kind === 'rejected'
                  ? 'rejected'
                  : a.attempt.result.outcome === 'unknown'
                    ? 'unknown-status'
                    : a.attempt.result.outcome),
            ),
          ],
        );
      detail.forEach(([key, value], i) => rows.push(label('detail-' + i, `${t(key)}: ${value}`)));
    } else if (state.activity) {
      rows.push(
        label('activity-count', String(state.activity.total) + ' ' + t('insights-attempts'), true),
      );
      state.activity.attempts.forEach((a, i) =>
        rows.push(
          button(
            'attempt-' + i,
            compactLabel(
              a.kind === 'model'
                ? `${a.attempt.modelId} · ${t('insights-' + (a.attempt.outcome === 'unknown' ? 'unknown-status' : a.attempt.outcome))} · ${a.attempt.costUsd == null ? t('insights-unvalued') : money(a.attempt.costUsd)}`
                : `${a.attempt.name} · ${t('insights-' + (a.attempt.result.kind === 'rejected' ? 'rejected' : a.attempt.result.outcome === 'unknown' ? 'unknown-status' : a.attempt.result.outcome))}`,
              180,
            ),
          ),
        ),
      );
      rows.push(
        ui.box(
          'activity-paging',
          [
            button('activity-prev', t('insights-previous'), busy || state.offsets.length < 2),
            button('activity-next', t('insights-next'), busy || state.activity.nextOffset == null),
          ],
          { row: true, gap: 1 },
        ),
      );
    }
  } else if (state.tab === 'pricing' && !state.editing) {
    rows.push(
      button('price-new', t('insights-new')),
      button('price-refresh', t('insights-refresh')),
    );
    state.prices?.entries.forEach((entry, i) =>
      rows.push(
        button(
          'price-' + i,
          `${compactLabel(entry.pricing.modelKey, 100)}  $${entry.pricing.inputUsdPer1M} / $${entry.pricing.outputUsdPer1M}${entry.source === 'custom' ? ' *' : ''}`,
        ),
      ),
    );
    rows.push(
      ui.box(
        'prices-paging',
        [
          button('prices-prev', t('insights-previous'), busy || state.priceOffsets.length < 2),
          button('prices-next', t('insights-next'), busy || state.prices?.nextOffset == null),
        ],
        { row: true, gap: 1 },
      ),
    );
  }
  const editorRows: N[] = [];
  if (state.editing) {
    editorRows.push(label('price-unit', t('insights-unit'), true));
    for (const name of fields)
      editorRows.push(
        ui.box(
          'price-field-' + name,
          [
            label('price-label-' + name, t('insights-' + name), true),
            presentation.slot(
              `insights-field-${state.editing.key}-${name}`,
              'input',
              {
                menu: editorMenu(props.theme, props.locale),
                appearance: {
                  ...props.theme.input,
                  borderWidth: 0,
                  padding: { top: 0, right: 0, bottom: 0, left: 0 },
                },
                accessibilityLabel: t('insights-' + name),
                placeholder: '',
                autoFocus: name === 'model',
                tabIndex: fields.indexOf(name) + 1,
                readOnly: name === 'model' && !!state.editing.entry,
                disabled: busy || !state.fields[name].ready || !!state.fields[name].command,
                command: state.fields[name].command,
              } satisfies InputProps,
              { changed: 'changed' },
            ),
          ],
          { gap: 0 },
        ),
      );
    editorRows.push(
      button(
        'price-save',
        t('insights-price-save'),
        busy || fields.some((name) => !state.fields[name].ready || !!state.fields[name].command),
      ),
    );
    if (state.editing.entry?.source === 'custom')
      editorRows.push(button('price-remove', t('insights-remove')));
    if (state.confirm) {
      editorRows.push(
        label(
          'price-remove-effect',
          t(
            state.editing.entry?.source === 'custom' &&
              state.editing.entry.resetEffect === 'restore_builtin'
              ? 'insights-restore-builtin'
              : 'insights-become-unpriced',
          ),
        ),
      );
      editorRows.push(button('price-confirm', t('insights-confirm')));
    }
    editorRows.push(button('price-close', t('insights-close')));
  }
  const feedback: N[] = [];
  if (state.bookmark.observed?.state.kind === 'failed')
    feedback.push(button('view-retry', t('insights-view-retry'), false));
  if (state.notice) feedback.push(label('insights-notice', t(state.notice), true));
  if (state.current)
    feedback.push(
      button('receipt', text.executionReceipt, !!state.pending || !state.current.reference),
    );
  if ((state.restoring || (state.current && !state.current.reference)) && !state.pending)
    feedback.push(button('history', text.refreshDefaultOperations, false));
  const result = output(
    state,
    ui.box(
      'insights-root',
      [
        ...pageHeader(presentation, state, props, events, t('settings-insights')),
        tabBar,
        ui.box(
          'insights-checkpoint',
          [
            bookmarkView(
              presentation,
              state.bookmark,
              'insights-bookmark',
              'insights/view',
              state.savedView,
            ),
          ],
          { visible: false },
        ),
        ui.box(
          'insights-view-actions',
          [
            button(
              'view-save',
              t(
                state.bookmark.saved &&
                  !state.bookmark.command &&
                  sameValue(state.bookmark.saved, {
                    tab: state.tab,
                    period: state.period,
                    group: state.group,
                    selection: state.selection,
                  })
                  ? 'insights-view-saved'
                  : 'insights-view-save',
              ),
              busy || state.bookmark.observed?.state.kind !== 'ready' || !!state.bookmark.command,
            ),
            button(
              'view-load',
              t('insights-view-load'),
              busy || !state.bookmark.saved || !!state.bookmark.command,
            ),
          ],
          { row: true, gap: 1 },
        ),
        ...feedback,
        ui.box(
          'insights-search',
          [
            presentation.slot(
              'insights-search-input',
              'input',
              {
                menu: editorMenu(props.theme, props.locale),
                appearance: {
                  ...props.theme.input,
                  borderWidth: 0,
                  padding: { top: 0, right: 0, bottom: 0, left: 0 },
                },
                accessibilityLabel: t('insights-search'),
                placeholder: t('insights-search'),
                autoFocus: false,
                tabIndex: 0,
                readOnly: false,
                disabled: busy || !state.search.ready || !!state.search.command,
                command: state.search.command,
              } satisfies InputProps,
              { changed: 'changed' },
            ),
            button(
              'search-apply',
              t('insights-search-apply'),
              busy || !state.search.ready || !!state.search.command,
            ),
          ],
          { visible: state.tab === 'activity', gap: 0 },
        ),
        ui.box('insights-content-' + state.tab, rows, {
          visible: state.tab !== 'pricing' || !state.editing,
          gap: 1,
          scroll: 'manual',
          terminal: { grow: 1 },
        }),
        ...(state.editing
          ? [
              ui.box('insights-price-editor-' + state.editing.key, editorRows, {
                visible: state.tab === 'pricing',
                gap: 1,
                scroll: 'manual',
                terminal: { grow: 1 },
              }),
            ]
          : []),
      ],
      {
        visible: props.open,
        gap: 1,
        terminal: { grow: 1 },
        shortcuts: [{ key: 'escape', modifiers: [], input: 'back', enabled: true }],
      },
    ),
    events,
  );
  result.actions = actions;
  return result;
}
