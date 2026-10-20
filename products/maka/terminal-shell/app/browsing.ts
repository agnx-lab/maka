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

import { batch, cleanup, signal } from 'nsh';
import { host } from 'maka/host';
import type {
  AgentSessionListScope,
  AgentSession,
  AgentSessionPage,
  AgentProject,
  AgentProjectsPage,
  AgentProjectCursor,
  AgentWorkspaceSelection,
} from '../../../../packages/plugin-sdk/src/index.js';

export type BrowserScope = AgentSessionListScope;
export type BrowserSession = AgentSession;
export type BrowserProject = AgentProject;
export type BrowserWorkspace = AgentWorkspaceSelection;

export type Browsing = {
  scope(): BrowserScope;
  titleQuery(): string;
  sessions(): BrowserSession[];
  projects(): BrowserProject[];
  loading(): boolean;
  error(): string;
  projectError(): string;
  setScope(scope: BrowserScope): void;
  setTitleQuery(text: string): void;
  loadMore(): Promise<void>;
  refresh(): Promise<void>;
};

type SessionPage = AgentSessionPage;
type ProjectCursor = AgentProjectCursor;
type ProjectsPage = AgentProjectsPage;
type Subscription = {
  next(): Promise<SessionPage | null>;
  close(): Promise<void>;
};
type CatalogRead = {
  scope: BrowserScope;
  titleQuery: string;
  stream: Subscription | null;
  ready(): void;
};

const message = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));
const uniqueSessions = (items: BrowserSession[]) => [
  ...new Map(items.map((session) => [session.id, session])).values(),
];

// One mounted read projection serves the workspace browser and session picker.
// Host revisions own paging; these snapshots grant no mutation authority.
export function createBrowsing(): Browsing {
  const scope = signal<BrowserScope>({ kind: 'all' });
  const titleQuery = signal('');
  const sessions = signal<BrowserSession[]>([]);
  const projects = signal<BrowserProject[]>([]);
  const loading = signal(true);
  const error = signal('');
  const projectError = signal('');
  let alive = true;
  let current: CatalogRead | null = null;
  let page: SessionPage | null = null;
  let continuation: SessionPage | null = null;
  let projectRead: object | null = null;
  const owns = (read: CatalogRead) => alive && current === read;
  const close = (read: CatalogRead | null) => {
    if (!read) return;
    read.ready();
    const stream = read.stream;
    read.stream = null;
    if (stream) void stream.close().catch(() => {});
  };
  cleanup(() => {
    alive = false;
    close(current);
    current = null;
    projectRead = null;
  });

  const restart = (clear: boolean): Promise<void> => {
    close(current);
    let ready!: () => void;
    const firstPage = new Promise<void>((resolve) => {
      ready = resolve;
    });
    const read: CatalogRead = { scope: scope(), titleQuery: titleQuery(), stream: null, ready };
    current = read;
    page = null;
    continuation = null;
    batch(() => {
      loading.set(true);
      error.set('');
      if (clear) sessions.set([]);
    });
    if ([...read.titleQuery].length > 128) {
      batch(() => {
        sessions.set([]);
        error.set('Use at most 128 characters in a title search.');
        loading.set(false);
      });
      read.ready();
      return firstPage;
    }
    void (async () => {
      const opened: Subscription = await host.subscribe('agent', 'catalog', {
        filter: 'all',
        scope: read.scope,
        titleQuery: read.titleQuery,
        position: { kind: 'start' },
      });
      if (!owns(read)) {
        await opened.close();
        return;
      }
      read.stream = opened;
      for (;;) {
        const next = await opened.next();
        if (!owns(read)) return;
        if (next === null) throw new Error('Conversation updates ended. Refresh to reconnect.');
        if (page?.revision === next.revision) continue;
        // Each catalog item replaces its window. Earlier continuation pages
        // cannot be attached to this revision, even if some IDs still match.
        page = next;
        continuation = null;
        batch(() => {
          sessions.set(uniqueSessions(next.sessions));
          loading.set(false);
          error.set('');
        });
        read.ready();
      }
    })()
      .catch((reason) => {
        if (!owns(read)) return;
        page = null;
        continuation = null;
        batch(() => {
          error.set(message(reason));
          loading.set(false);
        });
      })
      .finally(() => close(read));
    return firstPage;
  };

  const loadMore = async () => {
    const read = current;
    const original = page;
    if (!read || !owns(read) || !original?.after || loading() || error()) return;
    continuation = original;
    loading.set(true);
    try {
      const next: SessionPage = await host.query('agent', 'list', {
        filter: 'all',
        scope: read.scope,
        titleQuery: read.titleQuery,
        position: { kind: 'after', revision: original.revision, cursor: original.after },
      });
      if (!owns(read) || continuation !== original) return;
      if (next.revision !== original.revision)
        throw new Error('The conversation list changed. Refresh to continue.');
      if (next.after === original.after)
        throw new Error('Conversation paging did not advance. Refresh to continue.');
      page = next;
      sessions.set(uniqueSessions([...sessions(), ...next.sessions]));
    } catch (reason) {
      if (!owns(read) || continuation !== original) return;
      if (message(reason) === 'agent_catalog_changed') {
        await restart(true);
        return;
      }
      error.set(message(reason));
    } finally {
      if (owns(read) && continuation === original) {
        continuation = null;
        loading.set(false);
      }
    }
  };

  const loadProjects = async () => {
    const reading = {};
    projectRead = reading;
    projectError.set('');
    try {
      // Only an explicit revision change starts another read. Transport and
      // availability errors stay visible instead of becoming retry loops.
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const items: BrowserProject[] = [];
        let cursor: ProjectCursor | null = null;
        for (;;) {
          const next: ProjectsPage = await host.query('agent', 'projects', {
            kind: 'list',
            cursor,
          });
          if (!alive || projectRead !== reading) return;
          if (next.kind === 'changed') break;
          if (next.kind !== 'projects') throw new Error('Projects are unavailable.');
          if (cursor && next.revision !== cursor.revision)
            throw new Error('The project list changed between pages. Refresh to try again.');
          if (cursor && next.next?.after === cursor.after)
            throw new Error('Project paging did not advance. Refresh to try again.');
          items.push(...next.projects);
          cursor = next.next;
          if (cursor === null) {
            projects.set(items);
            return;
          }
        }
      }
      throw new Error('Projects changed while reading. Refresh to try again.');
    } catch (reason) {
      if (alive && projectRead === reading) projectError.set(message(reason));
    }
  };

  const refresh = async () => {
    if (!alive) return;
    await Promise.all([restart(false), loadProjects()]);
  };
  const setScope = (next: BrowserScope) => {
    if (!alive) return;
    const previous = scope();
    if (
      previous.kind === next.kind &&
      (previous.kind === 'all' ||
        (previous.kind === 'project' &&
          next.kind === 'project' &&
          previous.projectId === next.projectId) ||
        (previous.kind === 'workspace' &&
          next.kind === 'workspace' &&
          previous.sessionId === next.sessionId))
    )
      return;
    batch(() => {
      scope.set({ ...next });
      void restart(true);
    });
  };
  const setTitleQuery = (text: string) => {
    if (!alive || text === titleQuery()) return;
    batch(() => {
      titleQuery.set(text);
      void restart(true);
    });
  };
  void refresh();
  return {
    scope,
    titleQuery,
    sessions,
    projects,
    loading,
    error,
    projectError,
    setScope,
    setTitleQuery,
    loadMore,
    refresh,
  };
}
