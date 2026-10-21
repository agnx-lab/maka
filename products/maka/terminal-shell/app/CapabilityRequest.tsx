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

import { Text, View } from 'nsh';
import type { InteractionRequest } from '../../../../packages/plugin-sdk/src/generated/runtime';
import { C } from './ui';

type Request = Extract<InteractionRequest, { kind: 'client_capability' }>;
// The Agent's response gate accepts this exact producer-owned Computer target.
// It never turns a generic capability request into a broad client grant.
export function computerCapability(request: Request): boolean {
  const target = request.target;
  return (
    Number.isSafeInteger(request.baseRevision) &&
    request.baseRevision! >= 0 &&
    target.providerId === 'maka_workspace_computer' &&
    target.contractId === 'maka_workspace_computer_v1' &&
    target.serverId === 'maka_workspace_computer' &&
    target.toolName === 'cua_repl' &&
    target.capability === 'computer_use' &&
    target.scope.kind === 'capability'
  );
}
export function CapabilityRequest(props: { request: Request; sessionId: string }) {
  return (
    <View style={{ gap: 16 }}>
      <Text style={{ color: C.text, fontSize: 16, lineHeight: 16, fontWeight: '600' }}>
        {computerCapability(props.request)
          ? 'Computer access for this session'
          : 'Requested client capability'}
      </Text>
      <If when={computerCapability(props.request)}>
        <Text selectable style={{ color: C.text, fontSize: 16, lineHeight: 16 }}>
          This allows the requesting tool to inspect and control the local computer, including
          screenshots, keyboard and pointer actions. The grant lasts for this session's current
          permission boundary.
        </Text>
      </If>
      <Text selectable style={{ color: C.text, fontSize: 16, lineHeight: 16 }}>
        {`Session: ${props.sessionId}\nProvider: ${props.request.target.providerId}\nContract: ${props.request.target.contractId}\nServer: ${props.request.target.serverId}\nTool: ${props.request.target.toolName}\nCapability: ${props.request.target.capability}\nScope: ${JSON.stringify(props.request.target.scope)}\nPermission boundary: ${props.request.baseRevision ?? 'unavailable'}`}
      </Text>
      <If when={!computerCapability(props.request)}>
        <Text style={{ color: C.warning, fontSize: 16, lineHeight: 16 }}>
          This Host cannot answer the requested capability. Closing leaves the original request
          pending.
        </Text>
      </If>
    </View>
  );
}
