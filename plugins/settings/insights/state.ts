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
  EditorCommand,
  EditorObservation,
  PricingEntry,
  PricingPage,
  PricingUpdate,
  RecoveryRef,
  SettingsUsagePage,
  SettingsUsageRange,
  SettingsUsageSummary,
  UsageActivity,
  UsageSelection,
} from '../../../packages/plugin-sdk/src/index.js';
import { type Bookmark } from '../../shared/bookmark.js';
import { type InsightsProps } from '../../shared/contracts.js';
import { type PageState } from '../../shared/page-shell.js';

export type Tab = 'overview' | 'activity' | 'pricing';

export const fields = ['model', 'input', 'output', 'cacheRead', 'cacheWrite'] as const;

export type FieldName = (typeof fields)[number];

export interface Field {
  observation: EditorObservation | null;
  command: EditorCommand | null;
  reset: string | null;
  sequence: number;
  ready: boolean;
}

export type Request =
  | { kind: 'summary' | 'activity' | 'prices' | 'mutate' | 'receipt' }
  | { kind: 'history'; after: number }
  | { kind: 'search'; captured: EditorObservation }
  | { kind: 'field'; name: FieldName; captured: EditorObservation };

export interface SavedView {
  tab: Tab;
  period: string;
  group: State['group'];
  selection: UsageSelection;
}

export function savedView(value: unknown): SavedView | null {
  if (!value || typeof value !== 'object') return null;
  const v = value as SavedView;
  if (
    !['overview', 'activity', 'pricing'].includes(v.tab) ||
    !['1', '7', '30', 'all'].includes(v.period) ||
    !['byProvider', 'byModel', 'byTool'].includes(v.group) ||
    !v.selection ||
    ![null, 'model', 'tool'].includes(v.selection.kind ?? null) ||
    ![null, 'success', 'error', 'aborted', 'unknown', 'rejected'].includes(
      v.selection.status ?? null,
    ) ||
    typeof v.selection.search !== 'string' ||
    /[\u0000-\u001f\u007f]/.test(v.selection.search)
  )
    return null;
  return { tab: v.tab, period: v.period, group: v.group, selection: { ...v.selection } };
}

export interface State extends PageState {
  bookmark: Bookmark<SavedView | null>;
  savedView: SavedView | null;
  search: Field;
  scope: InsightsProps['scope'];
  revision: number;
  serial: number;
  tab: Tab;
  period: string;
  range: SettingsUsageRange;
  selection: UsageSelection;
  report: SettingsUsageSummary | null;
  activity: SettingsUsagePage | null;
  offsets: number[];
  detail: UsageActivity | null;
  prices: Extract<PricingPage, { kind: 'page' }> | null;
  priceOffsets: number[];
  group: 'byProvider' | 'byModel' | 'byTool';
  groupOffsets: number[];
  editing: { key: number; entry: PricingEntry | null; revision: number } | null;
  fields: Record<FieldName, Field>;
  capture: {
    observations: Record<FieldName, EditorObservation>;
    values: Partial<Record<FieldName, string>>;
  } | null;
  pending: { key: string; request: Request } | null;
  current: { input: PricingUpdate; reference: RecoveryRef | null } | null;
  restoring: boolean;
  confirm: boolean;
  notice: string | null;
}

export const freshField = (reset = ''): Field => ({
  observation: null,
  command: null,
  reset,
  sequence: 0,
  ready: false,
});

export const newFields = () =>
  Object.fromEntries(fields.map((name) => [name, freshField()])) as Record<FieldName, Field>;

export const money = (n: number) => '$' + (n === 0 ? '0' : n.toFixed(n < 1 ? 6 : 2));
