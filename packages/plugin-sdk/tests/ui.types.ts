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

import { ui } from '../src/index.js';
import type { TerminalScene, TerminalNode, Component } from '../src/index.js';
const terminalText = ui.terminal.text({ key: 'terminal-text', text: 'Hello', color: 0xffffff });
const terminal = ui.terminal.stack({
  key: 'terminal',
  direction: 'column',
  gap: 1,
  padding: 0,
  background: 0,
  children: [terminalText],
  scroll: 'anchor',
  layout: { grow: 1 },
});
const terminalWire: TerminalScene = terminal;
const terminalNode: TerminalNode = terminal;
void [terminalWire, terminalNode];
ui.terminal.dialog({
  key: 'dialog',
  label: 'Dialog',
  background: terminal,
  content: terminalText,
  open: true,
  width: 40,
  height: 10,
  background_color: 0,
  border_color: 0xffffff,
});
ui.terminal.reading({
  key: 'reading',
  binding: 'reading',
  props: { content: terminal },
  on: { observed: 'viewport' },
});
ui.terminal.component({
  key: 'plugin',
  binding: 'external',
  member: 1,
  props: { custom: true },
  on: { changed: 'change' },
});
ui.terminal.stack({ ...terminal, children: [terminalText] });
// @ts-expect-error Raw wire objects have no authoring target; construct them with ui.terminal.
const inferredTerminal: TerminalNode = terminalWire;
// @ts-expect-error Terminal presentation has no popup primitive.
ui.terminal.popup({
  key: 'popup',
  label: 'Menu',
  trigger: terminalText,
  content: terminal,
  open: true,
});
// @ts-expect-error Built-in label requires its real appearance contract.
ui.terminal.label({ key: 'label', binding: 'label', props: { text: 'Hello' }, on: {} });
// @ts-expect-error Input props cannot use string flags.
ui.terminal.input({ key: 'input', binding: 'input', props: { disabled: 'yes' }, on: {} });
const component: Component<null, null, TerminalNode> = {
  mount: () => ({
    frame: { state: null, revision: 0, surface: null, view: terminal },
    events: [],
    actions: [],
    subscriptions: [],
  }),
  update: () => ({
    frame: { state: null, revision: 0, surface: null, view: terminal },
    events: [],
    actions: [],
    subscriptions: [],
  }),
  event: () => ({
    frame: { state: null, revision: 0, surface: null, view: terminal },
    events: [],
    actions: [],
    subscriptions: [],
  }),
};
void [component, inferredTerminal];
const terminalChoice = ui.terminal.choice({
  key: 'thinking',
  label: 'Thinking',
  options: [
    { key: 'low', label: 'Low' },
    { key: 'high', label: 'High' },
  ],
  current: 'high',
  foreground: 0xffffff,
  background: 0,
  border: 0x888888,
  selected: 0x224466,
  accent: 0x4488ff,
});
ui.terminal.stack({ ...terminal, children: [terminalChoice] });
// @ts-expect-error Choice values are stable string keys, not array positions.
ui.terminal.choice({ ...terminalChoice, current: 1 });

const terminalItem = ui.terminal.item({
  key: 'model-row',
  content: terminal,
  current: true,
  follow_focus: true,
  submit: true,
});
ui.terminal.stack({ ...terminal, focus_group: true, children: [terminalItem] });
// @ts-expect-error Interaction policy is a typed flag, not a callback or string.
ui.terminal.item({ ...terminalItem, follow_focus: 'yes' });
