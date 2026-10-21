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

import { cleanup, computed, effect, Scroll, signal, Text, View } from 'nsh';
import { windowSize } from 'nsh/device';
import { markdown } from 'maka/markdown';
import type { MarkdownBlock } from 'maka/markdown';
import { hasCompletePrefix, type Row } from './history';
import { C } from './ui';

type Input = { messageId: string; field: string; source: string };
type Projection = { input: Input; blocks: MarkdownBlock[] };
const sameInput = (left: Input | null, right: Input | null) =>
  left === right ||
  (left !== null &&
    right !== null &&
    left.messageId === right.messageId &&
    left.field === right.field &&
    left.source === right.source);

export function MarkdownBody(props: { row: () => Row }) {
  const projection = signal<Projection | null>(null);
  const failure = signal<{ input: Input; message: string } | null>(null);
  let wanted: Input | null = null;
  let preparing = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let nextStart = 0;
  let alive = true;
  const input = (): Input | null => {
    const row = props.row();
    return hasCompletePrefix(row)
      ? { messageId: row.messageId, field: row.field, source: row.text }
      : null;
  };
  const compatible = (parsed: Input, current: Input | null) =>
    current !== null &&
    current.messageId === parsed.messageId &&
    current.field === parsed.field &&
    current.source.startsWith(parsed.source);
  function schedule() {
    if (!alive || preparing || timer !== undefined || wanted === null) return;
    if (
      sameInput(projection()?.input ?? null, wanted) ||
      sameInput(failure()?.input ?? null, wanted)
    )
      return;
    timer = setTimeout(
      () => {
        timer = undefined;
        void prepare();
      },
      Math.max(0, nextStart - Date.now()),
    );
  }
  async function prepare() {
    if (!alive || wanted === null) return;
    const captured = wanted;
    preparing = true;
    nextStart = Date.now() + 50;
    try {
      const blocks = await markdown.parse(captured.source);
      const current = input();
      if (
        alive &&
        compatible(captured, current) &&
        wanted !== null &&
        compatible(wanted, current)
      ) {
        projection.set({ input: captured, blocks });
        failure.set(null);
      }
    } catch (reason) {
      if (alive && sameInput(captured, wanted)) {
        projection.set(null);
        failure.set({ input: captured, message: String(reason) });
      }
    } finally {
      preparing = false;
      schedule();
    }
  }
  effect(() => {
    const current = input();
    if (sameInput(current, wanted)) return;
    if (wanted === null || !compatible(wanted, current)) {
      projection.set(null);
    }
    wanted = current;
    failure.set(null);
    schedule();
  });
  cleanup(() => {
    alive = false;
    if (timer !== undefined) clearTimeout(timer);
  });
  const blocks = computed(() => {
    const parsed = projection();
    // Only an actual parsed prefix of this exact message is retained while
    // append work catches up. Replacements and incomplete history stay literal.
    return parsed && compatible(parsed.input, input()) ? parsed.blocks : null;
  });
  return (
    <If
      when={blocks()}
      else={
        <View style={{ gap: 0 }}>
          <Text selectable style={{ color: C.text, fontSize: 14, lineHeight: 24 }}>
            {props.row().text}
          </Text>
          <If when={failure()}>
            <Text style={{ color: C.danger }}>Formatting unavailable: {failure()!.message}</Text>
          </If>
        </View>
      }
    >
      <View style={{ gap: 16 }}>
        <Each of={blocks() ?? []} key={(block: MarkdownBlock) => block.key}>
          {(block) => <MarkdownBlockView block={block} />}
        </Each>
      </View>
    </If>
  );
}

function MarkdownBlockView(props: { block: () => MarkdownBlock }) {
  const block = props.block;
  const container = () =>
    block().kind === 'code' || block().marker !== '' || block().quote > 0 || block().indent > 0;
  const inset = () =>
    Math.min((block().indent + Math.max(0, block().quote - 1)) * 16, windowSize().width / 4);
  return (
    <If when={container()} else={<MarkdownBlockContent block={block} />}>
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
          <Text style={{ color: C.muted, flexShrink: 0 }}>{block().marker}</Text>
        </If>
        <View style={{ flex: 1, minWidth: 0 }}>
          <MarkdownBlockContent block={block} />
        </View>
      </View>
    </If>
  );
}

function MarkdownBlockContent(props: { block: () => MarkdownBlock }) {
  const block = props.block;
  return (
    <If
      when={block().kind === 'code'}
      else={
        <If
          when={block().kind === 'paragraph' || block().kind === 'heading'}
          else={
            <Text selectable style={{ color: block().kind === 'rule' ? C.muted : C.text }}>
              {block().kind === 'rule' ? '───' : block().text}
            </Text>
          }
        >
          <Text
            selectable
            style={{
              color: block().quote > 0 ? C.secondary : C.text,
              fontSize: block().kind === 'heading' ? 18 : 14,
              lineHeight: 24,
              fontWeight: block().kind === 'heading' ? '700' : '400',
            }}
          >
            <Each of={block().runs} key={(run) => run.key}>
              {(run) => (
                <Text
                  style={{
                    fontWeight: run().strong ? '700' : undefined,
                    fontStyle: run().emphasis ? 'italic' : undefined,
                    textDecorationLine: run().strike ? 'line-through' : undefined,
                    color: run().code ? C.accentHover : undefined,
                    backgroundColor: run().code ? C.surface : undefined,
                  }}
                >
                  {run().text}
                </Text>
              )}
            </Each>
          </Text>
        </If>
      }
    >
      <View style={{ backgroundColor: C.surface, borderRadius: 12, padding: 16, gap: 8 }}>
        <Text style={{ color: C.muted, fontSize: 12 }}>{block().language || 'code'}</Text>
        <Scroll horizontal>
          <Text selectable style={{ color: C.text, fontFamily: 'monospace', fontSize: 13, lineHeight: 20 }}>
            {block().text}
          </Text>
        </Scroll>
      </View>
    </If>
  );
}
