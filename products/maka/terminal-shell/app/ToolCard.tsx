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

import { computed, inputAction, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import type { HistoryTool } from '../../../../packages/plugin-sdk/src/index.js';
import type { Row } from './history';
import type { ImagePreview } from './ImageThumbnail';
import { ToolImage } from './ToolImage';
import { createToolSource, OriginalToolSource, type ToolSourceRef } from './ToolSource';
import {
  cardFamily,
  processFamily,
  toolArguments,
  toolResult,
  type ToolSection,
} from './toolCards';
import { C } from './ui';

function reference(tool: HistoryTool, field: 'arguments' | 'result'): ToolSourceRef | null {
  const position = (field === 'arguments' ? tool.call : tool.result)?.position;
  return position
    ? {
        field,
        position,
        toolUseId: tool.toolUseId,
        name: tool.call?.name,
        presentation: tool.call ? tool.call.presentation : undefined,
        callPosition: tool.call?.position ?? null,
        resultPosition: tool.result?.position ?? null,
        key: JSON.stringify([
          field,
          tool.toolUseId,
          position.sourceSequence,
          position.itemIndex,
          tool.call?.presentation ?? null,
        ]),
      }
    : null;
}

function Sections(props: { items: ToolSection[] }) {
  return (
    <View style={{ gap: 8, minWidth: 0 }}>
      <Each of={props.items}>
        {(section) => (
          <View
            style={{
              gap: 0,
              paddingHorizontal: 8,
              paddingVertical: 8,
              backgroundColor: C.background,
              borderRadius: 8,
            }}
          >
            <Text style={{ color: C.secondary, fontWeight: '600' }}>{section().label}</Text>
            <Text
              selectable
              style={{
                color:
                  section().tone === 'error' || section().tone === 'removed'
                    ? C.danger
                    : section().tone === 'warning'
                      ? C.warning
                      : section().tone === 'added'
                        ? '#78d7a1'
                        : C.text,
                fontSize: 16,
                lineHeight: 16,
              }}
            >
              {section().text || '(empty)'}
            </Text>
          </View>
        )}
      </Each>
    </View>
  );
}

function ProcessOutcome(props: { reference: ToolSourceRef; sessionId: string; through: number }) {
  const status = createToolSource(props.reference, props.sessionId, props.through, [
    'value',
    'status',
  ]);
  return (
    <If
      when={status.complete() && !status.error()}
      else={<Text style={{ color: C.muted }}>Recorded</Text>}
    >
      <Each of={[status.content()]}>
        {(value) => {
          const phase = value();
          const settled = ['completed', 'failed', 'timed_out', 'cancelled'].includes(phase);
          const exit = settled
            ? createToolSource(props.reference, props.sessionId, props.through, [
                'value',
                'exitCode',
              ])
            : null;
          const code = () => exit?.json();
          const failed = () =>
            ['failed', 'timed_out', 'orphaned'].includes(phase) ||
            (typeof code() === 'number' && code() !== 0);
          const label = () => {
            if (phase === 'cancelled') return 'Stopped';
            if (phase === 'orphaned') return 'Unknown';
            if (failed()) return 'Error';
            if (phase === 'completed') return code() === 0 ? 'OK' : 'Completed';
            if (phase === 'starting') return 'Preparing';
            return phase === 'running' ? 'Running' : 'Recorded';
          };
          return (
            <Text
              style={{ color: phase === 'cancelled' ? C.warning : failed() ? C.danger : C.muted }}
            >
              {label()}
              {typeof code() === 'number' ? ` · Exit ${code()}` : ''}
            </Text>
          );
        }}
      </Each>
    </If>
  );
}

function Outcome(props: { reference: ToolSourceRef; sessionId: string; through: number }) {
  const kind = createToolSource(props.reference, props.sessionId, props.through, ['kind']);
  return (
    <If
      when={kind.complete() && !kind.error()}
      else={<Text style={{ color: C.muted }}>Recorded</Text>}
    >
      <If
        when={kind.content() === 'completed'}
        else={
          <Text style={{ color: kind.content() === 'failed' ? C.danger : C.muted }}>
            {kind.content() === 'failed' ? 'Error' : 'Recorded'}
          </Text>
        }
      >
        <If
          when={processFamily(cardFamily(props.reference.presentation))}
          else={<Text style={{ color: C.muted }}>OK</Text>}
        >
          <ProcessOutcome
            reference={props.reference}
            sessionId={props.sessionId}
            through={props.through}
          />
        </If>
      </If>
    </If>
  );
}

function Field(props: {
  reference: ToolSourceRef;
  sessionId: string;
  through: number;
  original(): boolean;
}) {
  const source = createToolSource(props.reference, props.sessionId, props.through);
  const family = cardFamily(source.original.presentation);
  const sections = computed(() =>
    props.reference.field === 'arguments'
      ? toolArguments(family, source.json())
      : toolResult(family, source.json()),
  );
  return (
    <If
      when={!props.original() && sections() !== null}
      else={
        <OriginalToolSource
          source={source}
          label={props.reference.field === 'arguments' ? 'Original arguments' : 'Original result'}
        />
      }
    >
      <Sections items={sections()!} />
    </If>
  );
}

export function ToolCard(props: {
  row(): Row;
  sessionId: string;
  ownerKey(): string;
  through(): number;
  expanded(): boolean;
  original(): boolean;
  expand(action: { sessionId: string; key: string; value: boolean }): void;
  showOriginal(action: { sessionId: string; key: string; value: boolean }): void;
  previewImage(image: ImagePreview): void;
}) {
  const tool = (): HistoryTool | null => props.row().tool ?? null;
  const family = () => cardFamily(tool()?.call?.presentation);
  const references = () =>
    tool()
      ? (['arguments', 'result'] as const).flatMap((field) => {
          const value = reference(tool()!, field);
          return value ? [value] : [];
        })
      : [];
  const result = () => (tool() ? reference(tool()!, 'result') : null);
  const fallbackState = () => {
    const value = tool();
    if (value?.result)
      return value.result.settlement === 'unknown'
        ? 'Unknown'
        : value.result.isError || value.result.settlement === 'failed'
          ? 'Error'
          : value.result.settlement === 'rejected'
            ? 'Rejected'
            : 'Recorded';
    // History references describe their fence, never current execution liveness.
    return value?.call
      ? value.call.dispatched
        ? 'Dispatched'
        : 'Input recorded'
      : 'Not completed';
  };
  return (
    <View style={{ width: '100%', minWidth: 0 }}>
      <Button
        selected={props.expanded()}
        accessibilityLabel={`Toggle tool ${tool()?.call?.name ?? 'details'}`}
        onPress={inputAction(props.expand, {
          sessionId: props.sessionId,
          key: props.ownerKey(),
          value: !props.expanded(),
        })}
        style={{ height: 16, paddingVertical: 0, flexDirection: 'row', gap: 8 }}
      >
        <Text
          numberOfLines={1}
          style={{ flex: 1, minWidth: 0, color: C.muted, fontSize: 16, lineHeight: 16 }}
        >
          {props.expanded() ? 'v' : '>'} {tool()?.call?.name ?? 'Tool'}
          {tool()?.call?.title?.fallback ? ` · ${tool()!.call!.title!.fallback}` : ''}
        </Text>
        <If
          when={
            family() &&
            result() &&
            tool()?.result?.settlement === 'returned' &&
            !tool()?.result?.isError
          }
          else={
            <Text style={{ color: tool()?.result?.isError ? C.danger : C.muted }}>
              {fallbackState()}
            </Text>
          }
        >
          <Each of={result() ? [result()!] : []} key={(value) => value.key}>
            {(value) => (
              <Outcome
                reference={value()}
                sessionId={props.sessionId}
                through={props.row().metadataThrough ?? props.through()}
              />
            )}
          </Each>
        </If>
      </Button>
      <If when={props.expanded()}>
        <View style={{ paddingLeft: 16, paddingVertical: 16, gap: 16 }}>
          <If when={tool()?.result?.image}>
            <ToolImage
              sessionId={props.sessionId}
              through={() => props.row().metadataThrough ?? props.through()}
              position={tool()!.result!.position}
              preview={props.previewImage}
            />
          </If>
          <Button
            accessibilityLabel="Toggle original tool details"
            selected={props.original()}
            onPress={inputAction(props.showOriginal, {
              sessionId: props.sessionId,
              key: props.ownerKey(),
              value: !props.original(),
            })}
            style={{ alignSelf: 'flex-start' }}
          >
            <Text style={{ color: C.accent }}>
              {props.original() ? 'Readable card' : 'Original details'}
            </Text>
          </Button>
          <Each of={references()} key={(value) => value.key}>
            {(value) => (
              <Field
                reference={value()}
                sessionId={props.sessionId}
                through={props.row().metadataThrough ?? props.through()}
                original={props.original}
              />
            )}
          </Each>
          <If when={!references().length}>
            <Text selectable style={{ color: C.muted }}>
              {props.row().text || 'Original tool source is unavailable in this history row.'}
            </Text>
          </If>
        </View>
      </If>
    </View>
  );
}
