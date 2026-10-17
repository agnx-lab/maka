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

import type { Theme } from '../../shared/contracts.js';
import { copy } from './locale.js';
import { type Node, type Presentation } from '../../shared/view.js';
import type { State } from './state.js';
import { canGenerateRecap } from './actions.js';

export function recapView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
): N {
  const ui = presentation.paint(theme),
    text = copy(locale),
    recap = state.recap;
  const generating = Object.values(state.pending).some(
    (request) => request.kind === 'recapGenerate' && request.epoch === state.epoch,
  );
  const open = state.inspector === 'recap' && state.dialog === null;
  return ui.box(
    'recap-panel',
    [
      ui.box(
        'recap-header',
        [
          ui.text('recap-title', text.recapTitle, theme.text, 550),
          ui.button('recap-close', text.closeModels),
        ],
        { row: true, gap: 1 },
      ),
      ...(recap.failed ? [ui.text('recap-error', text.recapFailed)] : []),
      ...(recap.loading !== null ? [ui.text('recap-loading', text.recapLoading)] : []),
      ...(generating || recap.receipt?.kind === 'pending'
        ? [ui.text('recap-pending', text.recapPending)]
        : []),
      ...(recap.receipt?.kind === 'ready'
        ? [
            ui.text('recap-saved', `${text.recapSaved} · ${recap.receipt.modelId}`, theme.muted),
            presentation.reading(
              'recap-reading',
              {
                command: null,
                content: ui.box('recap-body', [ui.text('recap-text', recap.receipt.text)], {
                  gap: 1,
                  scroll: 'manual',
                  terminal: { grow: 1 },
                }),
              },
              {},
            ),
          ]
        : recap.receipt?.kind === 'failed'
          ? [ui.text('recap-generation-failed', text.recapFailed)]
          : recap.loaded && !generating && recap.receipt === null
            ? [ui.text('recap-empty', text.recapEmpty)]
            : []),
      ui.button('recap-refresh', text.recapRefresh, recap.loading !== null),
      ...(state.uncertain?.reference.method === 'generateRecap'
        ? [
            ui.button(
              'recap-check',
              text.check,
              Object.values(state.pending).some((request) => request.kind === 'receipt'),
            ),
          ]
        : []),
      ...(!state.model || state.executor
        ? [ui.text('recap-model', text.recapModel, theme.muted)]
        : []),
      ...(recap.confirmation
        ? [
            ui.text('recap-fee', `${text.recapFee} ${recap.confirmation.modelId}`),
            ui.box(
              'recap-confirmation',
              [
                ui.button('recap-confirm', text.recapConfirm, !canGenerateRecap(state)),
                ui.button('recap-cancel', text.cancel),
              ],
              { row: true, gap: 1 },
            ),
          ]
        : [ui.button('recap-generate', text.recapGenerate, !canGenerateRecap(state))]),
    ],
    { visible: open, gap: 1, terminal: { grow: 1 } },
  );
}
