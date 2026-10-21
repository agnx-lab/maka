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

import { cleanup, computed, inputAction, Scroll, signal, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import { Clipboard } from 'nsh/clipboard';
import { platform, windowSize } from 'nsh/device';
import type { MarkdownBlock, MarkdownRun, MarkdownTable } from 'maka/markdown';
import {
  C,
  bodyTextStyle,
  secondaryTextStyle,
  captionTextStyle,
  codeTextStyle,
  controlStyle,
} from './ui';
import { Icon } from './Icon';

export function MarkdownBlockView(props: { block: () => MarkdownBlock; compact?: boolean }) {
  const block = props.block;
  const container = () =>
    block().kind === 'code' || block().marker !== '' || block().quote > 0 || block().indent > 0;
  const inset = () =>
    Math.min((block().indent + Math.max(0, block().quote - 1)) * 16, windowSize().width / 4);
  return (
    <If when={container()} else={<MarkdownBlockContent block={block} compact={props.compact} />}>
      <View
        style={{
          flexDirection: 'row',
          gap: 8,
          paddingLeft: inset(),
          borderLeftWidth: block().quote > 0 ? 2 : 0,
          borderLeftColor: C.line,
        }}
      >
        <If when={block().marker}>
          <Text
            style={{ ...(props.compact ? secondaryTextStyle : {}), color: C.muted, flexShrink: 0 }}
          >
            {block().marker}
          </Text>
        </If>
        <View style={{ flex: 1, minWidth: 0 }}>
          <MarkdownBlockContent block={block} compact={props.compact} />
        </View>
      </View>
    </If>
  );
}

function InlineRuns(props: { runs(): MarkdownRun[]; compact?: boolean }) {
  return (
    <Each of={props.runs()} key={(run) => run.key}>
      {(run) => (
        <Text
          style={{
            fontWeight: run().strong ? '700' : undefined,
            fontStyle: run().emphasis ? 'italic' : undefined,
            textDecorationLine: run().strike ? 'line-through' : undefined,
            fontFamily: props.compact && run().code ? 'monospace' : undefined,
            color: run().code ? (props.compact ? C.muted : C.accentHover) : undefined,
            backgroundColor: run().code ? C.surface : undefined,
          }}
        >
          {run().text}
        </Text>
      )}
    </Each>
  );
}

function MarkdownBlockContent(props: { block: () => MarkdownBlock; compact?: boolean }) {
  const block = props.block;
  return (
    <If
      when={block().kind === 'table'}
      else={
        <If
          when={block().kind === 'code'}
          else={
            <If
              when={block().kind === 'paragraph' || block().kind === 'heading'}
              else={
                <If
                  when={block().kind === 'rule'}
                  else={
                    <Text
                      selectable
                      style={{
                        ...(props.compact ? secondaryTextStyle : {}),
                        color: props.compact ? C.muted : C.text,
                      }}
                    >
                      {block().text}
                    </Text>
                  }
                >
                  <If
                    when={platform.isTerminal}
                    else={<View style={{ height: 1, backgroundColor: C.line }} />}
                  >
                    <Text style={{ ...(props.compact ? secondaryTextStyle : {}), color: C.muted }}>
                      ───
                    </Text>
                  </If>
                </If>
              }
            >
              <Text
                selectable
                style={{
                  ...(props.compact ? secondaryTextStyle : bodyTextStyle),
                  color: props.compact ? C.muted : block().quote > 0 ? C.secondary : C.text,
                  fontSize: props.compact
                    ? secondaryTextStyle.fontSize
                    : block().kind === 'heading'
                      ? 18
                      : 14,
                  fontWeight: block().kind === 'heading' ? (props.compact ? '600' : '700') : '400',
                }}
              >
                <InlineRuns runs={() => block().runs} compact={props.compact} />
              </Text>
            </If>
          }
        >
          <CodeBlock block={block} compact={props.compact} />
        </If>
      }
    >
      <TableBlock table={() => block().table!} compact={props.compact} />
    </If>
  );
}

function TableBlock(props: { table(): MarkdownTable; compact?: boolean }) {
  const cells = computed(() => {
    const table = props.table();
    return table.rows.flatMap((row, rowIndex) =>
      table.columns.map((align, column) => ({
        key: row.cells[column]?.key ?? `${row.key}:empty:${column}`,
        runs: row.cells[column]?.runs ?? [],
        align,
        row: rowIndex,
        last: rowIndex === table.rows.length - 1,
      })),
    );
  });
  return (
    <Scroll horizontal accessibilityLabel="Markdown table" style={{ flexGrow: 0 }}>
      <View
        style={{
          display: 'grid',
          gridTemplateColumns: `repeat(${props.table().columns.length}, max-content)`,
          alignSelf: 'flex-start',
          borderWidth: platform.isTerminal ? 0 : 1,
          borderColor: C.line,
          borderRadius: platform.isTerminal ? 0 : 8,
          overflow: 'hidden',
        }}
      >
        <Each of={cells()} key={(cell) => cell.key}>
          {(cell) => (
            <View
              style={{
                paddingHorizontal: props.compact || platform.isTerminal ? 8 : 12,
                paddingVertical: props.compact ? 4 : 8,
                minHeight:
                  (props.compact ? secondaryTextStyle.lineHeight : bodyTextStyle.lineHeight) +
                  (props.compact ? 8 : 16),
                borderBottomWidth: platform.isTerminal || cell().last ? 0 : 1,
                borderBottomColor: C.line,
                backgroundColor:
                  cell().row === 0 ? C.raised : cell().row % 2 ? C.surface : C.background,
              }}
            >
              <Text
                selectable
                style={{
                  ...(props.compact ? secondaryTextStyle : bodyTextStyle),
                  color: props.compact ? C.muted : C.text,
                  fontWeight: cell().row === 0 ? '600' : '400',
                  textAlign: cell().align,
                }}
              >
                <InlineRuns runs={() => cell().runs} compact={props.compact} />
              </Text>
            </View>
          )}
        </Each>
      </View>
    </Scroll>
  );
}

type CopyState =
  | { kind: 'idle' }
  | { kind: 'copying' | 'copied'; text: string }
  | { kind: 'failed'; text: string; message: string };

function CodeBlock(props: { block: () => MarkdownBlock; compact?: boolean }) {
  const block = props.block;
  const key = computed(() => block().key);
  const text = computed(() => block().text);
  const state = signal<CopyState>({ kind: 'idle' });
  let alive = true;
  cleanup(() => {
    alive = false;
  });
  const action = computed(() => {
    const originalKey = key();
    const originalText = text();
    const owns = () => alive && key() === originalKey && text() === originalText;
    // Handler identity retains this exact rendered version without serializing
    // the code into the action's JSON arguments. Unchanged code keeps its handler.
    return inputAction(async () => {
      if (!owns() || state().kind === 'copying') return;
      state.set({ kind: 'copying', text: originalText });
      try {
        await Clipboard.writeText(originalText);
        if (alive) state.set(owns() ? { kind: 'copied', text: originalText } : { kind: 'idle' });
      } catch (reason) {
        if (alive)
          state.set(
            owns()
              ? {
                  kind: 'failed',
                  text: originalText,
                  message: reason instanceof Error ? reason.message : String(reason),
                }
              : { kind: 'idle' },
          );
      }
    }, null);
  });
  const copied = () => {
    const current = state();
    return current.kind === 'copied' && current.text === text();
  };
  const failure = () => {
    const current = state();
    return current.kind === 'failed' && current.text === text() ? current.message : '';
  };
  return (
    <View
      style={{
        backgroundColor: C.surface,
        borderRadius: props.compact ? 8 : 12,
        padding: props.compact ? 8 : 16,
        gap: props.compact ? 4 : 8,
      }}
    >
      <View style={{ flexDirection: 'row', alignItems: 'center', minHeight: 32, gap: 8 }}>
        <Text style={{ ...captionTextStyle, color: C.muted, flex: 1 }}>
          {block().language || 'code'}
        </Text>
        <Text style={{ ...captionTextStyle, color: C.muted }}>{copied() ? 'Copied' : ''}</Text>
        <Button
          accessibilityLabel="Copy code"
          tooltip={copied() ? 'Code copied' : 'Copy code'}
          onPress={action()}
          style={{ ...controlStyle, width: 32, paddingHorizontal: 8, alignItems: 'center' }}
          hoverStyle={{ backgroundColor: C.line }}
          focusStyle={{ backgroundColor: C.raised }}
        >
          <Icon name={copied() ? 'check' : 'copy'} color={C.secondary} />
        </Button>
      </View>
      <Scroll horizontal>
        <Text selectable style={{ ...codeTextStyle, color: props.compact ? C.muted : C.text }}>
          {block().text}
        </Text>
      </Scroll>
      <If when={failure()}>
        <Text selectable style={{ ...captionTextStyle, color: C.danger }}>
          {failure()}
        </Text>
      </If>
    </View>
  );
}
