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

import { cleanup, computed, signal } from 'nsh';
import { Storage } from 'nsh/storage';

export const TRANSCRIPT_VIEW_MODES = ['compact', 'standard', 'detailed', 'verbose'] as const;
export type TranscriptViewMode = (typeof TRANSCRIPT_VIEW_MODES)[number];
export type CollapseTiming = 'completion' | 'next-input';
export type PresentationPolicy = {
  readonly mode: TranscriptViewMode;
  readonly collapseTiming: CollapseTiming;
  readonly foldCompletedTurns: boolean;
  readonly stepGrouping: 'collapsed' | 'history' | 'none';
  readonly liveProcessDetail: boolean;
  readonly settledReasoningPreview: boolean;
};
type PresentationChoice = Pick<PresentationPolicy, 'mode' | 'collapseTiming'>;
export type PresentationSettings = {
  mode(): TranscriptViewMode;
  collapseTiming(): CollapseTiming;
  policy(): PresentationPolicy;
  notice(): string;
  setMode(mode: TranscriptViewMode): void;
  setCollapseTiming(timing: CollapseTiming): void;
};

const CAPABILITIES: Record<
  TranscriptViewMode,
  Omit<PresentationPolicy, 'mode' | 'collapseTiming'>
> = {
  compact: {
    foldCompletedTurns: true,
    stepGrouping: 'collapsed',
    liveProcessDetail: false,
    settledReasoningPreview: false,
  },
  standard: {
    foldCompletedTurns: true,
    stepGrouping: 'collapsed',
    liveProcessDetail: true,
    settledReasoningPreview: true,
  },
  detailed: {
    foldCompletedTurns: true,
    stepGrouping: 'history',
    liveProcessDetail: true,
    settledReasoningPreview: true,
  },
  verbose: {
    foldCompletedTurns: false,
    stepGrouping: 'none',
    liveProcessDetail: false,
    settledReasoningPreview: true,
  },
};

export function presentationPolicyFor(
  mode: TranscriptViewMode,
  collapseTiming: CollapseTiming = 'completion',
): PresentationPolicy {
  return { mode, collapseTiming, ...CAPABILITIES[mode] };
}

const DEFAULT_CHOICE: PresentationChoice = { mode: 'standard', collapseTiming: 'completion' };
const STORAGE_KEY = 'terminal.presentation';

function readChoice(text: string | null): PresentationChoice {
  if (text === null) return DEFAULT_CHOICE;
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return DEFAULT_CHOICE;
  }
  if (typeof value !== 'object' || value === null) return DEFAULT_CHOICE;
  const savedMode = 'mode' in value ? value.mode : undefined;
  const savedTiming = 'collapseTiming' in value ? value.collapseTiming : undefined;
  return {
    mode: TRANSCRIPT_VIEW_MODES.find((mode) => mode === savedMode) ?? DEFAULT_CHOICE.mode,
    collapseTiming:
      savedTiming === 'completion' || savedTiming === 'next-input'
        ? savedTiming
        : DEFAULT_CHOICE.collapseTiming,
  };
}

// One mounted owner shares the choice across conversations. Storage calls are
// already ordered by NSH; no second persistence queue or Host setting is needed.
export function createPresentationSettings(): PresentationSettings {
  const choice = signal<PresentationChoice>(DEFAULT_CHOICE);
  const notice = signal('');
  const policy = computed(() => presentationPolicyFor(choice().mode, choice().collapseTiming));
  let alive = true;
  let revision = 0;
  cleanup(() => {
    alive = false;
  });
  void Storage.getItem(STORAGE_KEY).then(
    (text) => {
      if (alive && revision === 0) choice.set(readChoice(text));
    },
    () => {
      if (alive && revision === 0) notice.set('View settings could not be loaded.');
    },
  );
  const select = (next: PresentationChoice) => {
    if (
      !alive ||
      (revision > 0 &&
        choice().mode === next.mode &&
        choice().collapseTiming === next.collapseTiming)
    )
      return;
    // Even choosing the displayed default is an explicit choice while the
    // initial read is pending; it must win over an older saved preference.
    const ownRevision = ++revision;
    choice.set(next);
    notice.set('');
    void Storage.setItem(STORAGE_KEY, JSON.stringify(next)).catch(() => {
      if (alive && revision === ownRevision)
        notice.set('View settings could not be saved. This choice applies until you exit.');
    });
  };
  return {
    mode: () => choice().mode,
    collapseTiming: () => choice().collapseTiming,
    policy,
    notice,
    setMode: (mode) => select({ ...choice(), mode }),
    setCollapseTiming: (collapseTiming) => select({ ...choice(), collapseTiming }),
  };
}
