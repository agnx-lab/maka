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

export function checklistView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
): N {
  const ui = presentation.paint(theme),
    text = copy(locale),
    page = state.checklist.page;
  const open = state.inspector === 'checklist' && state.dialog === null;
  return ui.box(
    'checklist-panel',
    [
      ui.box(
        'checklist-header',
        [
          ui.text('checklist-title', text.checklistTitle, theme.text, 550),
          ui.button('checklist-close', text.closeModels),
        ],
        { row: true, gap: 1 },
      ),
      ui.text('checklist-reported', text.checklistReported, theme.muted),
      ...(state.checklist.failed
        ? [
            ui.text('checklist-error', text.checklistFailed),
            ui.button('checklist-retry', text.checklistRetry),
          ]
        : !page
          ? [ui.text('checklist-loading', text.checklistLoading)]
          : [
              ui.text('checklist-count', `${page.completed}/${page.total}`, theme.muted),
              presentation.reading(
                'checklist-reading',
                {
                  command: null,
                  content: ui.box(
                    'checklist-items',
                    page.items.length
                      ? page.items.map((item, index) =>
                          ui.text(
                            `checklist-item-${page.offset + index}`,
                            `${item.status === 'completed' ? '[x]' : item.status === 'in_progress' ? '[>]' : '[ ]'} ${item.content}`,
                            item.status === 'completed' ? theme.muted : theme.text,
                            item.status === 'in_progress' ? 550 : 400,
                          ),
                        )
                      : [ui.text('checklist-empty', text.checklistEmpty, theme.muted)],
                    { gap: 1, scroll: 'manual', terminal: { grow: 1 } },
                  ),
                },
                {},
              ),
              ...(page.total > 20
                ? [
                    ui.box(
                      'checklist-pages',
                      [
                        ui.button('checklist-previous', text.previousModels, page.offset === 0),
                        ui.text(
                          'checklist-range',
                          `${page.offset + 1}–${page.offset + page.items.length}/${page.total}`,
                        ),
                        ui.button('checklist-next', text.nextModelPage, page.nextOffset === null),
                      ],
                      { row: true, gap: 1 },
                    ),
                  ]
                : []),
            ]),
    ],
    { visible: open, gap: 1, terminal: { grow: 1 } },
  );
}
