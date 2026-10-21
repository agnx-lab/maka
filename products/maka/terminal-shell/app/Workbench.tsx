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

import { effect, inputAction, signal, View, type JSX, type YNode } from 'nsh';
import { windowSize } from 'nsh/device';
import { keymap } from 'nsh/keymap';
import { Dialog } from 'nsh/base';
import { C } from './ui';
import { WorkspaceBrowser, type WorkspaceBrowserProps } from './WorkspaceBrowser';
import { ResourcePane } from './ResourcePane';
import type { Resources } from './resources';

const CELL = 8;
const CENTER_MIN = 72 * CELL;
const BROWSER_MIN = 24 * CELL;
const BROWSER_MAX = 40 * CELL;
const PREVIEW_MIN = 40 * CELL;

export const canDockBrowser = () => windowSize().width >= CENTER_MIN + BROWSER_MIN + CELL;
// Current layout policy, not a claim that narrower terminal columns are impossible.
const canDockPreview = () => windowSize().width >= CENTER_MIN + PREVIEW_MIN + CELL;

export function Workbench(props: {
  expanded(): boolean;
  browser: WorkspaceBrowserProps;
  stopAction(): () => void;
  resources: Resources;
  children: JSX.Element;
}) {
  const preferredWidth = signal(32 * CELL);
  const preview = () => props.resources.shown() && canDockPreview();
  const browser = () =>
    canDockBrowser() &&
    props.expanded() &&
    (!preview() || windowSize().width >= CENTER_MIN + BROWSER_MIN + PREVIEW_MIN + 2 * CELL);
  const width = () =>
    Math.min(
      preferredWidth(),
      windowSize().width - CENTER_MIN - CELL - (preview() ? PREVIEW_MIN + CELL : 0),
    );
  const previewWidth = () =>
    Math.min(48 * CELL, windowSize().width - CENTER_MIN - CELL - (browser() ? width() + CELL : 0));
  const clamp = (value: number) =>
    Math.max(
      BROWSER_MIN,
      Math.min(
        BROWSER_MAX,
        windowSize().width - CENTER_MIN - CELL,
        Math.round(value / CELL) * CELL,
      ),
    );
  let drag: { x: number; width: number; viewport: number } | null = null;
  const content = signal<YNode | null>(null);
  effect(() => {
    const stop = props.stopAction();
    const node = content();
    if (node) return keymap.bind('escape', stop, { when: 'always', within: node });
  });
  effect(() => {
    windowSize().width;
    windowSize().height;
    drag = null;
  });
  const resize = (delta: number) => preferredWidth.set(clamp(width() + delta * CELL));
  const previewDialog = signal(false);
  let wasPreviewDocked = canDockPreview();
  effect(() => {
    const docked = canDockPreview();
    const root = props.resources.root();
    if (!props.resources.shown() || docked) previewDialog.set(false);
    else if (!wasPreviewDocked || (root && keymap.focusedWithin(root))) previewDialog.set(true);
    else {
      props.resources.hide();
      previewDialog.set(false);
    }
    wasPreviewDocked = docked;
  });
  return (
    <View style={{ flex: 1, flexDirection: 'row', minHeight: 0 }}>
      <If when={browser()}>
        <View style={{ width: width(), flexShrink: 0, backgroundColor: C.surface }}>
          <WorkspaceBrowser {...props.browser} docked resize={resize} />
        </View>
        <View
          accessibilityLabel="Resize session browser"
          onStartShouldSetResponder={() => true}
          onResponderGrant={(event) => {
            drag = { x: event.pageX, width: width(), viewport: windowSize().width };
            return true;
          }}
          onResponderMove={(event) => {
            if (drag && drag.viewport === windowSize().width && canDockBrowser())
              preferredWidth.set(clamp(drag.width + event.pageX - drag.x));
          }}
          onResponderRelease={() => {
            drag = null;
          }}
          onResponderTerminate={() => {
            drag = null;
          }}
          style={{ width: CELL, flexShrink: 0, alignItems: 'center', backgroundColor: C.background }}
          hoverStyle={{ backgroundColor: C.line }}
        >
          <View pointerEvents="none" style={{ width: 1, height: '100%', backgroundColor: C.line }} />
        </View>
      </If>
      <View ref={(node) => content.set(node)} style={{ flex: 1, minWidth: 0, minHeight: 0 }}>
        {props.children}
      </View>
      <If when={preview()}>
        <View style={{ width: CELL, backgroundColor: C.line }} />
        <View style={{ width: previewWidth(), flexShrink: 0, minHeight: 0 }}>
          <ResourcePane resources={props.resources} docked />
        </View>
      </If>
      <If when={previewDialog()}>
        <Dialog
          animation="none"
          onOpenChange={inputAction(props.resources.close, {
            sessionId: props.resources.sessionId()!,
            instance: props.resources.instance(),
          })}
          style={{ flex: 1, padding: 8, backgroundColor: '#000000cc' }}
        >
          <ResourcePane resources={props.resources} docked={false} />
        </Dialog>
      </If>
    </View>
  );
}
