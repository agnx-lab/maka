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

import { cleanup, inputAction, signal, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import { List } from 'nsh/list';
import { host } from './host';
import type { GitArea, GitDiff, WorkspacePath } from '../../../../packages/plugin-sdk/src/index.js';
import { parsePatch } from '../../desktop-shell/app/patch.js';
import { Icon } from './Icon';
import {
  C,
  bodyTextStyle,
  captionTextStyle,
  codeTextStyle,
  controlStyle,
  controlHoverStyle,
  controlFocusStyle,
} from './ui';

export const gitAreaLabel: Record<GitArea, string> = {
  staged: 'Staged',
  worktree: 'Worktree',
  untracked: 'Untracked',
};

export type CurrentDiffTarget = {
  sessionId: string;
  area: GitArea;
  path: WorkspacePath;
  title: string;
};
type PatchRow = ReturnType<typeof parsePatch>[number];
type DiffRow = {
  key: string;
  kind: PatchRow['kind'];
  text: string;
  old: number | null;
  new: number | null;
  continuation: boolean;
};
const MAX_PATCH_CODE_UNITS = 2 * 1024 * 1024;
const MAX_DIFF_ROWS = 8192;

// Display budgets only: Host still returns the whole patch. String.length is
// UTF-16 code units, not bytes; reject before the shared parser allocates rows.
function diffRows(patch: string): DiffRow[] | null {
  if (patch.length > MAX_PATCH_CODE_UNITS) return null;
  let sourceRows = patch.endsWith('\n') ? 0 : 1;
  for (let index = 0; index < patch.length; index += 1) {
    if (patch.charCodeAt(index) === 10 && ++sourceRows > MAX_DIFF_ROWS) return null;
  }
  const rows: DiffRow[] = [];
  for (const [index, row] of parsePatch(patch).entries()) {
    let text = '';
    let length = 0;
    let segment = 0;
    const append = () => {
      if (rows.length === MAX_DIFF_ROWS) return false;
      rows.push({
        key: `${index}:${segment}`,
        kind: row.kind,
        text,
        old: segment === 0 && 'old' in row ? (row.old ?? null) : null,
        new: segment === 0 && 'new' in row ? (row.new ?? null) : null,
        continuation: segment > 0,
      });
      segment += 1;
      text = '';
      length = 0;
      return true;
    };
    for (const point of row.text) {
      if (length === 256 && !append()) return null;
      text += point;
      length += 1;
    }
    if (!append()) return null;
  }
  return rows;
}

type DiffState =
  | { kind: 'loading' }
  | { kind: 'unavailable' }
  | { kind: 'too_large' }
  | { kind: 'error'; message: string }
  | { kind: 'text'; rows: DiffRow[] }
  | Exclude<GitDiff, { kind: 'text' }>;
export type CurrentDiffModel = {
  target: CurrentDiffTarget;
  state(): DiffState;
  position: { x: number; y: number };
  refresh(): Promise<void>;
  dispose(): void;
};

// The resource tab owns this projection across dock/Dialog presentation changes.
// The existing gitDiff query returns a whole patch without a size bound.
export function createCurrentDiff(target: CurrentDiffTarget): CurrentDiffModel {
  const state = signal<DiffState>({ kind: 'loading' });
  let generation = 0;
  let alive = true;
  let previous: GitDiff | null | undefined;
  const refresh = async () => {
    if (!alive) return;
    const reading = ++generation;
    state.set({ kind: 'loading' });
    try {
      const result: GitDiff | null = await host.query('agent', 'gitDiff', {
        sessionId: target.sessionId,
        area: target.area,
        path: target.path,
      });
      if (!alive || reading !== generation) return;
      const same =
        previous !== undefined &&
        (result === null
          ? previous === null
          : previous !== null &&
            previous.kind === result.kind &&
            (result.kind !== 'text' ||
              (previous.kind === 'text' && previous.patch === result.patch)));
      if (!same) model.position = { x: 0, y: 0 };
      const rows = result?.kind === 'text' ? diffRows(result.patch) : null;
      // Do not keep a rejected whole patch resident in an inactive resource tab.
      previous = result?.kind === 'text' && rows === null ? undefined : result;
      state.set(
        result === null
          ? { kind: 'unavailable' }
          : result.kind === 'text'
            ? rows === null
              ? { kind: 'too_large' }
              : { kind: 'text', rows }
            : result,
      );
    } catch (reason) {
      if (alive && reading === generation) state.set({ kind: 'error', message: String(reason) });
    }
  };
  const model: CurrentDiffModel = {
    target,
    state,
    position: { x: 0, y: 0 },
    refresh,
    dispose() {
      alive = false;
      generation += 1;
    },
  };
  void refresh();
  return model;
}

export function CurrentDiff(props: { model: CurrentDiffModel }) {
  const model = props.model;
  const notice = () => {
    const state = model.state();
    switch (state.kind) {
      case 'loading':
        return 'Reading current diff…';
      case 'unavailable':
        return 'Not a Git workspace.';
      case 'too_large':
        return 'This diff is too large to display in the terminal.';
      case 'binary':
        return 'Binary file. A text diff is unavailable.';
      case 'unchanged':
        return 'No current changes in this area.';
      case 'unsupported':
        return 'This change does not support a text diff.';
      case 'error':
        return state.message;
      case 'text':
        return '';
    }
  };
  return (
    <View style={{ flex: 1, minHeight: 0, gap: 8 }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
        <Icon name="code" color={C.muted} />
        <Text
          numberOfLines={1}
          style={{ ...captionTextStyle, flex: 1, color: C.text, fontWeight: '500' }}
        >
          {model.target.title}
        </Text>
        <Button
          accessibilityLabel="Refresh current diff"
          onPress={inputAction(model.refresh, null)}
          tooltip={model.state().kind === 'error' ? 'Retry current diff' : 'Refresh current diff'}
          style={{ ...controlStyle, width: 32, paddingHorizontal: 8, alignItems: 'center' }}
          hoverStyle={controlHoverStyle}
          focusStyle={controlFocusStyle}
        >
          <Icon name="refresh" color={C.secondary} />
        </Button>
      </View>
      <Text style={{ ...captionTextStyle, color: C.muted }}>
        Current {gitAreaLabel[model.target.area].toLowerCase()} diff
      </Text>
      <If when={notice()}>
        <Text
          selectable
          style={{ ...bodyTextStyle, color: model.state().kind === 'error' ? C.danger : C.muted }}
        >
          {notice()}
        </Text>
      </If>
      <If when={model.state().kind === 'text'}>
        <DiffRows model={model} />
      </If>
    </View>
  );
}

function DiffRows(props: { model: CurrentDiffModel }) {
  const model = props.model;
  const initialOffset = { ...model.position };
  let mounted = true;
  cleanup(() => {
    mounted = false;
  });
  const rows = () => {
    const state = model.state();
    return state.kind === 'text' ? state.rows : [];
  };
  return (
    <List
      accessibilityLabel="Current file diff"
      each={rows()}
      keyExtractor={(row: DiffRow) => row.key}
      estimatedItemSize={codeTextStyle.lineHeight}
      contentOffset={initialOffset}
      onScroll={(event) => {
        if (mounted) model.position = { x: event.offset.x, y: event.offset.y };
      }}
      style={{ flex: 1, minHeight: 0, backgroundColor: C.surface }}
      empty={
        <Text style={{ ...bodyTextStyle, color: C.muted }}>
          The current patch has no displayable rows.
        </Text>
      }
    >
      {(row) => (
        <Text
          selectable
          style={{
            ...codeTextStyle,
            color:
              row().kind === 'added'
                ? '#9bd5a5'
                : row().kind === 'removed'
                  ? C.danger
                  : row().kind === 'context'
                    ? C.text
                    : C.muted,
            backgroundColor:
              row().kind === 'added' ? '#203527' : row().kind === 'removed' ? '#382528' : undefined,
          }}
        >
          {row().continuation
            ? '↳'
            : row().kind === 'added'
              ? '+'
              : row().kind === 'removed'
                ? '-'
                : row().kind === 'context'
                  ? ' '
                  : ''}
          {row().text}
        </Text>
      )}
    </List>
  );
}
