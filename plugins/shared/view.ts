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

import { ui } from '../../packages/plugin-sdk/src/index.js';
import type {
  TerminalNode,
  TerminalColor,
  Emission,
  Output,
  TerminalLayout,
  TerminalResponsiveStack,
  Shortcut,
  FocusShortcut,
  ReadingProps,
  TerminalChoiceOption,
  TerminalPaneCommand,
} from '../../packages/plugin-sdk/src/index.js';
import { animation, type ActivityAnimation } from './motion.js';
import type { Theme } from './contracts.js';
export type Node = TerminalNode;
interface TextOptions {
  align?: 'start' | 'center' | 'end';
  truncate?: boolean;
}
interface ButtonOptions {
  align?: 'start' | 'center' | 'end';
  background?: number;
  foreground?: number;
}
interface BoxOptions {
  row?: boolean;
  gap?: number;
  padding?: number;
  surface?: boolean;
  scroll?: 'none' | 'manual' | 'anchor' | 'follow_end';
  visible?: boolean;
  terminal?: TerminalLayout;
  wide?: TerminalResponsiveStack;
  background?: number;
  viewport?: boolean;
  focusGroup?: boolean;
  autoFocus?: boolean;
  wheel?: { up: string; down: string };
  border?: boolean;
  borderColor?: number;
  busy?: boolean;
  shortcuts?: Shortcut[];
  focusShortcuts?: FocusShortcut[];
}
interface Painter<N extends Node> {
  activity(
    key: string,
    kind: ActivityAnimation,
    foreground?: number,
    prefix?: string,
    suffix?: string,
  ): N;
  text(
    key: string,
    text: string,
    color?: number,
    weight?: number,
    options?: TextOptions,
    hint?: string,
  ): N;
  button(
    key: string,
    label: string,
    disabled?: boolean,
    primary?: boolean,
    options?: ButtonOptions,
    copyText?: string,
    hint?: string,
  ): N;
  toggle(key: string, label: string, checked: boolean, disabled?: boolean, hint?: string): N;
  contextMenu(
    key: string,
    label: string,
    background: N,
    actions: N[],
    copySelectionLabel?: string,
  ): N;
  choice(
    key: string,
    label: string,
    options: TerminalChoiceOption[],
    current: string | null,
    disabled: boolean,
  ): N;
  item(
    key: string,
    content: N,
    current: boolean,
    disabled: boolean,
    interaction?: {
      followFocus?: boolean;
      submit?: boolean;
      submitSpace?: boolean;
    },
  ): N;
  focusButton(key: string, label: string, child: string): N;
  rule(key: string, direction: 'row' | 'column'): N;
  adaptivePane(
    key: string,
    label: string,
    background: N,
    content: N,
    command: TerminalPaneCommand | null,
  ): N;
  dialog(
    key: string,
    label: string,
    background: N,
    content: N,
    open: boolean,
    terminalSize?: {
      width?: number;
      height?: number;
      padding?: number;
      anchorToBackground?: boolean;
    },
  ): N;
  box(key: string, children: N[], options?: BoxOptions): N;
}
export interface Presentation<N extends Node> {
  paint(theme: Theme): Painter<N>;
  slot<P>(key: string, binding: string, props: P, on: Record<string, string>, member?: number): N;
  reading(
    key: string,
    props: Omit<ReadingProps, 'content'> & { content: N },
    on: Record<string, string>,
  ): N;
}
const rgba = (n: number) => n >>> 8;
function toggleSymbol(checked: boolean, ascii: boolean): string {
  return ascii ? (checked ? '[x]' : '[ ]') : checked ? '━●' : '○─';
}
function terminalPaint(theme: Theme): Painter<TerminalNode> {
  const color = (value: number): TerminalColor => {
    const palette = theme.terminal;
    if (!palette) return rgba(value);
    const roles = {
      background: 'background',
      surface: 'surface',
      text: 'foreground',
      muted: 'muted',
      accent: 'accent',
      border: 'border',
      selected: 'selection',
      thinking: 'thinking',
      success: 'success',
      warning: 'warning',
      error: 'error',
      subtle: 'subtle',
      scrollbar: 'scrollbar',
      selectionText: 'selectionText',
      search: 'search',
      searchActive: 'searchActive',
      searchText: 'searchText',
    } as const;
    for (const [role, native] of Object.entries(roles)) {
      if (theme[role as keyof typeof roles] === value) return palette[native];
    }
    return rgba(value);
  };
  return {
    toggle: (key, label, checked, disabled = false, hint = '') =>
      ui.terminal.stack({
        key: key + '-row',
        direction: 'row',
        gap: 0,
        padding: 0,
        children: [
          ui.terminal.stack({
            key: key + '-content',
            direction: 'row',
            gap: 2,
            padding: 0,
            layout: { basis_fraction: { numerator: 1, denominator: 1, max: 52 }, shrink: true },
            children: [
              ui.terminal.stack({
                key: key + '-label-area',
                direction: 'column',
                gap: 0,
                padding: 0,
                layout: { grow: 1, shrink: true },
                children: [
                  ui.terminal.text({
                    key: key + '-label',
                    text: label,
                    color: color(theme.text),
                    truncate: true,
                    hint,
                  }),
                ],
              }),
              ui.terminal.stack({
                key: key + '-switch-area',
                direction: 'column',
                gap: 0,
                padding: 0,
                layout: { basis: theme.ascii ? 3 : 2 },
                children: [
                  ui.terminal.button({
                    key,
                    label: toggleSymbol(checked, theme.ascii),
                    disabled,
                    color: color(checked ? theme.accent : theme.subtle),
                    hint,
                  }),
                ],
              }),
            ],
          }),
        ],
      }),
    activity: (key, kind, foreground = theme.accent, prefix = '', suffix = '') => {
      const frames = animation(kind, theme.ascii, theme.motion, prefix, suffix);
      return ui.terminal.text({
        key,
        text: frames[0]!.text,
        color: color(foreground),
        animation: theme.motion ? frames : [],
        truncate: true,
      });
    },
    rule: (key, direction) =>
      ui.terminal.rule({ key, direction, color: color(theme.border), ascii: theme.ascii }),
    adaptivePane: (key, label, background, content, command) =>
      ui.terminal.adaptivePane({
        key,
        background,
        content,
        command,
        pane_width: 32,
        inline_min_width: 88,
        auto_inline_min_width: 90,
        motion: theme.motion,
        overlay: {
          ascii: theme.ascii,
          label,
          width: 72,
          height: 24,
          padding: 1,
          background_color: color(theme.background),
          border_color: color(theme.border),
          title_color: color(theme.text),
        },
      }),
    item: (key, content, current, disabled, interaction = {}) =>
      ui.terminal.item({
        key,
        content,
        current,
        disabled,
        selected_color: theme.terminal?.terminal ? undefined : color(theme.selected),
        selected_foreground: theme.terminal?.terminal ? undefined : color(theme.selectionText),
        follow_focus: interaction.followFocus ?? false,
        submit: interaction.submit ?? false,
        submit_space: interaction.submitSpace ?? false,
      }),
    contextMenu: (key, label, background, actions, copySelectionLabel) => {
      const buttons = (nodes: TerminalNode[]): TerminalNode[] =>
        nodes.flatMap((node) =>
          node.kind === 'stack'
            ? buttons(node.children)
            : node.kind === 'button'
              ? [
                  {
                    ...node,
                    align: 'start' as const,
                    background: color(theme.surface),
                    hover_background: color(theme.selected),
                  },
                ]
              : [node],
        );
      return ui.terminal.contextMenu({
        key,
        label,
        background,
        copy_selection_label: copySelectionLabel,
        content: ui.terminal.stack({
          key: key + '-actions',
          direction: 'column',
          children: buttons(actions),
          gap: 0,
          padding: 0,
        }),
        width: 32,
        height: 18,
        padding: 0,
        ascii: theme.ascii,
        background_color: color(theme.surface),
        border_color: color(theme.border),
        title_color: color(theme.text),
      });
    },
    choice: (key, label, options, current, disabled) =>
      ui.terminal.choice({
        key,
        label,
        options,
        current,
        disabled,
        ascii: theme.ascii,
        foreground: color(theme.text),
        background: color(theme.surface),
        border: color(theme.border),
        selected: color(theme.selected),
        accent: color(theme.accent),
      }),
    focusButton: (key, label, child) =>
      ui.terminal.button({
        key,
        label,
        focus_child: child,
        color: color(theme.muted),
        background: color(theme.surface),
        hover_background: color(theme.border),
      }),
    text: (key, text, foreground = theme.text, weight = 400, options = {}, hint = '') =>
      ui.terminal.text({
        key,
        text,
        color: color(foreground),
        hint,
        truncate: options.truncate ?? false,
        bold: weight >= 600,
        align: options.align === 'center' ? 'center' : options.align === 'end' ? 'end' : 'start',
      }),
    button: (key, label, disabled = false, primary = false, options = {}, copyText, hint = '') =>
      ui.terminal.button({
        key,
        label,
        disabled,
        copy_text: copyText,
        hint,
        color: color(
          disabled ? theme.muted : (options.foreground ?? (primary ? theme.accent : theme.text)),
        ),
        align: options.align === 'start' ? 'start' : options.align === 'end' ? 'end' : 'center',
        background:
          options.background === 0 ? undefined : color(options.background ?? theme.surface),
        hover_background:
          options.background === 0 || theme.terminal?.terminal ? undefined : color(theme.border),
        bold: primary,
      }),
    dialog: (key, label, background, content, open, terminalSize = {}) =>
      ui.terminal.dialog({
        ascii: theme.ascii,
        key,
        label,
        background,
        content,
        open,
        width: terminalSize.width ?? 72,
        anchor_to_background: terminalSize.anchorToBackground ?? false,
        height: terminalSize.height ?? 20,
        padding: terminalSize.padding ?? 1,
        background_color: color(theme.background),
        border_color: color(theme.border),
        title_color: color(theme.text),
      }),
    box: (key, children, options = {}) =>
      ui.terminal.stack({
        key,
        direction: options.row ? 'row' : 'column',
        wide: options.wide ?? null,
        gap: options.gap ?? 1,
        padding: options.padding ?? 0,
        background:
          options.background !== undefined
            ? color(options.background)
            : options.surface
              ? color(theme.surface)
              : undefined,
        children,
        visible: options.visible ?? true,
        shortcuts: options.shortcuts ?? [],
        focus_shortcuts: options.focusShortcuts ?? [],
        viewport: options.viewport ?? false,
        focus_group: options.focusGroup ?? false,
        auto_focus: options.autoFocus ?? false,
        wheel: options.wheel ?? null,
        border: options.border
          ? {
              breathing: options.busy === true && theme.motion,
              color: color(options.borderColor ?? (options.busy ? theme.accent : theme.border)),
              focused_color: color(options.borderColor ?? theme.accent),
              style: theme.ascii ? 'ascii' : 'rounded',
            }
          : null,
        layout: options.terminal ?? {},
        scroll: options.scroll ?? 'none',
      }),
  };
}
export const presentations: { tui: Presentation<TerminalNode> } = {
  tui: {
    paint: terminalPaint,
    slot: (key, binding, props, on, member) =>
      ui.terminal.component({ key, binding, props, on, member }),
    reading: (key, props, on) => ui.terminal.reading({ key, binding: 'reading', props, on }),
  },
};
export function output<S extends { revision: number }, N extends Node>(
  state: S,
  view: N,
  events: Emission[] = [],
): Output<S, N> {
  return {
    frame: { state, revision: state.revision, surface: null, view },
    events,
    actions: [],
    subscriptions: [],
  };
}

// Disposable labels must not turn one catalog entry into an unbounded terminal
// layout. The full immutable identity/configuration remains in the domain DTO.
export function compactLabel(value: string, limit: number): string {
  const characters = [...value.replace(/\s+/gu, ' ').trim()];
  return characters.length > limit
    ? characters.slice(0, limit - 1).join('') + '…'
    : characters.join('');
}
