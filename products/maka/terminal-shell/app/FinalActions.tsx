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
import type { YNode } from 'nsh';
import { Button } from 'nsh/base';
import { host } from './host';
import type { Position, Row } from './history';
import { MessageCopy } from './MessageCopy';
import { forkTitle, type SessionAction } from './sessionLifecycle';
import { TurnUsage } from './TurnUsage';
import { C } from './ui';

type Target = {
  sessionId: string;
  key: string;
  position: Position;
  turnId: string;
  through: number;
};

/** Actions retain this exact completed reply, even if a later turn is running. */
export function FinalActions(props: {
  row(): Row;
  ownerKey(): string;
  sessionId: string;
  through(): number;
  fork(action: Extract<SessionAction, { kind: 'fork' }>): Promise<void>;
}) {
  const pending = signal(false);
  const notice = signal('');
  const usage = signal<Target | null>(null);
  let usageButton: YNode | undefined;
  let alive = true;
  cleanup(() => {
    alive = false;
  });
  const target = (): Target | null => {
    const row = props.row();
    return row.finalOutput && row.position && row.turnId
      ? {
          sessionId: props.sessionId,
          key: props.ownerKey(),
          position: { ...row.position },
          turnId: row.turnId,
          through: row.metadataThrough ?? props.through(),
        }
      : null;
  };
  const owns = (original: Target) => {
    const current = target();
    return (
      alive &&
      current !== null &&
      current.sessionId === original.sessionId &&
      current.key === original.key &&
      current.turnId === original.turnId &&
      current.position.sourceSequence === original.position.sourceSequence &&
      current.position.itemIndex === original.position.itemIndex
    );
  };
  const fork = async (original: Target | null) => {
    if (!original || !owns(original) || pending()) return;
    pending.set(true);
    notice.set('');
    try {
      const observed = await host.query('agent', 'observe', {
        sessionId: original.sessionId,
      });
      if (!owns(original)) return;
      if (observed.session.id !== original.sessionId)
        throw new Error('The Host observed a different conversation.');
      await props.fork({
        kind: 'fork',
        session: observed.session,
        title: forkTitle(observed.session.title),
        turnId: original.turnId,
      });
      if (owns(original)) notice.set('Fork created. Open it in Sessions.');
    } catch (reason) {
      if (owns(original)) notice.set(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (alive) pending.set(false);
    }
  };
  const showUsage = (original: Target | null) => {
    if (original && owns(original)) usage.set(original);
  };
  const closeUsage = () => {
    if (!alive) return;
    usage.set(null);
    usageButton?.focus();
  };
  const details = () => {
    const row = props.row();
    const model = row.model;
    const name =
      model?.kind === 'native' ? (model.reportedModelId ?? model.requestedModelId) : model?.modelId;
    const timing = row.timing;
    const elapsed =
      timing?.finished && timing.finished.at >= timing.startedAt
        ? timing.finished.at - timing.startedAt
        : null;
    const duration =
      elapsed === null
        ? ''
        : elapsed < 60000
          ? `${(elapsed / 1000).toFixed(1)}s`
          : `${Math.floor(elapsed / 60000)}m ${Math.floor(elapsed / 1000) % 60}s`;
    return [name, duration].filter(Boolean).join(' · ');
  };
  return (
    <View style={{ gap: 8, maxWidth: '100%' }}>
      <View
        style={{
          flexDirection: 'row',
          flexWrap: 'wrap',
          alignItems: 'center',
          gap: 8,
        }}
      >
        <MessageCopy
          row={props.row}
          ownerKey={props.ownerKey}
          sessionId={props.sessionId}
          through={props.through}
        />
        <Button
          accessibilityLabel="Fork from this turn"
          disabled={pending() || target() === null}
          onPress={inputAction(fork, target())}
          hoverStyle={{ backgroundColor: C.surface }}
          focusStyle={{ backgroundColor: C.raised }}
        >
          <Text style={{ color: C.muted }}>{pending() ? 'Forking…' : 'Fork from here'}</Text>
        </Button>
        <Button
          ref={(node) => {
            usageButton = node;
          }}
          accessibilityLabel="View this turn's model usage"
          disabled={target() === null}
          onPress={inputAction(showUsage, target())}
          hoverStyle={{ backgroundColor: C.surface }}
          focusStyle={{ backgroundColor: C.raised }}
        >
          <Text style={{ color: C.muted }}>Usage</Text>
        </Button>
        <If when={details()}>
          <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>{details()}</Text>
        </If>
      </View>
      <If when={notice()}>
        <Text selectable style={{ color: C.warning }}>
          {notice()}
        </Text>
      </If>
      <If when={usage()}>
        <TurnUsage
          sessionId={usage()!.sessionId}
          turnId={usage()!.turnId}
          through={usage()!.through}
          current={() => usage() !== null && owns(usage()!)}
          close={closeUsage}
        />
      </If>
    </View>
  );
}
