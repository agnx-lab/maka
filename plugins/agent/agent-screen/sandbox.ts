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

import type { Action, AgentWorkspaceAccess } from '../../../packages/plugin-sdk/src/index.js';
import type { Theme } from '../../shared/contracts.js';
import { copy } from './locale.js';
import { type Node, type Presentation } from '../../shared/view.js';
import { intent, mutationPending, conversationRoute } from './actions.js';
import type { State } from './state.js';

export const sandboxModes = ['read_only', 'workspace_write', 'danger_full_access'] as const;
export function sandboxLabel(access: AgentWorkspaceAccess, locale: string) {
  const text = copy(locale);
  return access === 'read_only'
    ? text.permissionReadOnly
    : access === 'workspace_write'
      ? text.sandboxWorkspace
      : text.permissionFullAccess;
}
export function sandboxAction(state: State, actions: Action[], key: string) {
  if (!key.startsWith('sandbox-')) return false;
  if (key === 'sandbox-close') {
    if (state.dialog === 'sandbox') state.dialog = null;
    return true;
  }
  if (key === 'sandbox-check' && state.uncertain?.request.kind === 'sandboxChange') {
    intent(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.uncertain.reference);
    return true;
  }
  const session = state.selected;
  if (!session || session.archived || mutationPending(state) || !conversationRoute(state))
    return true;
  if (key === 'sandbox-open' && state.dialog === null) state.dialog = 'sandbox';
  const access = sandboxModes.find((mode) => key === `sandbox-${mode}`);
  if (state.dialog === 'sandbox' && access && access !== session.permissions.access) {
    intent(
      state,
      actions,
      { kind: 'sandboxChange', epoch: state.epoch, sessionId: session.id },
      'agent',
      'metadata',
      {
        sessionId: session.id,
        expectedRevision: session.revision,
        title: session.title,
        labels: [...session.labels],
        permissions: { settings: { ...session.permissions, access }, revokeGrants: true },
      },
    );
  }
  return true;
}
export function sandboxView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
): N {
  const ui = presentation.paint(theme),
    text = copy(locale),
    busy = mutationPending(state);
  return ui.box(
    'sandbox-options',
    [
      ...sandboxModes.map((access) =>
        ui.button(
          `sandbox-${access}`,
          `${state.selected?.permissions.access === access ? (theme.ascii ? '* ' : '✓ ') : ''}${sandboxLabel(access, locale)}`,
          busy || state.selected?.permissions.access === access,
        ),
      ),
      ui.text('sandbox-help', text.sandboxHelp, theme.muted),
      ...(state.notice ? [ui.text('sandbox-notice', text[state.notice], theme.warning)] : []),
      ...(state.uncertain?.request.kind === 'sandboxChange'
        ? [
            ui.button(
              'sandbox-check',
              text.check,
              Object.values(state.pending).some((r) => r.kind === 'receipt'),
            ),
          ]
        : []),
      ui.button('sandbox-close', text.cancel),
    ],
    { gap: 1 },
  );
}
