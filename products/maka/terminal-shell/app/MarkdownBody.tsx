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

import { cleanup, computed, effect, signal, Text, View } from 'nsh';
import { markdown } from 'maka/markdown';
import type { MarkdownBlock } from 'maka/markdown';
import { hasCompletePrefix, type Row } from './history';
import { C, bodyTextStyle, secondaryTextStyle } from './ui';
import { MarkdownBlockView } from './MarkdownBlock';

type Input = { messageId: string; field: string; source: string };
type Projection = { input: Input; blocks: MarkdownBlock[] };
const sameInput = (left: Input | null, right: Input | null) =>
  left === right ||
  (left !== null &&
    right !== null &&
    left.messageId === right.messageId &&
    left.field === right.field &&
    left.source === right.source);

export function MarkdownBody(props: { row: () => Row; compact?: boolean }) {
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
          <Text
            selectable
            style={{
              ...(props.compact ? secondaryTextStyle : bodyTextStyle),
              color: props.compact ? C.muted : C.text,
            }}
          >
            {props.row().text}
          </Text>
          <If when={failure()}>
            <Text style={{ ...(props.compact ? secondaryTextStyle : {}), color: C.danger }}>
              Formatting unavailable: {failure()!.message}
            </Text>
          </If>
        </View>
      }
    >
      <View style={{ gap: props.compact ? 6 : 16 }}>
        <Each of={blocks() ?? []} key={(block: MarkdownBlock) => block.key}>
          {(block) => <MarkdownBlockView block={block} compact={props.compact} />}
        </Each>
      </View>
    </If>
  );
}
