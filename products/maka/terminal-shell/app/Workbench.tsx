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

import { cleanup, effect, signal, View, type JSX } from 'nsh';
import { windowSize } from 'nsh/device';
import { keymap } from 'nsh/keymap';
import { C } from './ui';
import { WorkspaceBrowser, type WorkspaceBrowserProps } from './WorkspaceBrowser';

const CELL = 8;
const CENTER_MIN = 72 * CELL;
const BROWSER_MIN = 24 * CELL;
const BROWSER_MAX = 40 * CELL;

export const canDockBrowser = () => windowSize().width >= CENTER_MIN + BROWSER_MIN + CELL;

export function Workbench(props: {
  expanded(): boolean;
  browser: WorkspaceBrowserProps;
  stop(): void;
  children: JSX.Element;
}) {
  const preferredWidth = signal(28 * CELL);
  const width = () => Math.min(preferredWidth(), windowSize().width - CENTER_MIN - CELL);
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
  let removeEscape = () => {};
  cleanup(() => removeEscape());
  effect(() => {
    windowSize().width;
    windowSize().height;
    drag = null;
  });
  const resize = (delta: number) => preferredWidth.set(clamp(width() + delta * CELL));
  return (
    <View style={{ flex: 1, flexDirection: 'row', minHeight: 0 }}>
      <If when={canDockBrowser() && props.expanded()}>
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
          style={{ width: CELL, flexShrink: 0, backgroundColor: C.line }}
          hoverStyle={{ backgroundColor: C.accent }}
        />
      </If>
      <View
        ref={(node) => {
          removeEscape();
          removeEscape = keymap.bind('escape', props.stop, { when: 'always', within: node });
        }}
        style={{ flex: 1, minWidth: 0, minHeight: 0 }}
      >
        {props.children}
      </View>
    </View>
  );
}
