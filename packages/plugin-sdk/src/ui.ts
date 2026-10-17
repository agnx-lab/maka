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

/** Typed declarations for the actual native scene protocols. */
import type {
  TerminalScene,
  ComponentSlot,
  ReadingProps,
  InputProps,
  TextareaProps,
  LabelProps,
  MarkdownProps,
  FilePickerProps,
  SourceTextProps,
  SelectionMenuProps,
} from './generated/runtime.js';

const authoredNode: unique symbol = Symbol('maka.terminal.node');
export interface TerminalNodeBrand {
  readonly [authoredNode]: true;
}

type Declaration<Wire> = Wire extends unknown
  ? {
      [K in keyof Wire]: K extends 'children'
        ? TerminalNode[]
        : K extends 'background' | 'content' | 'trigger'
          ? Wire[K] extends TerminalScene
            ? TerminalNode
            : Wire[K]
          : Wire[K];
    } & TerminalNodeBrand
  : never;
export type TerminalNode = Declaration<TerminalScene>;
export type TerminalKind<K extends TerminalNode['kind']> = Extract<TerminalNode, { kind: K }>;
export type TerminalProps<K extends TerminalNode['kind']> = Props<TerminalKind<K>>;
type Props<N extends TerminalNodeBrand> = Omit<N, 'kind' | typeof authoredNode> &
  Partial<Pick<N, typeof authoredNode>>;
export type ComponentDeclaration<P = unknown> = Omit<ComponentSlot, 'props' | 'member'> & {
  member?: number;
  props: P;
};
type ComponentFor<P> = ComponentDeclaration<P> & Partial<TerminalNodeBrand>;
export type ReadingDeclaration = Omit<ReadingProps, 'content'> & {
  content: TerminalNode;
};

// The non-enumerable brand distinguishes authored nodes from raw wire objects.
// JSON preserves only the original scene and component contracts.
function node<const N extends object>(value: N): N & TerminalNodeBrand {
  const result = Object.assign(value, { [authoredNode]: true as const });
  Object.defineProperty(result, authoredNode, { enumerable: false });
  return result;
}

function component<P>(value: ComponentFor<P>) {
  return node({ ...value, member: value.member ?? 0, kind: 'component' });
}

export const ui = {
  terminal: {
    contextMenu(value: Props<TerminalKind<'context_menu'>>): TerminalKind<'context_menu'> {
      return node({ ...value, kind: 'context_menu' });
    },
    adaptivePane(value: Props<TerminalKind<'adaptive_pane'>>): TerminalKind<'adaptive_pane'> {
      return node({ ...value, kind: 'adaptive_pane' });
    },
    item(value: Props<TerminalKind<'item'>>): TerminalKind<'item'> {
      return node({ ...value, kind: 'item' });
    },
    rule(value: Props<TerminalKind<'rule'>>): TerminalKind<'rule'> {
      return node({ ...value, kind: 'rule' });
    },
    choice(value: Props<TerminalKind<'choice'>>): TerminalKind<'choice'> {
      return node({ ...value, kind: 'choice' });
    },
    text(value: Props<TerminalKind<'text'>>): TerminalKind<'text'> {
      return node({ ...value, kind: 'text' });
    },
    button(value: Props<TerminalKind<'button'>>): TerminalKind<'button'> {
      return node({ ...value, kind: 'button' });
    },
    stack(value: Props<TerminalKind<'stack'>>): TerminalKind<'stack'> {
      return node({ ...value, kind: 'stack' });
    },
    dialog(value: Props<TerminalKind<'dialog'>>): TerminalKind<'dialog'> {
      return node({ ...value, kind: 'dialog' });
    },
    component<P>(value: ComponentFor<P>): TerminalKind<'component'> {
      return component(value);
    },
    input(value: ComponentFor<InputProps>): TerminalKind<'component'> {
      return component(value);
    },
    textarea(value: ComponentFor<TextareaProps>): TerminalKind<'component'> {
      return component(value);
    },
    label(value: ComponentFor<LabelProps>): TerminalKind<'component'> {
      return component(value);
    },
    markdown(value: ComponentFor<MarkdownProps>): TerminalKind<'component'> {
      return component(value);
    },
    filePicker(value: ComponentFor<FilePickerProps>): TerminalKind<'component'> {
      return component(value);
    },
    sourceText(value: ComponentFor<SourceTextProps>): TerminalKind<'component'> {
      return component(value);
    },
    selectionMenu(value: ComponentFor<SelectionMenuProps>): TerminalKind<'component'> {
      return component(value);
    },
    reading(value: ComponentFor<ReadingDeclaration>): TerminalKind<'component'> {
      return component(value);
    },
  },
};
