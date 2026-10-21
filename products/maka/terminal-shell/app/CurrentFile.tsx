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
import { Image } from 'nsh/image';
import { List } from 'nsh/list';
import { host, type FileView } from './host';
import type { AgentFileChange } from '../../../../packages/plugin-sdk/src/index.js';
import type { WorkspaceFileTarget } from './workspaceFileState';
import { C } from './ui';

type FileState =
  | FileView
  | { kind: 'idle' }
  | { kind: 'loading'; previous: FileView | null }
  | { kind: 'error'; message: string; previous: FileView | null };
export type CurrentFileModel = {
  target: WorkspaceFileTarget;
  state(): FileState;
  position: { x: number; y: number };
  stale(): boolean;
  activate(): void;
  suspend(): void;
  refresh(): void;
  dispose(): void;
};
type FileSubscription = { readFile(): Promise<FileView>; close(): Promise<void> };
type WatchSubscription = { next(): Promise<AgentFileChange | null>; close(): Promise<void> };
type Watching = { stream: WatchSubscription | null; ready: boolean; changes: number };
type Reading = { stream: FileSubscription | null };

const captured = (state: FileState): FileView | null =>
  state.kind === 'idle'
    ? null
    : state.kind === 'loading' || state.kind === 'error'
      ? state.previous
      : state;
const message = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));
const close = async (resource: { stream: { close(): Promise<void> } | null } | null) => {
  const stream = resource?.stream;
  if (resource) resource.stream = null;
  if (stream) await stream.close();
};

export function createCurrentFile(target: WorkspaceFileTarget): CurrentFileModel {
  const state = signal<FileState>({ kind: 'idle' });
  const stale = signal(false);
  let alive = true;
  let active = false;
  let watching: Watching | null = null;
  let reading: Reading | null = null;
  const ownsWatch = (watch: Watching) => alive && active && watching === watch;
  const ownsRead = (read: Reading, watch: Watching) => ownsWatch(watch) && reading === read;
  const stopRead = () => {
    const previous = reading;
    reading = null;
    return close(previous);
  };
  const stop = () => {
    const previous = watching;
    watching = null;
    void close(previous).catch(() => {});
    void stopRead().catch(() => {});
  };
  const failWatch = (watch: Watching, reason: unknown) => {
    if (!ownsWatch(watch)) return;
    stop();
    stale.set(true);
    state.set({ kind: 'error', message: message(reason), previous: captured(state()) });
  };
  const read = (watch: Watching) => {
    if (!ownsWatch(watch) || !watch.ready) return;
    const previous = reading;
    const request: Reading = { stream: null };
    const changes = watch.changes;
    reading = request;
    state.set({ kind: 'loading', previous: captured(state()) });
    void (async () => {
      await close(previous);
      if (!ownsRead(request, watch)) return;
      const opened: FileSubscription = await host.subscribe('agent', 'readFile', {
        sessionId: target.sessionId,
        path: target.path,
      });
      if (!ownsRead(request, watch)) {
        await opened.close();
        return;
      }
      request.stream = opened;
      try {
        // Native drains Begin/Chunk/End through successful completion and cleanup,
        // verifies bytes/digest, and prepares bounded text rows or an image asset.
        const result = await opened.readFile();
        await close(request);
        if (!ownsRead(request, watch)) return;
        const previous = captured(state());
        if (
          !previous ||
          !('version' in previous) ||
          !('version' in result) ||
          previous.version !== result.version
        )
          model.position = { x: 0, y: 0 };
        stale.set(watch.changes !== changes || result.kind === 'changed');
        state.set(result);
      } finally {
        await close(request);
      }
    })()
      .catch((reason) => {
        if (ownsRead(request, watch))
          state.set({ kind: 'error', message: message(reason), previous: captured(state()) });
      })
      .finally(() => {
        if (reading === request) reading = null;
      });
  };
  const startWatch = () => {
    if (!alive || !active) return;
    stop();
    const watch: Watching = { stream: null, ready: false, changes: 0 };
    watching = watch;
    state.set({ kind: 'loading', previous: captured(state()) });
    void (async () => {
      const opened: WatchSubscription = await host.subscribe('agent', 'watchFiles', {
        sessionId: target.sessionId,
        path: target.parent,
      });
      if (!ownsWatch(watch)) {
        await opened.close();
        return;
      }
      watch.stream = opened;
      for (;;) {
        const change = await opened.next();
        if (!ownsWatch(watch)) return;
        if (change === null) throw new Error('File updates ended. Retry to reconnect.');
        if (change.kind === 'unavailable')
          throw new Error('The parent directory changed or is unavailable. Retry to reopen it.');
        if (change.kind === 'ready') {
          watch.ready = true;
          read(watch);
        } else {
          watch.changes += 1;
          stale.set(true);
        }
      }
    })().catch((reason) => failWatch(watch, reason));
  };
  const suspend = () => {
    active = false;
    stop();
    const previous = captured(state());
    if (state().kind === 'loading') state.set(previous ?? { kind: 'idle' });
    if (previous) stale.set(true);
  };
  const model: CurrentFileModel = {
    target,
    state,
    stale,
    position: { x: 0, y: 0 },
    activate() {
      if (!alive || active) return;
      active = true;
      startWatch();
    },
    suspend,
    refresh() {
      if (!alive || !active) return;
      if (watching?.ready) read(watching);
      else startWatch();
    },
    dispose() {
      alive = false;
      suspend();
    },
  };
  return model;
}

export function CurrentFile(props: { model: CurrentFileModel }) {
  const model = props.model;
  const view = () => captured(model.state());
  const version = () => {
    const value = view();
    return value && 'version' in value ? value.version : '';
  };
  const error = () => {
    const value = model.state();
    return value.kind === 'error' ? value.message : '';
  };
  const notice = () => {
    const value = view();
    switch (value?.kind) {
      case 'text':
        return value.bytes === 0 ? 'Empty file.' : '';
      case 'absent':
        return 'This file no longer exists.';
      case 'changed':
        return 'This file changed while it was being read. Refresh to capture it again.';
      case 'binary':
        return `Binary file · ${value.bytes} bytes. A text preview is unavailable.`;
      case 'too_large':
        return `This file exceeds the preview limit of ${value.limit} bytes (${value.bytes} bytes).`;
      case 'unsupported':
        return `This ${value.fileKind === 'symlink' ? 'symbolic link' : value.fileKind} cannot be previewed.`;
      default:
        return '';
    }
  };
  return (
    <View style={{ flex: 1, minHeight: 0, gap: 8 }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
        <Text numberOfLines={1} style={{ flex: 1, color: C.text, fontWeight: '700' }}>
          {model.target.title}
        </Text>
        <Button
          disabled={model.state().kind === 'loading'}
          accessibilityLabel="Refresh current file"
          onPress={inputAction(model.refresh, null)}
          hoverStyle={{ backgroundColor: C.raised }}
          focusStyle={{ backgroundColor: C.line }}
        >
          <Text style={{ color: C.accent }}>
            {model.state().kind === 'error' ? 'Retry' : 'Refresh'}
          </Text>
        </Button>
      </View>
      <Text style={{ color: C.muted }}>Current file · Captured contents</Text>
      <If when={version()}>
        <Text selectable numberOfLines={1} style={{ color: C.muted }}>
          Captured version {version()}
        </Text>
      </If>
      <If when={model.stale()}>
        <Text style={{ color: C.warning }}>
          The workspace may have changed. Refresh for current contents.
        </Text>
      </If>
      <If when={model.state().kind === 'idle' || model.state().kind === 'loading'}>
        <Text style={{ color: C.muted }}>
          {model.state().kind === 'loading' ? 'Reading current file…' : 'File preview is inactive.'}
        </Text>
      </If>
      <If when={error()}>
        <Text selectable style={{ color: C.danger }}>
          {error()}
        </Text>
      </If>
      <If when={notice()}>
        <Text style={{ color: C.muted }}>{notice()}</Text>
      </If>
      <Each of={view() ? [view()!] : []} key={(value) => value}>
        {(value) => {
          const snapshot = value();
          if (snapshot.kind === 'text') return <FileRows model={model} view={snapshot} />;
          if (snapshot.kind === 'image') return <FileImage model={model} view={snapshot} />;
          return <View />;
        }}
      </Each>
    </View>
  );
}

function FileRows(props: { model: CurrentFileModel; view: Extract<FileView, { kind: 'text' }> }) {
  const initialOffset = { ...props.model.position };
  let mounted = true;
  cleanup(() => {
    mounted = false;
  });
  return (
    <List
      each={props.view.rows}
      keyExtractor={(row) => String(row.offset)}
      estimatedItemSize={16}
      contentOffset={initialOffset}
      onScroll={(event) => {
        if (mounted) props.model.position = { x: event.x, y: event.y };
      }}
      accessibilityLabel="Captured file text"
      style={{ flex: 1, minHeight: 0 }}
    >
      {(row) => (
        <View style={{ flexDirection: 'row', alignItems: 'flex-start', minHeight: 16, gap: 8 }}>
          <Text
            style={{
              width: Math.max(24, String(props.view.rows.at(-1)?.line ?? 1).length * 8),
              color: C.muted,
              fontFamily: 'monospace',
              lineHeight: 16,
            }}
          >
            {row().line}
          </Text>
          <Text
            selectable
            style={{
              flex: 1,
              color: C.text,
              fontFamily: 'monospace',
              fontSize: 16,
              lineHeight: 16,
            }}
          >
            {/* Each native block starts at this source line; internal newlines remain intact. */}
            {row().text.endsWith('\n') ? row().text.slice(0, -1) : row().text}
          </Text>
        </View>
      )}
    </List>
  );
}

function FileImage(props: { model: CurrentFileModel; view: Extract<FileView, { kind: 'image' }> }) {
  const error = signal('');
  const dimensions = signal<{ width: number; height: number } | null>(null);
  let mounted = true;
  cleanup(() => {
    mounted = false;
  });
  return (
    <View style={{ flex: 1, minHeight: 0, gap: 8 }}>
      <Image
        src={props.view.uri}
        resizeMode="contain"
        onLoad={(value) => {
          if (mounted) dimensions.set(value);
        }}
        onError={(reason) => {
          if (mounted) error.set(reason.message);
        }}
        accessibilityLabel="Captured workspace image"
        style={{ flex: 1, minHeight: 0, width: '100%' }}
      />
      <If when={dimensions()}>
        <Text style={{ color: C.muted }}>
          {dimensions()!.width} × {dimensions()!.height} · Captured image
        </Text>
      </If>
      <If when={error()}>
        <Text selectable style={{ color: C.danger }}>
          {error()}
        </Text>
        <Button
          disabled={props.model.state().kind === 'loading'}
          onPress={inputAction(props.model.refresh, null)}
        >
          <Text style={{ color: C.accent }}>Retry image</Text>
        </Button>
      </If>
    </View>
  );
}
