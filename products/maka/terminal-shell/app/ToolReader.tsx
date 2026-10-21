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
import type { ToolSource } from './ToolSource';
import type { ToolChange, ToolProcess, ToolProjection, ToolRead, ToolSection } from './toolCards';
import { C, bodyTextStyle, captionTextStyle, codeTextStyle, controlStyle } from './ui';
import { Icon } from './Icon';

function Copy(props: { text(): string; label: string; source: ToolSource }) {
  const feedback = signal<{ text: string; error: string } | null>(null);
  let alive = true;
  let pending = false;
  cleanup(() => {
    alive = false;
  });
  const action = computed(() => {
    const originalText = props.text();
    const owns = () =>
      alive && props.source.complete() && !props.source.error() && props.text() === originalText;
    return inputAction(async () => {
      if (!owns() || pending) return;
      pending = true;
      try {
        await Clipboard.writeText(originalText);
        if (owns()) feedback.set({ text: originalText, error: '' });
      } catch (reason) {
        if (owns())
          feedback.set({
            text: originalText,
            error: reason instanceof Error ? reason.message : String(reason),
          });
      } finally {
        pending = false;
      }
    }, null);
  });
  const current = () => (feedback()?.text === props.text() ? feedback() : null);
  const copied = () => current() !== null && !current()!.error;
  return (
    <View style={{ alignItems: 'flex-end', minWidth: 32, flexShrink: 1 }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
        <Text style={{ ...captionTextStyle, color: C.muted }}>{copied() ? 'Copied' : ''}</Text>
        <Button
          accessibilityLabel={props.label}
          tooltip={copied() ? 'Copied' : props.label}
          onPress={action()}
          style={{ ...controlStyle, width: 32, paddingHorizontal: 8, alignItems: 'center' }}
          hoverStyle={{ backgroundColor: C.line }}
          focusStyle={{ backgroundColor: C.raised }}
        >
          <Icon name={copied() ? 'check' : 'copy'} color={C.secondary} />
        </Button>
      </View>
      <If when={current()?.error}>
        <Text selectable style={{ ...captionTextStyle, color: C.danger }}>
          {current()!.error}
        </Text>
      </If>
    </View>
  );
}

function ToolText(props: {
  label: string;
  text: string;
  copyLabel: string;
  source: ToolSource;
  gutter?: string;
  tone?: 'added' | 'removed' | 'warning';
}) {
  const color =
    props.tone === 'removed'
      ? C.danger
      : props.tone === 'added'
        ? '#78d7a1'
        : props.tone === 'warning'
          ? C.warning
          : C.text;
  return (
    <View style={{ minWidth: 0, borderRadius: 8, backgroundColor: C.background }}>
      <View
        style={{
          flexDirection: 'row',
          alignItems: 'center',
          gap: 8,
          paddingHorizontal: 12,
          paddingVertical: 4,
        }}
      >
        <Text style={{ ...captionTextStyle, color: C.secondary, flex: 1, minWidth: 0 }}>
          {props.label}
        </Text>
        <Copy label={props.copyLabel} text={() => props.text} source={props.source} />
      </View>
      <Scroll horizontal contentStyle={{ paddingHorizontal: 12, paddingBottom: 12 }}>
        <View style={{ flexDirection: 'row', alignItems: 'flex-start', gap: 12 }}>
          <If when={props.gutter !== undefined}>
            <Text
              pointerEvents="none"
              accessibilityElementsHidden
              selectable={false}
              style={{ ...codeTextStyle, color: C.muted, textAlign: 'right' }}
            >
              {props.gutter}
            </Text>
            <View
              pointerEvents="none"
              style={{ width: 1, alignSelf: 'stretch', backgroundColor: C.line }}
            />
          </If>
          <Text selectable style={{ ...codeTextStyle, color }}>
            {props.text}
          </Text>
        </View>
      </Scroll>
      <If when={props.text === ''}>
        <Text
          style={{ ...captionTextStyle, color: C.muted, paddingHorizontal: 12, paddingBottom: 12 }}
        >
          Empty recorded text.
        </Text>
      </If>
    </View>
  );
}

function Read(props: { value: ToolRead; source: ToolSource }) {
  const value = props.value;
  const lines = value.content.split('\n');
  if (lines.at(-1) === '') lines.pop();
  const gutter = lines.map((_, index) => String(value.offset + index + 1)).join('\n');
  const label = value.partialLine
    ? `Read lines · partial line · ${value.totalLines} total`
    : `Read lines ${value.returnedLines ? value.offset + 1 : value.offset}–${value.offset + value.returnedLines} / ${value.totalLines}`;
  return (
    <View style={{ gap: 8 }}>
      <ToolText
        label={label}
        text={value.content}
        gutter={gutter}
        copyLabel="Copy read contents"
        source={props.source}
      />
      <If when={value.partialLine}>
        <Text style={{ ...captionTextStyle, color: C.warning }}>
          This original result contains part of a line.
        </Text>
      </If>
      <If when={value.hasMore}>
        <Text style={{ ...captionTextStyle, color: C.muted }}>
          The tool read a range of this file; this card preserves that original range.
        </Text>
      </If>
    </View>
  );
}

function Process(props: { value: ToolProcess; source: ToolSource }) {
  const value = props.value;
  const label = {
    starting: 'Preparing at capture',
    running: 'Running at capture',
    completed: 'Completed',
    failed: 'Failed',
    timed_out: 'Timed out',
    cancelled: 'Stopped',
    orphaned: 'Outcome unknown',
  }[value.status];
  const failed =
    value.status !== 'cancelled' &&
    (['failed', 'timed_out', 'orphaned'].includes(value.status) ||
      (value.exitCode !== null && value.exitCode !== 0));
  const output = value.output;
  return (
    <View style={{ gap: 8 }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
        <Icon name="terminal" color={failed ? C.danger : C.secondary} />
        <Text
          style={{
            ...captionTextStyle,
            color: failed ? C.danger : value.status === 'cancelled' ? C.warning : C.secondary,
          }}
        >
          {label}
          {value.exitCode === null ? '' : ` · Exit ${value.exitCode}`}
        </Text>
      </View>
      <ToolText
        label="Recorded command"
        text={value.command}
        copyLabel="Copy recorded command"
        source={props.source}
      />
      <Text selectable style={{ ...captionTextStyle, color: C.muted }}>
        Working directory · {value.cwd}
      </Text>
      <If when={value.failure !== null}>
        <Text selectable style={{ ...bodyTextStyle, color: C.danger }}>
          {value.failure}
        </Text>
      </If>
      <If when={output === null}>
        <Text style={{ ...captionTextStyle, color: C.muted }}>
          No output was included in this original result.
        </Text>
      </If>
      {output?.mode === 'pipes' ? (
        <View style={{ gap: 8 }}>
          <ToolText
            label="stdout"
            text={output.stdout}
            copyLabel="Copy stdout"
            source={props.source}
          />
          <ToolText
            label="stderr"
            text={output.stderr}
            copyLabel="Copy stderr"
            source={props.source}
            tone="warning"
          />
          <If when={output.stdoutTruncated || output.stderrTruncated}>
            <Text style={{ ...captionTextStyle, color: C.warning }}>
              The recorded process output is truncated.
            </Text>
          </If>
        </View>
      ) : output?.mode === 'pty' ? (
        <View style={{ gap: 8 }}>
          <ToolText
            label="Terminal scrollback"
            text={output.scrollback}
            copyLabel="Copy terminal scrollback"
            source={props.source}
          />
          <ToolText
            label="Terminal screen"
            text={output.screen}
            copyLabel="Copy terminal screen"
            source={props.source}
          />
          <If when={output.lastAlternateScreen !== null}>
            <ToolText
              label="Last alternate screen"
              text={output.lastAlternateScreen!}
              copyLabel="Copy last alternate screen"
              source={props.source}
            />
          </If>
          <If when={output.truncated}>
            <Text style={{ ...captionTextStyle, color: C.warning }}>
              The recorded terminal output is truncated.
            </Text>
          </If>
        </View>
      ) : null}
      <If when={output?.redacted}>
        <Text style={{ ...captionTextStyle, color: C.warning }}>
          The Host redacted parts of this recorded output.
        </Text>
      </If>
    </View>
  );
}

function Change(props: { value: ToolChange; source: ToolSource }) {
  const value = props.value;
  if (value.phase === 'requested')
    return (
      <View style={{ gap: 8 }}>
        <Text style={{ ...captionTextStyle, color: C.muted }}>File</Text>
        <Text selectable style={{ ...bodyTextStyle, color: C.secondary }}>
          {value.path}
        </Text>
        {value.operation === 'write' ? (
          <ToolText
            label="Requested file contents"
            text={value.content}
            copyLabel="Copy requested file contents"
            tone="added"
            source={props.source}
          />
        ) : value.operation === 'edit' ? (
          <View style={{ gap: 8 }}>
            <ToolText
              label="Requested removal"
              text={value.before}
              copyLabel="Copy requested removal"
              tone="removed"
              source={props.source}
            />
            <ToolText
              label="Requested replacement"
              text={value.after}
              copyLabel="Copy requested replacement"
              tone="added"
              source={props.source}
            />
          </View>
        ) : (
          <View style={{ gap: 8 }}>
            <Text style={{ ...captionTextStyle, color: C.muted }}>
              Requested operation · {value.type}
            </Text>
            <If when={value.diff !== null}>
              <ToolText
                label="Requested patch"
                text={value.diff!}
                copyLabel="Copy requested patch"
                source={props.source}
              />
            </If>
          </View>
        )}
      </View>
    );
  if (value.operation === 'patch')
    return (
      <Text style={{ ...captionTextStyle, color: C.secondary }}>
        Recorded patch result · Operation completed. The patch below is the original request.
      </Text>
    );
  return (
    <View style={{ gap: 8 }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
        <Icon name="check" color={C.accent} />
        <Text style={{ ...captionTextStyle, color: C.secondary }}>
          {value.operation === 'write' ? 'Recorded write' : 'Recorded edit'}
        </Text>
      </View>
      <Text selectable style={{ ...bodyTextStyle, color: C.secondary }}>
        {value.path}
      </Text>
      {value.operation === 'write' ? (
        <View style={{ gap: 8 }}>
          <Text style={{ ...captionTextStyle, color: C.muted }}>{value.bytes} bytes</Text>
          <If
            when={value.previous !== null}
            else={
              <Text style={{ ...captionTextStyle, color: C.muted }}>
                Previous contents were not included in this original result.
              </Text>
            }
          >
            <ToolText
              label="Recorded previous contents"
              text={value.previous!}
              copyLabel="Copy recorded previous contents"
              source={props.source}
              tone="removed"
            />
          </If>
        </View>
      ) : (
        <View style={{ gap: 4 }}>
          <Text style={{ ...captionTextStyle, color: C.muted }}>
            {value.replacements} replacements
          </Text>
          <If when={value.startLine !== null && value.endLine !== null}>
            <Text style={{ ...captionTextStyle, color: C.muted }}>
              Recorded lines {value.startLine}–{value.endLine}
            </Text>
          </If>
          <If when={value.matchedVia !== null}>
            <Text style={{ ...captionTextStyle, color: C.muted }}>
              Match method · {value.matchedVia}
            </Text>
          </If>
        </View>
      )}
    </View>
  );
}

function Sections(props: { items: ToolSection[] }) {
  return (
    <View style={{ gap: 8, minWidth: 0 }}>
      <Each of={props.items} key="index">
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
                ...codeTextStyle,
                color:
                  section().tone === 'error' || section().tone === 'removed'
                    ? C.danger
                    : section().tone === 'warning'
                      ? C.warning
                      : section().tone === 'added'
                        ? '#78d7a1'
                        : C.text,
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

export function ToolReader(props: { value: ToolProjection; source: ToolSource }) {
  const value = props.value;
  switch (value.kind) {
    case 'read':
      return <Read value={value.value} source={props.source} />;
    case 'process':
      return <Process value={value.value} source={props.source} />;
    case 'change':
      return <Change value={value.value} source={props.source} />;
    case 'sections':
      return <Sections items={value.sections} />;
    case 'failure':
      return (
        <View style={{ flexDirection: 'row', alignItems: 'flex-start', gap: 8 }}>
          <Icon name="close" color={C.danger} />
          <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
            <Text style={{ ...captionTextStyle, color: C.danger }}>Tool failed</Text>
            <Text selectable style={{ ...bodyTextStyle, color: C.danger }}>
              {value.message}
            </Text>
          </View>
        </View>
      );
  }
}
