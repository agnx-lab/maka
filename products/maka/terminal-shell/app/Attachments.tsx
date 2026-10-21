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

import { inputAction, Scroll, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import { AttachmentPicker } from 'maka/host';
import type { AgentHistoryAttachment } from '../../../../packages/plugin-sdk/src/index.js';
import type { AttachmentDraft } from './attachmentDraft';
import type { Position } from './history';
import { ImageThumbnail, type ImagePreview } from './ImageThumbnail';
import { C, controlStyle } from './ui';
import { Icon } from './Icon';

const image = (mime: string) =>
  ['image/png', 'image/jpeg', 'image/gif', 'image/webp', 'image/avif'].includes(mime);
const size = (bytes: number) =>
  bytes < 1024
    ? `${bytes} B`
    : bytes < 1024 * 1024
      ? `${Math.ceil(bytes / 1024)} KB`
      : `${(bytes / (1024 * 1024)).toFixed(1)} MB`;

export function DraftAttachments(props: {
  draft: AttachmentDraft;
  preview(image: ImagePreview): void;
}) {
  const owner = props.draft;
  return (
    <View style={{ gap: 8 }}>
      <If when={owner.picker()}>
        <View style={{ flexDirection: 'row', justifyContent: 'flex-end' }}>
          <Button
            accessibilityLabel="Close attachment picker"
            onPress={inputAction(owner.open, { owner: owner.sessionId, value: false })}
            style={{ ...controlStyle, flexDirection: 'row', alignItems: 'center', gap: 8 }}
            hoverStyle={{ backgroundColor: C.line }}
            focusStyle={{ backgroundColor: C.line }}
          >
            <Icon name="close" color={C.secondary} />
            <Text style={{ color: C.secondary }}>Close picker</Text>
          </Button>
        </View>
      </If>
      {/* Keep this native owner mounted. Hiding only closes its private picker;
        the original selected path and in-flight resource retain their owner. */}
      <AttachmentPicker
        client={owner.client}
        visible={owner.picker()}
        disabled={owner.restoring()}
        onStatus={owner.status}
      />
      <If when={owner.files().length > 0 || owner.rows().length > 0}>
        <Scroll style={{ maxHeight: 176, flexShrink: 1 }}>
          <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
            <Each of={owner.files()} key={(file) => file.id}>
              {(file) => {
                const original = file();
                return (
                  <View style={{ backgroundColor: C.surface, borderRadius: 8, padding: 8, gap: 8 }}>
                    <If when={image(original.mimeType)}>
                      <ImageThumbnail
                        small
                        source={{
                          kind: 'attachment-image',
                          read: {
                            sessionId: owner.sessionId,
                            source: { kind: 'uploaded', id: original.id },
                            offset: 0,
                          },
                        }}
                        label={`Open attachment ${original.name}`}
                        preview={props.preview}
                      />
                    </If>
                    <Text numberOfLines={1} style={{ color: C.text }}>
                      {original.name}
                    </Text>
                    <View style={{ flexDirection: 'row', gap: 16 }}>
                      <Text style={{ color: C.muted }}>{size(original.bytes)} · Ready</Text>
                      <Button
                        accessibilityLabel={`Remove attachment ${original.name}`}
                        onPress={inputAction(owner.remove, {
                          owner: owner.sessionId,
                          id: original.id,
                        })}
                        style={controlStyle}
                        hoverStyle={{ backgroundColor: C.line }}
                        focusStyle={{ backgroundColor: C.line }}
                      >
                        <Text style={{ color: C.muted }}>Remove</Text>
                      </Button>
                    </View>
                  </View>
                );
              }}
            </Each>
            <Each of={owner.rows()} key={(row) => row.id}>
              {(row) => (
                <View style={{ backgroundColor: C.surface, borderRadius: 8, padding: 8, gap: 8 }}>
                  <Text style={{ color: C.text }}>{row().name}</Text>
                  <Text style={{ color: row().kind === 'failed' ? C.danger : C.muted }}>
                    {row().kind === 'reading'
                      ? 'Reading selected file…'
                      : row().kind === 'uploading'
                        ? 'Confirming upload…'
                        : row().kind === 'unknown'
                          ? 'Original upload needs confirmation'
                          : 'Upload failed'}
                  </Text>
                  <If when={row().error}>
                    <Text selectable style={{ color: C.danger }}>
                      {row().error}
                    </Text>
                  </If>
                  <View style={{ flexDirection: 'row', gap: 16 }}>
                    <If when={row().kind === 'unknown'}>
                      <Button
                        disabled={owner.recovering() !== null}
                        accessibilityLabel={
                          row().original ? 'Recover original upload' : 'Check original upload'
                        }
                        onPress={inputAction(row().original ? owner.recover : owner.inspect, {
                          owner: owner.sessionId,
                          id: row().id,
                        })}
                        style={controlStyle}
                        hoverStyle={{ backgroundColor: C.line }}
                        focusStyle={{ backgroundColor: C.line }}
                      >
                        <Text style={{ color: C.accent }}>
                          {row().original ? 'Recover original upload' : 'Check original upload'}
                        </Text>
                      </Button>
                      <If when={row().original && !row().original.reference}>
                        <Button
                          disabled={owner.recovering() !== null}
                          onPress={inputAction(owner.abandon, {
                            owner: owner.sessionId,
                            id: row().id,
                          })}
                          style={controlStyle}
                          hoverStyle={{ backgroundColor: C.line }}
                          focusStyle={{ backgroundColor: C.line }}
                        >
                          <Text style={{ color: C.warning }}>Abandon unconfirmed upload</Text>
                        </Button>
                      </If>
                    </If>
                    <If when={row().kind === 'failed'}>
                      <Button
                        accessibilityLabel={`Remove failed attachment ${row().name}`}
                        onPress={inputAction(owner.remove, {
                          owner: owner.sessionId,
                          id: row().id,
                        })}
                        style={controlStyle}
                        hoverStyle={{ backgroundColor: C.line }}
                        focusStyle={{ backgroundColor: C.line }}
                      >
                        <Text style={{ color: C.muted }}>Remove</Text>
                      </Button>
                    </If>
                  </View>
                </View>
              )}
            </Each>
          </View>
        </Scroll>
      </If>
      <If when={owner.error()}>
        <Text selectable style={{ color: C.danger }}>
          {owner.error()}
        </Text>
        <Button
          onPress={inputAction(owner.retry, owner.sessionId)}
          style={controlStyle}
          hoverStyle={{ backgroundColor: C.line }}
          focusStyle={{ backgroundColor: C.line }}
        >
          <Text style={{ color: C.accent }}>Retry attachment updates</Text>
        </Button>
      </If>
    </View>
  );
}

export function HistoryAttachments(props: {
  sessionId: string;
  through: number;
  position?: Position;
  attachments: AgentHistoryAttachment[];
  preview(image: ImagePreview): void;
}) {
  return (
    <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
      <Each
        of={props.attachments.map((attachment, index) => ({ ...attachment, index }))}
        key={(attachment) => attachment.index}
      >
        {(attachment) => {
          const original = attachment();
          return (
            <View style={{ backgroundColor: C.raised, borderRadius: 8, padding: 8, gap: 8 }}>
              <If when={image(original.mimeType) && props.position !== undefined}>
                <ImageThumbnail
                  small
                  source={{
                    kind: 'attachment-image',
                    read: {
                      sessionId: props.sessionId,
                      offset: 0,
                      source: {
                        kind: 'history',
                        through: props.through,
                        position: { ...props.position! },
                        attachmentIndex: original.index,
                      },
                    },
                  }}
                  label={`Open attachment ${original.name}`}
                  preview={props.preview}
                />
              </If>
              <Text style={{ color: C.text }}>{original.name}</Text>
              <Text style={{ color: C.muted }}>
                {size(original.bytes)} · {original.kind}
              </Text>
            </View>
          );
        }}
      </Each>
    </View>
  );
}
