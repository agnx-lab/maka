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

import { Buffer } from 'buffer';
import { cleanup, inputAction, signal, Text, View } from 'nsh';
import { Button, Dialog } from 'nsh/base';
import { windowSize } from 'nsh/device';
import { Image } from 'nsh/image';
import type { Position } from './history';
import { C } from './ui';

type ImageSource = { sessionId: string; through: number; position: Position; offset: 0 };
export type ImagePreview = {
  source: ImageSource;
  uri: string;
  width: number;
  height: number;
};

export function ToolImage(props: {
  sessionId: string;
  through: () => number;
  position: Position;
  preview(image: ImagePreview): void;
}) {
  // Capture once: a later stream fence must not reload the same immutable image.
  const source: ImageSource = Object.freeze({
    sessionId: props.sessionId,
    through: props.through(),
    position: Object.freeze({ ...props.position }),
    offset: 0,
  });
  const encoded = Buffer.from(JSON.stringify(source))
    .toString('base64')
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
  const uri = `asset:/tool-image/${encoded}`;
  const size = signal<{ width: number; height: number } | null>(null);
  const error = signal('');
  const attempt = signal(0);
  let alive = true;
  cleanup(() => {
    alive = false;
  });
  const owns = (selected: ImageSource) =>
    selected.sessionId === source.sessionId &&
    selected.through === source.through &&
    selected.position.sourceSequence === source.position.sourceSequence &&
    selected.position.itemIndex === source.position.itemIndex;
  const preview = (selected: ImageSource) => {
    if (alive && owns(selected) && size()) props.preview({ source, uri, ...size()! });
  };
  const retry = (selected: ImageSource) => {
    if (!alive || !owns(selected) || !error()) return;
    size.set(null);
    error.set('');
    attempt.set(attempt() + 1);
  };
  return (
    <View style={{ gap: 16 }}>
      <Button
        accessibilityLabel="Open image preview"
        disabled={!size()}
        onPress={inputAction(preview, source)}
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
                width: 320,
                maxWidth: '100%',
                height: Math.max(64, Math.min(192, Math.floor(windowSize().height / 48) * 16)),
              }}
            />
          )}
        </Each>
      </Button>
      <If when={error()}>
        <Text selectable style={{ color: C.danger }}>
          {error()}
        </Text>
        <Button onPress={inputAction(retry, source)}>
          <Text style={{ color: C.accent }}>Retry image</Text>
        </Button>
      </If>
    </View>
  );
}

export function ImageViewer(props: { image: ImagePreview; close(): void }) {
  const error = signal('');
  return (
    <Dialog
      animation="none"
      onOpenChange={props.close}
      style={{
        flex: 1,
        alignItems: 'center',
        justifyContent: 'center',
        padding: 16,
        backgroundColor: '#000000cc',
      }}
    >
      <View
        style={{
          width: '100%',
          height: Math.max(80, windowSize().height - 32),
          backgroundColor: C.background,
          padding: 16,
          gap: 16,
        }}
      >
        <View style={{ flexDirection: 'row', justifyContent: 'space-between', gap: 16 }}>
          <Text style={{ color: C.text, fontWeight: '700', flex: 1 }}>Image preview</Text>
          <Button onPress={props.close}>
            <Text style={{ color: C.muted }}>Close · Esc</Text>
          </Button>
        </View>
        <Image
          src={props.image.uri}
          resizeMode="contain"
          onError={(reason) => error.set(reason.message)}
          style={{ flex: 1, minHeight: 0, width: '100%' }}
        />
        <Text style={{ color: C.muted }}>
          {props.image.width} × {props.image.height} · Original image
        </Text>
        <If when={error()}>
          <Text style={{ color: C.danger }}>{error()}</Text>
        </If>
      </View>
    </Dialog>
  );
}
