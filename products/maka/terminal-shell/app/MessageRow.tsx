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

import { computed, inputAction, signal, Text, View } from 'nsh';
import type { JSX } from 'nsh';
import { Button } from 'nsh/base';
import { messageKey, type Row } from './history';
import { MarkdownBody } from './MarkdownBody';
import { C } from './ui';
import { HistoryAttachments } from './Attachments';
import { ToolCard } from './ToolCard';
import { MessageCopy } from './MessageCopy';
import type { ImagePreview } from './ImageThumbnail';

export type MessageViewState = {
  expanded: ReturnType<typeof signal<boolean>>;
  sourceMode: ReturnType<typeof signal<boolean>>;
};
type MessageRowToggle = {
  sessionId: string;
  key: string;
  value: boolean;
};

export function MessageRow({
  item,
  sessionId,
  through,
  viewState,
  previewImage,
  onExpand,
  ownerKey,
  reasoningPreview,
  finalActions,
}: {
  item: () => Row;
  sessionId: string;
  through: () => number;
  viewState?: MessageViewState;
  previewImage(image: ImagePreview): void;
  onExpand?(key: string): void;
  ownerKey?: () => string;
  reasoningPreview?: () => boolean;
  finalActions?(row: () => Row, ownerKey: () => string): JSX.Element;
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
        const summary = () =>
          `◇ Think${reasoningPreview?.() !== false && preview() ? ` · ${preview()}` : ''}`;
        const timestamp = () => {
          const value = (row() as Row & { timestamp?: number }).timestamp;
          if (typeof value !== 'number') return '';
          const date = new Date(value);
          return Number.isNaN(date.getTime())
            ? ''
            : `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
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
                      <View
                        style={{
                          flexDirection: 'row',
                          flexWrap: 'wrap',
                          alignSelf: 'flex-end',
                          alignItems: 'center',
                          gap: 16,
                        }}
                      >
                        <If
                          when={
                            row().finalOutput &&
                            row().role === 'assistant' &&
                            row().field === 'body' &&
                            row().position
                          }
                        >
                          <If
                            when={finalActions}
                            else={
                              <MessageCopy
                                row={row}
                                ownerKey={() => rowKey(row())}
                                sessionId={sessionId}
                                through={through}
                              />
                            }
                          >
                            {finalActions?.(row, () => rowKey(row()))}
                          </If>
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
                    </View>
                  }
                >
                  <If
                    when={isTool()}
                    else={
                      <View>
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
                            style={{ height: 16, color: C.muted, fontSize: 16, lineHeight: 16 }}
                          >
                            {summary()}
                          </Text>
                        </Button>
                        <If when={expanded()}>
                          <View style={{ paddingLeft: 16, paddingVertical: 16 }}>
                            <Text
                              selectable
                              style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}
                            >
                              {text()}
                            </Text>
                          </View>
                        </If>
                      </View>
                    }
                  >
                    <ToolCard
                      row={row}
                      sessionId={sessionId}
                      through={through}
                      ownerKey={() => rowKey(row())}
                      expanded={expanded}
                      original={sourceMode}
                      previewImage={previewImage}
                      expand={setExpanded}
                      showOriginal={setSourceMode}
                    />
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
                <If when={(row().attachments?.length ?? 0) > 0}>
                  <HistoryAttachments
                    sessionId={sessionId}
                    through={row().metadataThrough ?? through()}
                    position={row().position}
                    attachments={row().attachments!}
                    preview={previewImage}
                  />
                </If>
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
