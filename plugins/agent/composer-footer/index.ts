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
  AgentSessionMetrics,
  Component,
  AgentDataSource,
} from '../../../packages/plugin-sdk/src/index.js';
import type { Theme } from '../../shared/contracts.js';
import { composerFooterContract } from '../../shared/contracts.js';
import { tuiText } from './locale.js';
import { output, type Node, type Presentation } from '../../shared/view.js';

interface Props {
  theme: Theme;
  locale: string;
  sessionId: string | null;
  dataSource: AgentDataSource | null;
  running: boolean;
  compacting: boolean;
  metrics: AgentSessionMetrics | null;
}
interface State {
  revision: number;
}
const tokens = (value: number) =>
  value >= 1_000_000
    ? `${(value / 1_000_000).toFixed(1)}M`
    : value >= 1000
      ? `${(value / 1000).toFixed(1).replace(/\.0$/, '')}k`
      : String(value);
export function footer<N extends Node>(presentation: Presentation<N>): Component<State, Props, N> {
  const frame = (state: State, props: Props) => {
    const { metrics, theme, locale } = props;
    let context = '';
    const used = metrics?.contextTokens;
    const capacity = metrics?.contextWindow;
    const last = metrics?.contextLastKnown ? ` (${tuiText(locale, 'chat-context-last')})` : '';
    if (used !== null && used !== undefined) {
      const amount = `${metrics?.approximate ? '≈' : ''}${tokens(used)}`;
      context = `${tuiText(locale, 'chat-context')} ${amount}${capacity ? ` / ${tokens(capacity)} · ${Math.round((100 * used) / capacity)}%` : ''}${last}`;
    }
    if (props.compacting && !context) context = tuiText(locale, 'chat-compacting');
    if (props.running && !context)
      context = `${tuiText(locale, 'chat-context')} —${capacity ? ` / ${tokens(capacity)}` : ''}`;
    const rate = metrics?.outputTokensPerSecond;
    const speed = rate != null && Number.isFinite(rate) ? `${rate.toFixed(1)} tok/s` : '';
    const ui = presentation.paint(theme);
    const hint = tuiText(
      locale,
      metrics?.contextLastKnown ? 'chat-context-last-hint' : 'chat-metrics-hint',
    );
    const contextColor = props.compacting ? theme.accent : theme.muted;
    const line = (key: string, text: string, color = contextColor) =>
      ui.text(key, text, color, 400, { truncate: true }, hint);
    const reducedLine = (key: string, text: string) =>
      ui.box(
        `${key}-row`,
        [
          ...(text ? [line(`${key}-text`, text)] : []),
          ...(speed ? [line(`${key}-rate`, `${text ? ' · ' : ''}${speed}`, theme.muted)] : []),
        ],
        { row: true, gap: 0 },
      );
    const full = () =>
      ui.box(
        'session-metrics-text',
        [
          ...(context ? [line('session-context-text', context)] : []),
          ...(speed
            ? [line('session-rate-text', `${context ? ' · ' : ''}${speed}`, theme.muted)]
            : []),
        ],
        { row: true, gap: 0 },
      );

    const layout = (basis: number) => ({
      basis,
      grow: 0,
      shrink: false,
      wrap: false,
      basis_fraction: null,
    });
    // Native layout uses the width left by the controls, including sidebar changes.
    // Zero-height alternatives do not paint or participate in hit testing.
    const compact =
      used != null
        ? capacity
          ? `${metrics?.approximate ? '≈' : ''}${Math.round((100 * used) / capacity)}%${last}`
          : `${tuiText(locale, 'chat-context')} ${metrics?.approximate ? '≈' : ''}${tokens(used)}${last}`
        : props.compacting
          ? tuiText(locale, 'chat-compacting')
          : props.running
            ? `${tuiText(locale, 'chat-context')} —`
            : '';
    const reduced = ui.box(
      'metrics-reduced',
      [
        ui.box('metrics-percent', [reducedLine('metrics-percent', compact)], {
          terminal: layout(compact || speed ? 1 : 0),
        }),
      ],
      {
        gap: 0,
        terminal: layout(1),
      },
    );
    return output(
      state,
      ui.box(
        'metrics-responsive',
        [
          reduced,
          ui.box('metrics-full', [full()], {
            terminal: layout(0),
          }),
        ],
        {
          gap: 0,
          wide: {
            min_width: 64,
            direction: 'column',
            children: {
              'metrics-reduced': layout(0),
              'metrics-full': layout(context || speed ? 1 : 0),
            },
          },
        },
      ),
    );
  };
  return {
    mount: (props) => frame({ revision: 0 }, props),
    update: (state, props) => frame({ revision: state.revision + 1 }, props),
    event: (state, _event, props) => frame({ revision: state.revision + 1 }, props),
  };
}
export function activate() {
  return {
    Footer: { kind: 'component', contract: composerFooterContract, component: ['component'] },
  };
}
