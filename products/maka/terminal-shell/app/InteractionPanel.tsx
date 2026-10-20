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

import { cleanup, effect, Input, signal, Scroll, Text, View } from 'nsh';
import { host } from 'maka/host';
import { keymap } from 'nsh/keymap';
import { windowSize } from 'nsh/device';
import { Button, Dialog } from 'nsh/base';
import { C } from './ui';

type InteractionControl = (method: string, input: unknown) => Promise<any>;
type InteractionQuestion = {
  question: string;
  options: Array<{ label: string; description?: string }>;
};
const interactionBytes = (text: string) => {
  let length = 0;
  for (const point of text) {
    const code = point.codePointAt(0)!;
    length += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
  }
  return length;
};
function InteractionAction(props: {
  label: string;
  onPress: () => void;
  disabled?: boolean;
  selected?: boolean;
  primary?: boolean;
}) {
  return (
    <Button
      accessibilityLabel={props.label}
      disabled={props.disabled}
      selected={props.selected}
      onPress={props.onPress}
      style={{
        paddingHorizontal: 8,
        paddingVertical: 0,
        backgroundColor: props.primary ? C.line : C.surface,
      }}
    >
      <Text
        style={{
          fontSize: 16,
          lineHeight: 16,
          color: props.disabled ? C.muted : props.primary ? C.accent : C.text,
        }}
      >
        {props.label}
      </Text>
    </Button>
  );
}
function interactionPermissions(value: any): string {
  const paths = value.filesystem.map(
    (rule: any) =>
      `${rule.access === 'write' ? 'Read/write' : 'Read'} · ${rule.scope === 'exact' ? 'this exact path' : 'path and descendants'}\n  ${rule.path}`,
  );
  const network =
    value.network === 'allowed'
      ? 'Network: all destinations'
      : value.network === 'denied'
        ? 'Network: no additional access'
        : `Network: listed destinations only\n${value.network.restricted.destinations.map((target: any) => `  ${target.host}:${target.port}`).join('\n')}`;
  return [...(paths.length ? paths : ['Filesystem: no additional access']), network].join('\n\n');
}

export function InteractionPanel({
  sessionId,
  requestId,
  control,
  close,
}: {
  sessionId: string;
  requestId: string;
  control: InteractionControl;
  close: () => void;
}) {
  const record = signal<any>(null);
  const loading = signal(false),
    busy = signal(false),
    uncertain = signal(false);
  const pending = signal(false),
    error = signal('');
  const scope = signal<'once' | 'turn' | 'session'>('once');
  const answers = signal<Array<{ other: boolean; value: string | null | undefined }>>([]);
  const argumentsText = signal(''),
    argumentTotal = signal<number | null>(null);
  const argumentBusy = signal(false),
    argumentError = signal('');
  let argumentPosition: any = null;
  let alive = true;
  cleanup(() => {
    alive = false;
  });
  const request = () => record()?.request;
  const sameRecord = (value: any) =>
    value?.sessionId === sessionId && value?.requestId === requestId;
  const originalPending = async () =>
    (await host.pendingControls()).some(
      (original: any) =>
        original.method === 'respond' &&
        original.sessionId === sessionId &&
        original.correlation?.requestId === requestId,
    );
  const blocked = () => loading() || busy() || uncertain() || !pending() || !!record()?.outcome;
  const completeArguments = () =>
    argumentTotal() !== null && interactionBytes(argumentsText()) === argumentTotal();
  const questionReady = () =>
    request()?.kind === 'question' &&
    answers().length === request().questions.length &&
    answers().every(
      (part) =>
        part.value === null ||
        (typeof part.value === 'string' &&
          !!part.value.trim() &&
          interactionBytes(part.value) <= 2048),
    );
  const choose = (index: number, value: string | null | undefined, other = false) => {
    if (!blocked())
      answers.update((current) =>
        current.map((part, i) => (i === index ? { other, value } : part)),
      );
  };
  const readArguments = async () => {
    if (argumentBusy() || completeArguments() || request()?.kind !== 'service_approval') return;
    argumentBusy.set(true);
    argumentError.set('');
    const original = request(),
      offset = interactionBytes(argumentsText());
    try {
      const page = await host.query('agent', 'source', {
        sessionId,
        through: original.through,
        source: { kind: 'tool_use', toolUseId: original.toolUseId },
        field: 'arguments',
        position: offset === 0 ? { kind: 'start' } : { kind: 'after', offset },
      });
      if (!alive) return;
      const source = page.source,
        call = source?.tool?.call;
      const equalPosition = (a: any, b: any) =>
        !!a && !!b && a.sourceSequence === b.sourceSequence && a.itemIndex === b.itemIndex;
      const end = offset + interactionBytes(source?.text ?? '');
      if (
        page.kind !== 'ready' ||
        page.through !== original.through ||
        source?.field !== 'arguments' ||
        source.format !== 'json' ||
        source.path != null ||
        source.offset !== offset ||
        source.tool?.toolUseId !== original.toolUseId ||
        call?.name !== original.toolName ||
        !equalPosition(source.position, call.position) ||
        (argumentPosition && !equalPosition(source.position, argumentPosition)) ||
        (argumentTotal() !== null && source.totalBytes !== argumentTotal()) ||
        end > source.totalBytes ||
        (end === offset && end !== source.totalBytes)
      ) {
        throw new Error(
          'The original call arguments could not be verified. Allow remains disabled.',
        );
      }
      argumentPosition = source.position;
      argumentTotal.set(source.totalBytes);
      argumentsText.set(argumentsText() + source.text);
    } catch (reason) {
      if (alive) argumentError.set(String(reason));
    } finally {
      if (alive) {
        argumentBusy.set(false);
        if (!completeArguments() && !argumentError()) void readArguments();
      }
    }
  };
  const refresh = async () => {
    if (loading() || busy()) return;
    loading.set(true);
    error.set('');
    try {
      const [canonical, list, unresolved] = await Promise.all([
        host.query('agent', 'interaction', { sessionId, requestId }),
        host.query('agent', 'pendingInteractions', { sessionId }),
        originalPending(),
      ]);
      if (!alive) return;
      if (!sameRecord(canonical)) throw new Error('The Host returned a different request.');
      record.set(canonical);
      uncertain.set(unresolved);
      pending.set(list.entries.some((entry: any) => entry.requestId === requestId));
      if (
        canonical.request.kind === 'permissions' &&
        canonical.request.toolUseId === null &&
        scope() === 'once'
      )
        scope.set('turn');
      if (canonical.request.kind === 'question' && answers().length === 0) {
        answers.set(canonical.request.questions.map(() => ({ other: false, value: undefined })));
      }
      if (canonical.request.kind === 'service_approval' && !canonical.outcome) void readArguments();
    } catch (reason) {
      if (alive) {
        uncertain.set(true);
        error.set(String(reason));
      }
    } finally {
      if (alive) loading.set(false);
    }
  };
  const respond = async (answer: any) => {
    if (blocked() || answer.kind !== request()?.kind) return;
    busy.set(true);
    error.set('');
    try {
      const result = await control('respond', { sessionId, requestId, answer });
      if (!alive) return;
      if (result?.kind === 'recorded' && sameRecord(result.record) && result.record.outcome) {
        // This is the canonical winner, which can differ from our answer.
        record.set(result.record);
        pending.set(false);
      } else if (result?.kind === 'rejected') {
        error.set(
          `Answer was not accepted: ${result.reason}. Refresh the request before trying again.`,
        );
        pending.set(false);
      } else {
        uncertain.set(true);
        error.set(
          'Answer status is unknown. Recover its original receipt in the main view; do not submit again.',
        );
      }
    } catch (reason) {
      let unresolved = true;
      try {
        unresolved = await originalPending();
      } catch {
        /* Unknown receipt state stays blocked. */
      }
      if (alive) {
        uncertain.set(unresolved);
        error.set(
          unresolved
            ? `Answer status is unknown. Recover its original receipt in the main view. ${String(reason)}`
            : String(reason),
        );
      }
    } finally {
      if (alive) busy.set(false);
    }
  };
  const submit = () => {
    const current = request();
    if (blocked() || !current) return;
    if (current.kind === 'service_approval') {
      if (completeArguments() && !argumentBusy() && !argumentError())
        void respond({ kind: current.kind, decision: 'allow' });
    } else if (current.kind === 'permissions') {
      if (scope() === 'once' && current.toolUseId === null) return;
      void respond({
        kind: current.kind,
        decision: { decision: 'allow', scope: scope(), permissions: current.request.permissions },
      });
    } else if (current.kind === 'question') {
      const values = answers();
      if (!questionReady()) {
        error.set(
          'Answer every question or explicitly choose Skip. Other answers must fit in 2048 UTF-8 bytes.',
        );
        return;
      }
      void respond({ kind: 'question', answers: values.map((part) => part.value) });
    }
  };
  const deny = () => {
    const kind = request()?.kind;
    if (kind === 'service_approval') void respond({ kind, decision: 'deny' });
    if (kind === 'permissions') void respond({ kind, decision: { decision: 'deny' } });
    if (kind === 'form') void respond({ kind, action: 'decline' });
  };
  // Parent mounts only the current keyed panel. Native keymap ownership removes
  // these bindings on close; the Dialog owns Escape without answering a request.
  keymap.bind('mod+enter', submit, { when: 'always' });
  keymap.bind(
    'mod+r',
    () => {
      void refresh();
    },
    { when: 'always' },
  );
  keymap.bind('d', deny, { when: 'notEditing' });
  // Start once after construction, without tracking refresh's loading guards
  // as effect dependencies (which would create a request loop).
  effect(() => {
    void Promise.resolve().then(() => {
      if (alive) void refresh();
    });
  });
  const title = () =>
    ({
      service_approval: 'Review tool call',
      permissions: 'Review additional access',
      question: 'A question for you',
      form: 'Form request',
      client_capability: 'Capability request',
    })[request()?.kind as string] ?? 'Review request';
  return (
    <Dialog
      animation="none"
      onOpenChange={close}
      style={{
        flex: 1,
        alignItems: 'center',
        justifyContent: 'center',
        paddingHorizontal: 8,
        paddingVertical: 16,
        backgroundColor: '#000000bb',
      }}
    >
      <View
        style={{
          width: Math.max(8, Math.min(768, windowSize().width - 16)),
          height: Math.max(80, Math.min(640, windowSize().height - 32)),
          padding: 16,
          gap: 16,
          backgroundColor: C.background,
        }}
      >
        <View style={{ flexDirection: 'row', gap: 16 }}>
          <Text
            style={{ flex: 1, color: C.warning, fontSize: 16, lineHeight: 16, fontWeight: '700' }}
          >
            {title()}
          </Text>
          <InteractionAction label="[Esc] Close" onPress={close} />
        </View>
        <Scroll style={{ flex: 1, minHeight: 0 }} contentStyle={{ gap: 16 }}>
          <If when={!record()}>
            <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
              {loading() ? 'Reading the original request…' : 'Request not available.'}
            </Text>
          </If>
          <If
            when={record()?.outcome}
            else={
              <View style={{ gap: 16 }}>
                <If when={request()?.kind === 'service_approval'}>
                  <Text
                    selectable
                    style={{ color: C.text, fontSize: 16, lineHeight: 16 }}
                  >{`${request()?.toolName}\n${request()?.service}.${request()?.method}`}</Text>
                  <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
                    Approve this original call only. This does not grant access to future calls.
                  </Text>
                  <Text selectable style={{ color: C.text, fontSize: 16, lineHeight: 16 }}>
                    {argumentsText()}
                  </Text>
                  <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
                    {completeArguments()
                      ? 'Complete original arguments loaded.'
                      : `${interactionBytes(argumentsText())} of ${argumentTotal() ?? '?'} argument bytes loaded. Read all before allowing.`}
                  </Text>
                  <If when={argumentError()}>
                    <InteractionAction
                      label="Retry original arguments"
                      disabled={argumentBusy()}
                      onPress={() => void readArguments()}
                    />
                  </If>
                  <If when={argumentError()}>
                    <Text style={{ color: C.danger, fontSize: 16, lineHeight: 16 }}>
                      {argumentError()}
                    </Text>
                  </If>
                </If>
                <If when={request()?.kind === 'permissions'}>
                  <Text selectable style={{ color: C.text, fontSize: 16, lineHeight: 16 }}>
                    {request()?.request.reason}
                  </Text>
                  <If when={request()?.request.command}>
                    <Text
                      selectable
                      style={{ color: C.text, fontSize: 16, lineHeight: 16 }}
                    >{`$ ${request()?.request.command.command}\nIn ${request()?.request.command.cwd}`}</Text>
                  </If>
                  <Text selectable style={{ color: C.text, fontSize: 16, lineHeight: 16 }}>
                    {request()?.kind === 'permissions'
                      ? interactionPermissions(request().request.permissions)
                      : ''}
                  </Text>
                  <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
                    Allow exactly these paths and network destinations for:
                  </Text>
                  <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 16 }}>
                    <Each
                      of={
                        request()?.toolUseId === null
                          ? ['turn', 'session']
                          : ['once', 'turn', 'session']
                      }
                    >
                      {(duration) => (
                        <InteractionAction
                          label={`${scope() === duration() ? '(*)' : '( )'} ${duration() === 'once' ? 'This call' : duration() === 'turn' ? 'This turn' : 'This session'}`}
                          disabled={blocked()}
                          selected={scope() === duration()}
                          onPress={() => {
                            if (!blocked()) scope.set(duration() as 'once' | 'turn' | 'session');
                          }}
                        />
                      )}
                    </Each>
                  </View>
                </If>
                <If when={request()?.kind === 'question'}>
                  <Each<InteractionQuestion> of={request()?.questions ?? []}>
                    {(question, index) => (
                      <View style={{ gap: 16 }}>
                        <Text
                          selectable
                          style={{
                            color: C.text,
                            fontSize: 16,
                            lineHeight: 16,
                            fontWeight: '600',
                          }}
                        >{`${index() + 1}. ${question().question}`}</Text>
                        <Each of={question().options}>
                          {(option) => (
                            <Button
                              disabled={blocked()}
                              selected={answers()[index()]?.value === option().label}
                              onPress={() => choose(index(), option().label)}
                              style={{ paddingHorizontal: 8, backgroundColor: C.surface }}
                            >
                              <Text
                                style={{
                                  color:
                                    answers()[index()]?.value === option().label
                                      ? C.accent
                                      : C.text,
                                  fontSize: 16,
                                  lineHeight: 16,
                                }}
                              >{`${answers()[index()]?.value === option().label ? '(*)' : '( )'} ${option().label}`}</Text>
                              <If when={option().description}>
                                <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
                                  {option().description}
                                </Text>
                              </If>
                            </Button>
                          )}
                        </Each>
                        <View style={{ flexDirection: 'row', gap: 16 }}>
                          <InteractionAction
                            label={
                              answers()[index()]?.other ? '(*) Other answer' : '( ) Other answer'
                            }
                            disabled={blocked()}
                            selected={answers()[index()]?.other}
                            onPress={() => choose(index(), '', true)}
                          />
                          <InteractionAction
                            label={answers()[index()]?.value === null ? '(*) Skip' : '( ) Skip'}
                            disabled={blocked()}
                            selected={answers()[index()]?.value === null}
                            onPress={() => choose(index(), null)}
                          />
                        </View>
                        <If when={answers()[index()]?.other}>
                          <Input
                            value={String(answers()[index()]?.value ?? '')}
                            readonly={blocked()}
                            placeholder="Your answer"
                            onChange={(value) => choose(index(), value, true)}
                            style={{
                              placeholderColor: C.muted,
                              minHeight: 32,
                              paddingHorizontal: 8,
                              color: C.text,
                              backgroundColor: C.surface,
                              fontSize: 16,
                              lineHeight: 16,
                            }}
                          />
                        </If>
                      </View>
                    )}
                  </Each>
                </If>
                <If when={request()?.kind === 'form' || request()?.kind === 'client_capability'}>
                  <Text style={{ color: C.warning, fontSize: 16, lineHeight: 16 }}>
                    {request()?.kind === 'form'
                      ? 'Fill this form in the existing client, or explicitly Decline / Cancel the request here.'
                      : 'Review this capability in the existing client. Closing this view leaves it pending.'}
                  </Text>
                  <Text selectable style={{ color: C.text, fontSize: 16, lineHeight: 16 }}>
                    {JSON.stringify(request(), null, 2)}
                  </Text>
                </If>
              </View>
            }
          >
            <Text style={{ color: C.accent, fontSize: 16, lineHeight: 16 }}>Recorded outcome</Text>
            <Text selectable style={{ color: C.text, fontSize: 16, lineHeight: 16 }}>
              {JSON.stringify(record()?.outcome, null, 2)}
            </Text>
          </If>
          <If when={error()}>
            <Text style={{ color: C.danger, fontSize: 16, lineHeight: 16 }}>{error()}</Text>
          </If>
          <If when={uncertain() && !record()?.outcome}>
            <Text style={{ color: C.warning, fontSize: 16, lineHeight: 16 }}>
              An original answer receipt is unresolved. Refresh is read-only; further answers stay
              disabled.
            </Text>
          </If>
          <If when={request()?.kind === 'question' && !record()?.outcome && !questionReady()}>
            <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
              Choose an option, type Other, or explicitly Skip each question. Other answers allow up
              to 2048 UTF-8 bytes.
            </Text>
          </If>
          <If when={record() && !record().outcome && !pending() && !loading()}>
            <Text style={{ color: C.warning, fontSize: 16, lineHeight: 16 }}>
              This request is no longer listed as pending. Refresh to read its canonical outcome.
            </Text>
          </If>
        </Scroll>
        <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 16 }}>
          <If when={request()?.kind === 'form' && !record()?.outcome}>
            <InteractionAction label="[D] Decline" disabled={blocked()} onPress={deny} />
            <InteractionAction
              label="Cancel request"
              disabled={blocked()}
              onPress={() => {
                void respond({ kind: 'form', action: 'cancel' });
              }}
            />
          </If>
          <If
            when={
              !record()?.outcome &&
              ['service_approval', 'permissions', 'question'].includes(request()?.kind)
            }
          >
            <InteractionAction
              primary
              label={
                busy()
                  ? 'Recording answer…'
                  : request()?.kind === 'question'
                    ? '[Ctrl+Enter] Submit answers'
                    : '[Ctrl+Enter] Allow'
              }
              disabled={
                blocked() ||
                (request()?.kind === 'question' && !questionReady()) ||
                (request()?.kind === 'service_approval' &&
                  (!completeArguments() || argumentBusy() || !!argumentError()))
              }
              onPress={submit}
            />
            <If when={request()?.kind !== 'question'}>
              <InteractionAction label="[D] Deny" disabled={blocked()} onPress={deny} />
            </If>
          </If>
          <InteractionAction
            label="[Ctrl+R] Refresh status"
            disabled={loading() || busy()}
            onPress={() => {
              void refresh();
            }}
          />
        </View>
        <Text style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
          Tab to move · Enter to activate · Esc closes without answering
        </Text>
      </View>
    </Dialog>
  );
}
