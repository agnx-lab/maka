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

import type { AgentSession } from '../../../../packages/plugin-sdk/src/index.js';
import type { Control } from './Conversation';

export type SessionAction =
  | { kind: 'rename' | 'fork'; session: AgentSession; title: string }
  | { kind: 'archive' | 'restore' | 'remove'; session: AgentSession };
export type SessionActionResult =
  { kind: 'updated' | 'forked'; session: AgentSession } | { kind: 'removed'; sessionId: string };

export class SessionRevisionConflict extends Error {
  constructor() {
    super('This conversation changed. Review its current details, then try the action again.');
  }
}

// The captured Session is the user's original revision and labels. A conflict
// requires another explicit action; never silently replace its CAS identity.
export async function performSessionAction(
  control: Control,
  action: SessionAction,
): Promise<SessionActionResult> {
  const session = action.session;
  const identity = { sessionId: session.id, expectedRevision: session.revision };
  const result =
    action.kind === 'rename'
      ? await control('metadata', { ...identity, title: action.title, labels: [...session.labels] })
      : action.kind === 'fork'
        ? await control('fork', {
            ...identity,
            turnId: null,
            title: action.title,
            labels: [...session.labels],
          })
        : await control('changeSession', { ...identity, action: action.kind });
  if (result.kind === 'revision_conflict') throw new SessionRevisionConflict();
  if (result.kind === 'rejected') {
    const reason =
      result.reason === 'busy'
        ? 'This conversation is working, has queued messages, or still needs your input. Finish it before changing its lifecycle.'
        : `Conversation action rejected: ${result.reason}`;
    throw new Error(reason);
  }
  if (result.kind === 'removed') return { kind: 'removed', sessionId: result.sessionId };
  if (result.kind === 'forked') return { kind: 'forked', session: result.session };
  if (result.kind === 'changed' || result.kind === 'committed')
    return { kind: 'updated', session: result.session };
  throw new Error('Host returned an unexpected conversation action result.');
}
