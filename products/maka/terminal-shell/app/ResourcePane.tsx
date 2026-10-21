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

import { cleanup, effect, inputAction, Scroll, signal, Text, View } from 'nsh';
import type { YNode } from 'nsh';
import { Button } from 'nsh/base';
import { keymap } from 'nsh/keymap';
import { Image } from 'nsh/image';
import { Terminal } from './Terminal';
import { Terminals } from './Terminals';
import { WorkspaceFiles } from './WorkspaceFiles';
import { WorkspaceChanges } from './WorkspaceChanges';
import { CurrentDiff } from './CurrentDiff';
import { CurrentFile } from './CurrentFile';
import type { Resources } from './resources';
import type { ImagePreview } from './ImageThumbnail';
import { C } from './ui';

export function ResourcePane(props: { resources: Resources; docked: boolean }) {
  const resources = props.resources;
  const closeTarget = {
    sessionId: resources.sessionId()!,
    instance: resources.instance(),
  };
  let control: YNode | undefined;
  let unbind = () => {};
  cleanup(() => unbind());
  // Consume only a user-requested focus transfer; a resize/remount alone must
  // leave native dialog restoration and the resident composer caret intact.
  effect(() => {
    if (resources.focusRequest() && control) {
      resources.focusHandled();
      if (props.docked) control.focus();
    }
  });
  return (
    <View
      ref={(node) => {
        resources.attach(node);
        unbind();
        unbind = keymap.bind('escape', () => resources.close(closeTarget), {
          when: 'always',
          within: node,
        });
      }}
      style={{ flex: 1, minHeight: 0, backgroundColor: C.surface, padding: 8, gap: 8 }}
    >
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
        <Button
          accessibilityLabel="Browse files"
          selected={resources.active() === 'files'}
          ref={(node) => {
            control = node;
            if (resources.focusRequest()) {
              resources.focusHandled();
              if (props.docked) node.focus();
            }
          }}
          onPress={inputAction(resources.inventory, {
            sessionId: resources.sessionId()!,
            kind: 'files',
          })}
        >
          <Text style={{ color: resources.active() === 'files' ? C.accent : C.muted }}>Files</Text>
        </Button>
        <Button
          accessibilityLabel="Browse current changes"
          selected={resources.active() === 'changes'}
          onPress={inputAction(resources.inventory, {
            sessionId: resources.sessionId()!,
            kind: 'changes',
          })}
        >
          <Text style={{ color: resources.active() === 'changes' ? C.accent : C.muted }}>
            Changes
          </Text>
        </Button>
        <Button
          accessibilityLabel="Show conversation terminals"
          onPress={inputAction(resources.inventory, { sessionId: resources.sessionId()!, kind: 'terminals' })}
          style={{ paddingHorizontal: 8, backgroundColor: resources.active() === 'terminals' ? C.raised : 'transparent' }}
          hoverStyle={{ backgroundColor: C.raised }} focusStyle={{ backgroundColor: C.line }}>
          <Text style={{ color: resources.active() === 'terminals' ? C.text : C.accent }}>Terminal</Text>
        </Button>
        <View style={{ flex: 1 }} />
        <Button
          accessibilityLabel="Close workspace preview"
          onPress={inputAction(resources.close, closeTarget)}
        >
          <Text style={{ color: C.muted }}>Close · Esc</Text>
        </Button>
      </View>
      <If when={resources.tabs().length}>
        <Scroll horizontal style={{ flexGrow: 0, flexShrink: 0 }}>
          <View style={{ flexDirection: 'row', gap: 8 }}>
            <Each of={resources.tabs()} key={(tab) => tab.instance}>
              {(tab) => (
                <View
                  style={{
                    flexDirection: 'row',
                    gap: 8,
                    backgroundColor: resources.active() === tab().instance ? C.raised : C.surface,
                  }}
                >
                  <Button
                    selected={resources.active() === tab().instance}
                    onPress={inputAction(resources.select, {
                      sessionId: tab().sessionId,
                      instance: tab().instance,
                    })}
                    accessibilityLabel={`Preview ${tab().title}`}
                  >
                    <Text
                      numberOfLines={1}
                      style={{
                        color: resources.active() === tab().instance ? C.accent : C.muted,
                        maxWidth: 200,
                      }}
                    >
                      {tab().title}
                    </Text>
                  </Button>
                  <Button
                    onPress={inputAction(resources.closeTab, {
                      sessionId: tab().sessionId,
                      instance: tab().instance,
                    })}
                    accessibilityLabel={`Close ${tab().title}`}
                  >
                    <Text style={{ color: C.muted }}>×</Text>
                  </Button>
                </View>
              )}
            </Each>
          </View>
        </Scroll>
      </If>
      <If when={resources.active() === 'terminals'}><Terminals model={resources.terminals} sessionId={resources.sessionId()!} /></If>
      <If when={resources.active() === 'files'}>
        <WorkspaceFiles model={resources.files} />
      </If>
      <If when={resources.active() === 'changes' && resources.sessionId()}>
        <WorkspaceChanges sessionId={resources.sessionId()!} openDiff={resources.openDiff} />
      </If>
      <Each of={resources.activeTab() ? [resources.activeTab()!] : []} key={(tab) => tab.instance}>
        {(tab) => {
          const current = tab();
          if (current.kind === 'file') return <CurrentFile model={current.model} />;
          if (current.kind === 'diff') return <CurrentDiff model={current.model} />;
          if (current.kind === 'terminal') return <Terminal model={current.model} />;
          return <OriginalImage image={current.image} />;
        }}
      </Each>
    </View>
  );
}

function OriginalImage(props: { image: ImagePreview }) {
  const error = signal('');
  let mounted = true;
  cleanup(() => {
    mounted = false;
  });
  return (
    <View style={{ flex: 1, minHeight: 0, gap: 8 }}>
      <Text style={{ color: C.text }}>Image preview</Text>
      <Image
        src={props.image.uri}
        resizeMode="contain"
        onError={(reason) => {
          if (mounted) error.set(reason.message);
        }}
        style={{ flex: 1, minHeight: 0, width: '100%' }}
      />
      <Text style={{ color: C.muted }}>
        {props.image.width} × {props.image.height} · Original image
      </Text>
      <If when={error()}>
        <Text style={{ color: C.danger }}>{error()}</Text>
      </If>
    </View>
  );
}
