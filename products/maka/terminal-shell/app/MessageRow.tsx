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

import { cleanup, computed, inputAction, signal, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import { host } from 'maka/host';
import { messageBytes, messageKey, type Position, type Row } from './history';
import { MarkdownBody } from './MarkdownBody';
import { C } from './ui';
import { ToolImage, type ImagePreview } from './ToolImage';

export type MessageViewState = {
  expanded: ReturnType<typeof signal<boolean>>;
  sourceMode: ReturnType<typeof signal<boolean>>;
};
type MessageRowToggle = {
  sessionId: string;
  key: string;
  value: boolean;
};
type MessageSourceRef = {
  key: string;
  field: 'arguments' | 'result';
  toolUseId: string;
  position: Position;
  name?: string;
};
// Each instance owns one exact immutable field at its opening fence. It has
// one in-flight page; closing/rebinding disposes its owner and ignores replies.
function MessageSource({
  reference,
  sessionId,
  through,
}: {
  reference: MessageSourceRef;
  sessionId: string;
  through: () => number;
}) {
  const original = {
    ...reference,
    position: { ...reference.position },
    sessionId,
    through: through(),
  };
  const content = signal(''),
    total = signal<number | null>(null),
    format = signal('');
  const busy = signal(false),
    error = signal('');
  let alive = true;
  cleanup(() => {
    alive = false;
  });
  const complete = () => total() !== null && messageBytes(content()) === total();
  const read = async () => {
    if (!alive || busy() || complete()) return;
    const offset = messageBytes(content());
    busy.set(true);
    error.set('');
    try {
      const page = await host.query('agent', 'source', {
        sessionId: original.sessionId,
        through: original.through,
        source: { kind: 'row', position: original.position },
        field: original.field,
        position: offset === 0 ? { kind: 'start' } : { kind: 'after', offset },
      });
      if (!alive) return;
      const source = page.source,
        tool = source?.tool;
      const samePosition = (a: any, b: any) =>
        !!a && !!b && a.sourceSequence === b.sourceSequence && a.itemIndex === b.itemIndex;
      const owned = original.field === 'arguments' ? tool?.call : tool?.result;
      const end = offset + messageBytes(source?.text ?? '');
      if (
        page.kind !== 'ready' ||
        page.through !== original.through ||
        source?.field !== original.field ||
        source.path != null ||
        !samePosition(source.position, original.position) ||
        source.role !== (original.field === 'arguments' ? 'tool_call' : 'tool_result') ||
        tool?.toolUseId !== original.toolUseId ||
        !samePosition(owned?.position, original.position) ||
        (original.field === 'arguments' && tool.call.name !== original.name) ||
        source.offset !== offset ||
        messageBytes(content()) !== offset ||
        !Number.isSafeInteger(source.totalBytes) ||
        source.totalBytes < 0 ||
        end > source.totalBytes ||
        (total() !== null && source.totalBytes !== total()) ||
        (end === offset && end !== source.totalBytes) ||
        (format() !== '' && source.format !== format()) ||
        !(original.field === 'arguments'
          ? source.format === 'json'
          : ['text', 'json', 'image_descriptor'].includes(source.format))
      ) {
        throw new Error('The original tool source could not be verified.');
      }
      total.set(source.totalBytes);
      format.set(source.format);
      content.set(content() + source.text);
    } catch (reason) {
      if (alive) error.set(String(reason));
    } finally {
      if (alive) {
        busy.set(false);
        if (!complete() && !error()) void read();
      }
    }
  };
  void Promise.resolve().then(read);
  return (
    <View style={{ gap: 16 }}>
      <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
        {original.field === 'arguments' ? 'Arguments' : 'Result'}
        {format() === 'image_descriptor' ? ' · image descriptor' : ''}
      </Text>
      <Text selectable style={{ color: C.text, fontSize: 16, lineHeight: 16 }}>
        {content()}
      </Text>
      <If when={error()}>
        <Text style={{ color: C.danger, fontSize: 16, lineHeight: 16 }}>{error()}</Text>
      </If>
      <If when={!complete()}>
        <Button
          disabled={!error()}
          onPress={inputAction(read, {
            sessionId: original.sessionId,
            key: original.key,
            through: original.through,
          })}
          style={{ height: 16 }}
        >
          <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
            {error() ? 'Retry original source' : 'Reading…'} · {messageBytes(content())}/
            {total() ?? '?'} bytes
          </Text>
        </Button>
      </If>
    </View>
  );
}

export function MessageRow({
  item,
  sessionId,
  through,
  viewState,
  previewImage,
  onExpand,
  ownerKey,
  reasoningPreview,
}: {
  item: () => Row;
  sessionId: string;
  through: () => number;
  viewState?: MessageViewState;
  previewImage(image: ImagePreview): void;
  onExpand?(key: string): void;
  ownerKey?: () => string;
  reasoningPreview?: () => boolean;
}) {
  const rowKey = (row: Row) => ownerKey?.() ?? messageKey(row);
  return (
    <Each of={[item()]} key={rowKey}>
      {(row) => {
        const expanded = viewState?.expanded ?? signal(false),
          sourceMode = viewState?.sourceMode ?? signal(false);
        const ownsRow = (action: MessageRowToggle) => {
          return action.sessionId === sessionId && action.key === rowKey(row());
        };
        const setSourceMode = (action: MessageRowToggle) => {
          if (ownsRow(action)) sourceMode.set(action.value);
        };
        const setExpanded = (action: MessageRowToggle) => {
          if (!ownsRow(action)) return;
          expanded.set(action.value);
          if (action.value) onExpand?.(action.key);
        };
        const text = computed(() => row().text);
        const preview = computed(() => {
          const first = /\S[^\r\n]*/.exec(text())?.[0].trim() ?? '';
          return first.length > 160 ? `${first.slice(0, 160)}…` : first;
        });
        const thinking = () => row().field === 'thinking' || row().role === 'thinking';
        const isTool = () =>
          row().tool != null || row().role === 'tool_call' || row().role === 'tool_result';
        const compact = () => thinking() || isTool();
        const user = () => row().role === 'user';
        const summary = () => {
          if (thinking())
            return `◇ Think${reasoningPreview?.() !== false && preview() ? ` · ${preview()}` : ''}`;
          const call = row().tool?.call;
          return `${expanded() ? 'v' : '>'} ${call?.name ?? 'Tool'}${call?.title?.fallback ? ` · ${call.title.fallback}` : ''}${row().tool?.result?.isError ? ' · error' : ''}`;
        };
        const timestamp = () => {
          const value = (row() as Row & { timestamp?: number }).timestamp;
          if (typeof value !== 'number') return '';
          const date = new Date(value);
          return Number.isNaN(date.getTime())
            ? ''
            : `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
        };
        const sourceRefs = (): MessageSourceRef[] => {
          const tool = row().tool;
          if (!tool?.toolUseId) return [];
          return (['arguments', 'result'] as const).flatMap((field) => {
            const position = (field === 'arguments' ? tool.call : tool.result)?.position;
            return position
              ? [
                  {
                    field,
                    position,
                    toolUseId: tool.toolUseId,
                    name: tool.call?.name,
                    key: `${field}:${tool.toolUseId}:${position.sourceSequence}/${position.itemIndex}`,
                  },
                ]
              : [];
          });
        };
        return (
          <View style={{ paddingVertical: user() ? 16 : 0, gap: 0, width: '100%' }}>
            <If
              when={user()}
              else={
                <If
                  when={compact()}
                  else={
                    <View style={{ gap: 0 }}>
                      <If
                        when={sourceMode()}
                        else={
                          <If
                            when={row().role === 'assistant' && row().field === 'body'}
                            else={
                              <Text
                                selectable
                                style={{ color: C.text, fontSize: 16, lineHeight: 16 }}
                              >
                                {text()}
                              </Text>
                            }
                          >
                            <MarkdownBody row={row} />
                          </If>
                        }
                      >
                        <Text selectable style={{ color: C.text, fontSize: 16, lineHeight: 16 }}>
                          {text()}
                        </Text>
                      </If>
                      <Button
                        selected={sourceMode()}
                        onPress={inputAction(setSourceMode, {
                          sessionId,
                          key: rowKey(row()),
                          value: !sourceMode(),
                        })}
                        style={{ height: 16, alignSelf: 'flex-end' }}
                      >
                        <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
                          {sourceMode() ? '[Formatted]' : '[Source]'}
                        </Text>
                      </Button>
                    </View>
                  }
                >
                  <Button
                    selected={expanded()}
                    onPress={inputAction(setExpanded, {
                      sessionId,
                      key: rowKey(row()),
                      value: !expanded(),
                    })}
                    style={{ height: 16, paddingVertical: 0 }}
                  >
                    <Text
                      numberOfLines={1}
                      style={{
                        height: 16,
                        color: row().tool?.result?.isError ? C.danger : C.muted,
                        fontSize: 16,
                        lineHeight: 16,
                      }}
                    >
                      {summary()}
                    </Text>
                  </Button>
                  <If when={expanded()}>
                    <View style={{ paddingLeft: 16, paddingVertical: 16, gap: 16 }}>
                      <If when={row().tool?.result?.image}>
                        <ToolImage
                          sessionId={sessionId}
                          through={through}
                          position={row().tool.result.position}
                          preview={previewImage}
                        />
                      </If>
                      <If
                        when={thinking()}
                        else={
                          <If
                            when={sourceRefs().length > 0}
                            else={
                              <Text
                                selectable
                                style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}
                              >
                                {text() ||
                                  'Original tool source is unavailable in this history row.'}
                              </Text>
                            }
                          >
                            <Each
                              of={sourceRefs()}
                              key={(reference: MessageSourceRef) => reference.key}
                            >
                              {(reference) => (
                                <MessageSource
                                  reference={reference()}
                                  sessionId={sessionId}
                                  through={through}
                                />
                              )}
                            </Each>
                          </If>
                        }
                      >
                        <Text selectable style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
                          {text()}
                        </Text>
                      </If>
                    </View>
                  </If>
                </If>
              }
            >
              <View
                style={{
                  alignSelf: 'flex-end',
                  maxWidth: '78%',
                  paddingHorizontal: 16,
                  paddingVertical: 16,
                  borderRadius: 16,
                  backgroundColor: C.surface,
                }}
              >
                <Text selectable style={{ color: C.text, fontSize: 16, lineHeight: 16 }}>
                  {text()}
                </Text>
              </View>
              <If when={timestamp()}>
                <Text
                  style={{ alignSelf: 'flex-end', color: C.muted, fontSize: 16, lineHeight: 16 }}
                >
                  {timestamp()}
                </Text>
              </If>
            </If>
          </View>
        );
      }}
    </Each>
  );
}
