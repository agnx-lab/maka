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

import { Input, inputAction, signal, Text, View } from 'nsh';
import type { YNode } from 'nsh';
import { Image } from 'nsh/image';
import { Button } from 'nsh/base';
import { windowSize } from 'nsh/device';
import { Checklist } from './Checklist';
import { C } from './ui';
import logo from './assets/maka.png';

export type BackendIdentity = {
  connectionId: string;
  modelId: string;
  thinkingLevel: string | null;
} | null;
export type SendAction = { owner: string; text: string; backend: BackendIdentity };

// These are the immutable session controller's original reads and actions.
// The resident composer owns no second draft or model selection.
export type ComposerSession = {
  sessionId: string;
  draft(): string;
  editable(): boolean;
  blockedReason(): string;
  backend(): BackendIdentity;
  permissions(): string;
  workspace(): string;
  empty(): boolean;
  error(): string;
  notice(): string;
  recoverable(): boolean;
  recovering(): boolean;
  unconfirmed(): string[];
  invocation(): string | null;
  stopping(): boolean;
  metrics(): string;
  changeDraft(text: string): void;
  submit(event: { text: string }): void;
  sendRendered(action: SendAction): void;
  recover(): Promise<void>;
  abandon(id: string): Promise<void>;
  stop(invocationId: string | null): Promise<void>;
};

export function Composer(props: {
  session(): ComposerSession | null;
  attach(node: YNode): void;
  commands(target: { sessionId: string | null }): void;
  models(target: { sessionId: string | null }): void;
  chooseWorkspace(): void;
}) {
  const width = signal(windowSize().width);
  const hero = () => props.session() === null || props.session()!.empty();
  const blocked = () => props.session()?.blockedReason() ?? 'Choose a workspace to begin.';
  const cannotSend = () => !!blocked() || !props.session()?.draft().trim();
  return (
    <View
      onLayout={(frame) => width.set(frame.width)}
      style={{
        flex: hero() ? 1 : 0,
        justifyContent: 'center',
        width: '100%',
        maxWidth: 800,
        alignSelf: 'center',
        paddingHorizontal: 16,
      }}
    >
      <If when={hero()}>
        <View style={{ gap: 16, paddingVertical: 16 }}>
          <View style={{ flexDirection: 'row', alignItems: 'center', gap: 16 }}>
            <If when={windowSize().height >= 336}>
              <Image src={logo} resizeMode="contain" style={{ width: 32, height: 32 }} />
            </If>
            <Text style={{ color: C.text, fontWeight: '700' }}>Make something remarkable.</Text>
          </View>
          <If when={props.session() === null}>
            <Button
              accessibilityLabel="Choose composer workspace"
              onPress={props.chooseWorkspace}
              style={{ alignSelf: 'flex-start', paddingHorizontal: 8, backgroundColor: C.surface }}
              hoverStyle={{ backgroundColor: C.raised }}
              focusStyle={{ backgroundColor: C.line }}
            >
              <Text style={{ color: C.accent }}>▸ Choose workspace ▾</Text>
            </Button>
          </If>
          <If when={props.session()?.workspace()}>
            <Text numberOfLines={1} style={{ color: C.muted }}>
              Workspace · {props.session()?.workspace().split('/').filter(Boolean).at(-1)}
            </Text>
          </If>
        </View>
      </If>
      <If when={props.session()?.error()}>
        <Text selectable style={{ color: C.danger }}>
          {props.session()?.error()}
        </Text>
      </If>
      <If when={props.session()?.notice()}>
        <Text style={{ color: C.muted }}>{props.session()?.notice()}</Text>
      </If>
      <Each of={props.session() ? [props.session()!] : []} key={(session) => session.sessionId}>
        {(session) => {
          const owner = session();
          return (
            <View>
              <Checklist sessionId={owner.sessionId} />
              <If when={owner.recoverable()}>
                <Button
                  disabled={owner.recovering()}
                  onPress={inputAction(owner.recover, null)}
                  style={{ paddingVertical: 16 }}
                >
                  <Text style={{ color: C.warning }}>
                    {owner.recovering()
                      ? 'Reading original receipts…'
                      : 'Recover original send receipt'}
                  </Text>
                </Button>
              </If>
              <Each of={owner.unconfirmed()} key={(id: string) => id}>
                {(id) => (
                  <Button
                    disabled={owner.recovering()}
                    onPress={inputAction(owner.abandon, id())}
                    style={{ paddingVertical: 16 }}
                  >
                    <Text style={{ color: C.warning }}>
                      Abandon unconfirmed send · {id().slice(0, 8)}
                    </Text>
                  </Button>
                )}
              </Each>
            </View>
          );
        }}
      </Each>
      <View
        accessibilityLabel="Conversation composer"
        style={{
          backgroundColor: C.raised,
          borderRadius: 16,
          paddingHorizontal: 16,
          paddingVertical: 16,
          gap: 16,
        }}
      >
        <Each
          of={props.session() ? [props.session()!] : [null]}
          key={(owner) => owner?.sessionId ?? 'no-session'}
        >
          {(session) => {
            const owner = session();
            // Undo, selection and queued native events belong to this session.
            // Hero/history and responsive layout never replace this editor.
            return (
              <Input
                ref={props.attach}
                testID="message"
                accessibilityLabel="Message composer"
                autoFocus
                multiline
                submitBehavior="submit"
                value={owner?.draft() ?? ''}
                readonly={!owner?.editable()}
                onChange={owner?.changeDraft}
                onSubmit={owner?.submit}
                placeholder={owner ? 'Message or run a task…' : 'Choose a workspace to begin…'}
                style={{
                  minHeight: 16,
                  maxHeight: 80,
                  color: C.text,
                  placeholderColor: C.muted,
                  fontSize: 16,
                  lineHeight: 16,
                }}
              />
            );
          }}
        </Each>
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 16 }}>
          <Button
            onPress={inputAction(props.commands, { sessionId: props.session()?.sessionId ?? null })}
            accessibilityLabel="Open commands"
            hoverStyle={{ backgroundColor: C.line }}
            focusStyle={{ backgroundColor: C.line }}
            activeStyle={{ backgroundColor: C.surface }}
          >
            <Text style={{ color: C.secondary }}>+</Text>
          </Button>
          <Text style={{ color: C.secondary }}>{props.session()?.permissions()}</Text>
          <Button
            disabled={props.session() === null}
            onPress={inputAction(props.models, { sessionId: props.session()?.sessionId ?? null })}
            style={{ flex: 1, alignItems: 'flex-end' }}
          >
            <Text
              numberOfLines={1}
              style={{ color: props.session()?.backend() ? C.secondary : C.warning }}
            >
              {props.session()?.backend()?.modelId ?? 'Choose a model'} · Ctrl+G
            </Text>
          </Button>
          <If when={props.session()?.invocation() != null}>
            <Button
              disabled={props.session()!.stopping()}
              accessibilityLabel="Stop response"
              onPress={inputAction(props.session()!.stop, props.session()!.invocation())}
              style={{ backgroundColor: C.accent, paddingHorizontal: 8, borderRadius: 8 }}
              hoverStyle={{ backgroundColor: C.accentHover }}
              focusStyle={{ backgroundColor: C.accentHover }}
              activeStyle={{ backgroundColor: C.accent }}
              disabledStyle={{ backgroundColor: C.line }}
            >
              <Text style={{ color: props.session()!.stopping() ? C.muted : C.background }}>
                {props.session()!.stopping() ? '…' : '■'}
              </Text>
            </Button>
          </If>
          <Button
            accessibilityLabel="Send message"
            disabled={cannotSend()}
            onPress={
              props.session()
                ? inputAction(props.session()!.sendRendered, {
                    owner: props.session()!.sessionId,
                    text: props.session()!.draft(),
                    backend: props.session()!.backend(),
                  })
                : undefined
            }
            style={{ backgroundColor: C.accent, paddingHorizontal: 8, borderRadius: 8 }}
            hoverStyle={{ backgroundColor: C.accentHover }}
            focusStyle={{ backgroundColor: C.accentHover }}
            activeStyle={{ backgroundColor: C.accent }}
            disabledStyle={{ backgroundColor: C.line }}
          >
            <Text style={{ color: cannotSend() ? C.muted : C.background }}>Send</Text>
          </Button>
        </View>
      </View>
      <Text style={{ color: blocked() ? C.warning : C.muted }}>
        {blocked() ||
          (width() < 720
            ? 'Enter sends · Shift+Enter newline · Ctrl+P commands'
            : 'Enter sends · Shift+Enter newline · Ctrl+P commands · Ctrl+Q exits')}
      </Text>
      <If when={props.session()?.metrics()}>
        <Text numberOfLines={1} style={{ color: C.muted }}>
          {props.session()?.metrics()}
        </Text>
      </If>
    </View>
  );
}
