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
import { Clipboard } from 'nsh/clipboard';
import { host } from './host';
import { messageBytes, type Position, type Row } from './history';
import { C } from './ui';

type OriginalMessage = {
  sessionId: string;
  key: string;
  position: Position;
  through: number;
};
const CLIPBOARD_BYTES = 64 * 1024;
const samePosition = (left: Position, right: Position) =>
  left.sourceSequence === right.sourceSequence && left.itemIndex === right.itemIndex;

/** Copy one canonical body, including text outside the loaded history window. */
export function MessageCopy(props: {
  row(): Row;
  ownerKey(): string;
  sessionId: string;
  through(): number;
}) {
  const busy = signal(false);
  const notice = signal('');
  const failed = signal(false);
  let alive = true;
  cleanup(() => {
    alive = false;
  });
  const target = (): OriginalMessage | null => {
    const row = props.row();
    if (!row.finalOutput || row.role !== 'assistant' || row.field !== 'body' || !row.position)
      return null;
    return {
      sessionId: props.sessionId,
      key: props.ownerKey(),
      position: { ...row.position },
      through: row.metadataThrough ?? props.through(),
    };
  };
  const owns = (original: OriginalMessage) => {
    const current = target();
    return (
      alive &&
      current !== null &&
      current.sessionId === original.sessionId &&
      current.key === original.key &&
      samePosition(current.position, original.position)
    );
  };
  const copy = async (original: OriginalMessage | null) => {
    if (original === null || !owns(original) || busy()) return;
    busy.set(true);
    notice.set('');
    failed.set(false);
    try {
      let content = '';
      let offset = 0;
      let total: number | null = null;
      for (;;) {
        const page = await host.query('agent', 'source', {
          sessionId: original.sessionId,
          through: original.through,
          source: { kind: 'row', position: original.position },
          field: 'body',
          position: offset === 0 ? { kind: 'start' } : { kind: 'after', offset },
        });
        if (!owns(original)) return;
        const source = page.source;
        const end = offset + messageBytes(source.text);
        if (
          page.kind !== 'ready' ||
          page.through !== original.through ||
          !samePosition(source.position, original.position) ||
          source.field !== 'body' ||
          source.role !== 'assistant' ||
          source.format !== 'text' ||
          source.path != null ||
          source.offset !== offset ||
          !Number.isSafeInteger(source.totalBytes) ||
          source.totalBytes < 0 ||
          (total !== null && source.totalBytes !== total) ||
          end > source.totalBytes ||
          (end === offset && end !== source.totalBytes)
        ) {
          throw new Error('The original message source could not be verified.');
        }
        if (source.totalBytes > CLIPBOARD_BYTES)
          throw new Error('Message exceeds the terminal clipboard limit (64 KiB).');
        total = source.totalBytes;
        content += source.text;
        offset = end;
        if (offset === total) break;
      }
      if (!owns(original)) return;
      Clipboard.setString(content);
      notice.set('Sent to terminal clipboard');
    } catch (reason) {
      if (owns(original)) {
        failed.set(true);
        notice.set(reason instanceof Error ? reason.message : String(reason));
      }
    } finally {
      if (owns(original)) busy.set(false);
    }
  };
  return (
    <View style={{ flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: 8 }}>
      <If when={notice()}>
        <Text style={{ color: failed() ? C.danger : C.muted, fontSize: 16, lineHeight: 16 }}>
          {notice()}
        </Text>
      </If>
      <Button
        accessibilityLabel="Copy message"
        disabled={busy()}
        onPress={inputAction(copy, target())}
        style={{ height: 16 }}
        hoverStyle={{ backgroundColor: C.surface }}
        focusStyle={{ backgroundColor: C.raised }}
      >
        <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
          {busy() ? 'Reading…' : '[Copy]'}
        </Text>
      </Button>
    </View>
  );
}
