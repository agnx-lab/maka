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
import { windowSize } from 'nsh/device';
import { Image } from 'nsh/image';
import type { YNode } from 'nsh';
import { C } from './ui';
import { immutableAsset } from './immutableAsset';
import type {
  AgentToolImageRead,
  AgentAttachmentImageRead,
} from '../../../../packages/plugin-sdk/src/index.js';

export type PreviewSource =
  | { kind: 'tool-image'; read: AgentToolImageRead }
  | { kind: 'attachment-image'; read: AgentAttachmentImageRead };
export type ImagePreview = {
  source: PreviewSource;
  uri: string;
  width: number;
  height: number;
  restoreFocus?(): boolean;
};

export function ImageThumbnail(props: {
  source: PreviewSource;
  small?: boolean;
  label?: string;
  preview(image: ImagePreview): void;
}) {
  const source = props.source;
  const uri =
    source.kind === 'tool-image'
      ? immutableAsset('tool-image', source.read)
      : immutableAsset('attachment-image', source.read);
  const action = Object.freeze({ sessionId: source.read.sessionId, uri });
  const size = signal<{ width: number; height: number } | null>(null);
  const error = signal('');
  const attempt = signal(0);
  let alive = true;
  let button: YNode | undefined;
  cleanup(() => {
    alive = false;
  });
  const owns = (selected: typeof action) =>
    selected.sessionId === action.sessionId && selected.uri === action.uri;
  const preview = (selected: typeof action) => {
    const originalButton = button;
    if (alive && owns(selected) && size())
      props.preview({
        source,
        uri,
        ...size()!,
        restoreFocus: () => {
          if (!alive || !size() || !originalButton || button !== originalButton) return false;
          originalButton.focus();
          return true;
        },
      });
  };
  const retry = (selected: typeof action) => {
    if (!alive || !owns(selected) || !error()) return;
    size.set(null);
    error.set('');
    attempt.set(attempt() + 1);
  };
  return (
    <View style={{ gap: 16 }}>
      <Button
        ref={(node) => {
          button = node;
        }}
        accessibilityLabel={props.label ?? 'Open image preview'}
        disabled={!size()}
        onPress={inputAction(preview, action)}
        style={{
          alignSelf: 'flex-start',
          maxWidth: '100%',
          backgroundColor: C.surface,
          padding: 8,
          gap: 8,
        }}
      >
        <Text style={{ color: size() ? C.accent : C.muted }}>
          {size()
            ? `Open image · ${size()!.width} × ${size()!.height}`
            : error()
              ? 'Image unavailable'
              : 'Loading image…'}
        </Text>
        <Each of={[attempt()]} key={(value: number) => value}>
          {() => (
            <Image
              src={uri}
              resizeMode="contain"
              onLoad={(dimensions) => {
                if (alive) size.set(dimensions);
              }}
              onError={(reason) => {
                if (alive) {
                  size.set(null);
                  error.set(reason.message);
                }
              }}
              style={{
                width: props.small ? 112 : 320,
                maxWidth: '100%',
                height: props.small
                  ? 64
                  : Math.max(64, Math.min(192, Math.floor(windowSize().height / 48) * 16)),
              }}
            />
          )}
        </Each>
      </Button>
      <If when={error()}>
        <Text selectable style={{ color: C.danger }}>
          {error()}
        </Text>
        <Button onPress={inputAction(retry, action)}>
          <Text style={{ color: C.accent }}>Retry image</Text>
        </Button>
      </If>
    </View>
  );
}
