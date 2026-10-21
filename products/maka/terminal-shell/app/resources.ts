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

import { cleanup, effect, signal, untracked } from 'nsh';
import type { YNode } from 'nsh';
import { createWorkspaceFiles, type WorkspaceFileTarget } from './workspaceFileState';
import { createCurrentFile, type CurrentFileModel } from './CurrentFile';
import { createCurrentDiff, type CurrentDiffTarget, type CurrentDiffModel } from './CurrentDiff';
import type { ImagePreview } from './ImageThumbnail';

export type ResourceTab = {
  instance: number;
  identity: string;
  sessionId: string;
  title: string;
} & (
  | { kind: 'file'; model: CurrentFileModel }
  | { kind: 'diff'; model: CurrentDiffModel }
  | { kind: 'image'; image: ImagePreview }
);

// Window presentation state only. All resource authority remains in the
// original Host selector; no session, draft or backend data is duplicated here.
export function createResources(options: { selected(): string | null; blocked(): boolean }) {
  const shown = signal(false);
  const tabs = signal<ResourceTab[]>([]);
  const active = signal<number | 'files' | 'changes'>('files');
  const focusRequest = signal(false);
  const instance = signal(0);
  let nextInstance = 0;
  let session: string | null = null;
  let alive = true;
  let root: YNode | undefined;
  let header: { sessionId: string; node: YNode } | undefined;
  let opener: (() => boolean) | undefined;
  const dispose = (tab: ResourceTab) => {
    if (tab.kind !== 'image') tab.model.dispose();
  };
  const adopt = () => {
    const selected = options.selected();
    if (selected !== session) {
      tabs().forEach(dispose);
      tabs.set([]);
      shown.set(false);
      active.set('files');
      focusRequest.set(false);
      instance.set(instance() + 1);
      session = selected;
      opener = undefined;
    }
  };
  effect(() => {
    options.selected();
    untracked(adopt);
  });
  const admits = (sessionId: string) =>
    alive && !options.blocked() && options.selected() === sessionId;
  const reveal = () => {
    if (!shown()) {
      instance.set(instance() + 1);
      focusRequest.set(true);
    }
    shown.set(true);
  };
  const select = (target: { sessionId: string; instance: number }) => {
    if (
      !admits(target.sessionId) ||
      !tabs().some((tab) => tab.instance === target.instance && tab.sessionId === target.sessionId)
    )
      return;
    active.set(target.instance);
    reveal();
  };
  const open = (sessionId: string, identity: string, build: (instance: number) => ResourceTab) => {
    if (!admits(sessionId)) return;
    adopt();
    let tab = tabs().find((candidate) => candidate.identity === identity);
    if (!tab) {
      tab = build(++nextInstance);
      tabs.set([...tabs(), tab]);
    }
    active.set(tab.instance);
    reveal();
    focusRequest.set(true);
  };
  const openFile = (target: WorkspaceFileTarget) =>
    open(target.sessionId, `file:${target.path.bytesBase64}`, (instance) => ({
      kind: 'file',
      instance,
      identity: `file:${target.path.bytesBase64}`,
      sessionId: target.sessionId,
      title: target.title,
      model: createCurrentFile(target),
    }));
  const openDiff = (target: CurrentDiffTarget) =>
    open(target.sessionId, `diff:${target.area}:${target.path.bytesBase64}`, (instance) => ({
      kind: 'diff',
      instance,
      identity: `diff:${target.area}:${target.path.bytesBase64}`,
      sessionId: target.sessionId,
      title: `${target.title} · ${target.area}`,
      model: createCurrentDiff(target),
    }));
  const headerOpener = (sessionId: string) => {
    const original = header;
    return () => {
      if (
        !alive ||
        options.selected() !== sessionId ||
        !original ||
        original.sessionId !== sessionId ||
        header !== original
      )
        return false;
      original.node.focus();
      return true;
    };
  };
  const openImage = (image: ImagePreview) => {
    const { sessionId } = image.source.read;
    const identity = image.uri;
    if (!admits(sessionId)) return;
    open(sessionId, identity, (instance) => ({
      kind: 'image',
      instance,
      identity,
      sessionId,
      title: 'Original image',
      image,
    }));
    const fallback = headerOpener(sessionId);
    opener = () => image.restoreFocus?.() || fallback();
  };
  const inventory = (target: { sessionId: string; kind: 'files' | 'changes' }) => {
    if (!admits(target.sessionId)) return;
    adopt();
    active.set(target.kind);
    reveal();
  };
  const closeTab = (target: { sessionId: string; instance: number }) => {
    if (!admits(target.sessionId)) return;
    const tab = tabs().find((tab) => tab.instance === target.instance);
    if (!tab) return;
    dispose(tab);
    tabs.set(tabs().filter((candidate) => candidate !== tab));
    if (active() === tab.instance) active.set('files');
    focusRequest.set(true);
  };
  const hide = () => {
    shown.set(false);
    focusRequest.set(false);
  };
  const close = (target: { sessionId: string; instance: number }) => {
    if (!admits(target.sessionId) || target.instance !== instance()) return;
    hide();
    opener?.();
  };
  const openFiles = (target: { sessionId: string }) => {
    if (admits(target.sessionId)) {
      inventory({ ...target, kind: 'files' });
      opener = headerOpener(target.sessionId);
      focusRequest.set(true);
    }
  };
  const files = createWorkspaceFiles({
    sessionId: options.selected,
    active: () => shown() && active() === 'files',
    openFile,
  });
  effect(() => {
    const visible = shown();
    const selected = active();
    const current = tabs();
    untracked(() =>
      current.forEach((tab) => {
        if (tab.kind === 'file') {
          if (visible && selected === tab.instance) tab.model.activate();
          else tab.model.suspend();
        }
      }),
    );
  });
  cleanup(() => {
    alive = false;
    tabs().forEach(dispose);
  });
  return {
    shown,
    tabs,
    active,
    files,
    inventory,
    openFiles,
    openImage,
    openDiff,
    select,
    closeTab,
    hide,
    close,
    instance,
    sessionId: options.selected,
    activeTab: () => tabs().find((tab) => tab.instance === active()),
    attach(node: YNode) {
      root = node;
    },
    root: () => root,
    focusRequest,
    focusHandled: () => focusRequest.set(false),
    attachOpener(sessionId: string, node: YNode) {
      if (alive && options.selected() === sessionId) header = { sessionId, node };
    },
  };
}
export type Resources = ReturnType<typeof createResources>;
