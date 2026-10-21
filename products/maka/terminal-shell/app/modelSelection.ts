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

export type ModelBackend = {
  connectionId: string;
  modelId: string;
  thinkingLevel: string | null;
};
export type BackendIdentity = ModelBackend | null;
export type ModelChoice = {
  selection: { connectionId: string; modelId: string; revision: number };
  connectionName: string;
  label: string;
  thinkingLevels: string[];
  defaultThinkingLevel: string | null;
};
export type ModelDetails =
  | { kind: 'loading' }
  | { kind: 'unavailable'; message: string }
  | { kind: 'ready'; choice: ModelChoice };

type ModelRoute = Pick<ModelBackend, 'connectionId' | 'modelId'>;
export const sameRoute = (left: ModelRoute | null, right: ModelRoute | null): boolean =>
  left === null || right === null
    ? left === right
    : left.connectionId === right.connectionId && left.modelId === right.modelId;

export function modelCaption(backend: BackendIdentity, details: ModelDetails): string {
  if (backend === null) return 'Choose a model';
  if (details.kind === 'ready' && sameRoute(backend, details.choice.selection))
    return details.choice.label;
  return details.kind === 'unavailable' ? `${backend.modelId} · Unavailable` : backend.modelId;
}

export function effortCaption(backend: BackendIdentity, details: ModelDetails): string {
  const choice =
    details.kind === 'ready' && sameRoute(backend, details.choice.selection)
      ? details.choice
      : null;
  if (backend?.thinkingLevel == null)
    return choice?.defaultThinkingLevel != null
      ? `Default · ${choice.defaultThinkingLevel}`
      : 'Default';
  return choice && !choice.thinkingLevels.includes(backend.thinkingLevel)
    ? `${backend.thinkingLevel} · Unavailable`
    : backend.thinkingLevel;
}
