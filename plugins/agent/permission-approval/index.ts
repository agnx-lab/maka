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

import type { Component, Emission, Permissions } from '../../../packages/plugin-sdk/src/index.js';
import { permissionApprovalContract, type ServiceApprovalProps } from '../../shared/contracts.js';
import { copy } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { output, type Node, type Presentation } from '../../shared/view.js';

type State = {
  revision: number;
  identity: Pick<ServiceApprovalProps['record'], 'sessionId' | 'requestId' | 'request'>;
  scope: 'once' | 'turn' | 'session';
};
function identity(props: ServiceApprovalProps): State['identity'] {
  const { sessionId, requestId, request } = props.record;
  return { sessionId, requestId, request };
}
export function permissionApproval<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ServiceApprovalProps, N> {
  function frame(state: State, props: ServiceApprovalProps, events: Emission[] = []) {
    if (!sameValue(state.identity, identity(props))) {
      state.identity = identity(props);
      state.scope = 'once';
    }
    const ui = presentation.paint(props.theme),
      text = copy(props.locale),
      request = props.record.request;
    const available =
      props.open && !props.busy && !props.record.outcome && request.kind === 'permissions';
    const choose = (key: string, label: string, selected: boolean) =>
      ui.item(key, ui.text(key + '-label', label), selected, !available);
    const outcome = props.record.outcome;
    const settled = outcome?.kind === 'permissions_decision' ? outcome.decision : null;
    const scope = settled?.decision === 'allow' ? settled.scope : state.scope;
    const scopeLabel =
      scope === 'once'
        ? text.permissionOnce
        : scope === 'turn'
          ? text.permissionTurn
          : text.permissionSession;
    const permissionLines = (prefix: string, permissions: Permissions) => [
      ...permissions.filesystem.map((rule, index) =>
        ui.box(
          prefix + '-rule-' + index,
          [
            ui.text(
              prefix + '-kind-' + index,
              (rule.access === 'read'
                ? text.permissionRead
                : rule.access === 'write'
                  ? text.permissionWrite
                  : text.permissionDeny) +
                ' · ' +
                (rule.scope === 'exact' ? text.permissionExact : text.permissionSubtree),
              props.theme.muted,
            ),
            ui.text(prefix + '-path-' + index, rule.path),
          ],
          { gap: 0 },
        ),
      ),
      ui.text(
        prefix + '-network',
        text.permissionNetwork + ': ' + JSON.stringify(permissions.network),
      ),
    ];
    return output(
      state,
      ui.box(
        'permission-approval',
        [
          ui.box(
            'permission-controls',
            [ui.button('close', text.closeModels), ui.button('check', text.check, props.checking)],
            { row: true, gap: 1, terminal: { wrap: true } },
          ),
          ...(props.status ? [ui.text('permission-status', props.status, props.theme.muted)] : []),
          ...(outcome
            ? [
                ui.text(
                  'permission-outcome',
                  outcome.kind === 'permissions_decision'
                    ? outcome.decision.decision === 'allow'
                      ? text.permissionGranted + ' · ' + scopeLabel
                      : text.permissionDenied
                    : text.questionClosed,
                  props.theme.muted,
                ),
              ]
            : []),
          ui.box(
            'permission-scope',
            [
              choose('once', text.permissionOnce, scope === 'once'),
              choose('turn', text.permissionTurn, scope === 'turn'),
              choose('session', text.permissionSession, scope === 'session'),
            ],
            { row: true, gap: 1, focusGroup: true, terminal: { wrap: true } },
          ),
          ui.box(
            'permission-choice',
            [
              ui.button('allow', text.permissionAllow + ' · ' + scopeLabel, !available),
              ui.button('deny', text.approvalDeny, !available),
            ],
            { row: true, gap: 1, focusGroup: true, terminal: { wrap: true } },
          ),
          ...(request.kind === 'permissions'
            ? [
                presentation.reading(
                  'permission-request',
                  {
                    content: ui.box(
                      'permission-document',
                      [
                        ui.text('permission-reason', request.request.reason),
                        ...(request.request.command
                          ? [
                              ui.text('permission-command', request.request.command.command),
                              ui.text('permission-cwd', request.request.command.cwd),
                            ]
                          : []),
                        ui.text(
                          'permission-requested',
                          text.permissionRequested,
                          props.theme.muted,
                        ),
                        ...permissionLines('permission-requested', request.request.permissions),
                        ...(settled?.decision === 'allow' &&
                        !sameValue(settled.permissions, request.request.permissions)
                          ? [
                              ui.text(
                                'permission-granted',
                                text.permissionGranted,
                                props.theme.muted,
                              ),
                              ...permissionLines('permission-granted', settled.permissions),
                            ]
                          : []),
                      ],
                      { gap: 0, scroll: 'manual', terminal: { grow: 1, basis: 0 } },
                    ),
                  },
                  {},
                ),
              ]
            : []),
        ],
        { visible: props.open, gap: 1, terminal: { grow: 1 } },
      ),
      events,
    );
  }
  return {
    mount(props) {
      return frame({ revision: 0, identity: identity(props), scope: 'once' }, props);
    },
    update(state, props) {
      state.revision++;
      return frame(state, props);
    },
    event(state, input, props) {
      state.revision++;
      const events: Emission[] = [];
      if (!sameValue(state.identity, identity(props))) return frame(state, props);
      if (props.open && input.kind === 'raw') {
        if (input.key === 'close') events.push({ name: 'closed', value: {} });
        else if (input.key === 'check' && !props.checking)
          events.push({ name: 'check', value: {} });
        else if (
          !props.busy &&
          !props.record.outcome &&
          props.record.request.kind === 'permissions'
        ) {
          if (input.key === 'once' || input.key === 'turn' || input.key === 'session')
            state.scope = input.key;
          else if (input.key === 'allow' || input.key === 'deny')
            events.push({
              name: 'submit',
              value: {
                requestId: props.record.requestId,
                decision: input.key,
                scope: state.scope,
              },
            });
        }
      }
      return frame(state, props, events);
    },
  };
}
export function activate() {
  return {
    PermissionApproval: {
      kind: 'component',
      contract: permissionApprovalContract,
      component: ['component'],
    },
  };
}
