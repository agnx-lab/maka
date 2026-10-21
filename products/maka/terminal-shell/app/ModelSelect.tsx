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

import { batch, cleanup, computed, Input, inputAction, Scroll, signal, Text, View } from 'nsh';
import type { YNode } from 'nsh';
import { Button, Dialog } from 'nsh/base';
import { keymap } from 'nsh/keymap';
import { List } from 'nsh/list';
import { windowSize } from 'nsh/device';
import { host } from './host';
import type { ConversationHandle } from './Conversation';
import { effortCaption, modelCaption, sameRoute } from './modelSelection';
import type { BackendIdentity, ModelBackend, ModelChoice } from './modelSelection';
import {
  C,
  compact,
  controlStyle,
  controlHoverStyle,
  controlFocusStyle,
  controlDisabledStyle,
} from './ui';

type Pane = 'root' | 'models' | 'effort';
type SelectionAction = {
  instance: number;
  sessionId: string;
  expected: BackendIdentity;
  backend: ModelBackend;
};
type ModelsPage = {
  revision: number;
  next: { query: string; connection: number; model: number } | null;
  choices: ModelChoice[];
};

export function ModelSelect(props: {
  owner: ConversationHandle;
  instance: number;
  current(): boolean;
  close(instance: number): boolean;
  select(backend: ModelBackend): Promise<void>;
  manage(): void;
}) {
  const owner = props.owner;
  const pane = signal<Pane>(owner.backend() ? 'root' : 'models');
  const query = signal('');
  const page = signal<ModelsPage | null>(null);
  const choices = () => page()?.choices ?? [];
  const selected = signal(0);
  const loading = signal(false);
  const pending = signal(false);
  const error = signal('');
  let alive = true;
  let generation = 0;
  let removeSearchKeys = () => {};
  let list: YNode | undefined;
  let focusIntent: 'model' | 'effort' | null = null;
  cleanup(() => {
    alive = false;
    generation += 1;
    removeSearchKeys();
  });

  const active = () => alive && props.current();
  const readyChoice = () => {
    const details = owner.modelDetails();
    return details.kind === 'ready' && sameRoute(owner.backend(), details.choice.selection)
      ? details.choice
      : null;
  };
  const sameBackend = (left: BackendIdentity, right: BackendIdentity) =>
    sameRoute(left, right) && left?.thinkingLevel === right?.thinkingLevel;
  const action = (backend: ModelBackend): SelectionAction => ({
    instance: props.instance,
    sessionId: owner.sessionId,
    expected: owner.backend(),
    backend,
  });
  const submit = async (request: SelectionAction) => {
    if (
      !active() ||
      pending() ||
      request.instance !== props.instance ||
      request.sessionId !== owner.sessionId ||
      !sameBackend(request.expected, owner.backend())
    )
      return;
    if (sameBackend(request.backend, request.expected)) {
      props.close(request.instance);
      return;
    }
    pending.set(true);
    error.set('');
    try {
      await props.select(request.backend);
      if (active()) props.close(request.instance);
    } catch (reason) {
      if (active()) error.set(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (alive) pending.set(false);
    }
  };
  const selectEffort = (request: SelectionAction) => {
    const choice = readyChoice();
    if (
      !choice ||
      !sameRoute(request.backend, choice.selection) ||
      (request.backend.thinkingLevel !== null &&
        !choice.thinkingLevels.includes(request.backend.thinkingLevel))
    )
      return;
    void submit(request);
  };
  const close = ({ instance, from }: { instance: number; from: Pane }) => {
    if (active() && !pending() && pane() === from) props.close(instance);
  };
  const back = () => {
    if (!active() || pending()) return;
    if (pane() === 'root') props.close(props.instance);
    else {
      focusIntent = pane() === 'models' ? 'model' : 'effort';
      pane.set('root');
      error.set('');
    }
  };
  const changePane = ({ instance, to, from }: { instance: number; to: Pane; from: Pane }) => {
    if (!active() || pending() || instance !== props.instance || pane() !== from) return;
    if (to === 'effort' && !readyChoice()) return;
    if (to === 'root') focusIntent = from === 'models' ? 'model' : 'effort';
    error.set('');
    pane.set(to);
    if (to === 'models') void loadModels(true);
  };
  const loadModels = async (restart = false) => {
    if (!active() || (!restart && (loading() || !page()?.next))) return;
    const reading = ++generation;
    const search = query();
    let minimum = restart ? count() : choices().length + count();
    let changed = false;
    if (restart) {
      page.set(null);
      selected.set(0);
    }
    loading.set(true);
    error.set('');
    try {
      do {
        const previousPage = page();
        const result = await host.query(
          'settings',
          'models',
          previousPage?.next
            ? { kind: 'continue', revision: previousPage.revision, cursor: previousPage.next }
            : { kind: 'start', query: search },
        );
        if (!active() || reading !== generation) return;
        if (result.kind === 'changed') {
          if (changed)
            throw new Error('The model catalog keeps changing. Retry to read the current choices.');
          changed = true;
          minimum = count();
          page.set(null);
          selected.set(0);
          continue;
        }
        page.set({
          revision: result.revision,
          next: result.next,
          choices: [...(previousPage?.choices ?? []), ...result.choices],
        });
        if (!result.next || choices().length >= minimum) break;
      } while (active() && reading === generation);
    } catch (reason) {
      if (active() && reading === generation)
        error.set(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (alive && reading === generation) loading.set(false);
    }
  };
  const retry = ({ instance, from }: { instance: number; from: Pane }) => {
    if (!active() || pending() || instance !== props.instance || pane() !== from) return;
    if (from === 'models') void loadModels(true);
    else void owner.refreshModel();
  };
  const index = () => Math.min(selected(), Math.max(0, choices().length - 1));
  const count = () => Math.max(1, Math.min(8, Math.floor((windowSize().height / 16 - 14) / 3)));
  const routeId = (choice: ModelChoice) =>
    JSON.stringify([choice.selection.connectionId, choice.selection.modelId]);
  const byId = computed(() => new Map(choices().map((choice) => [routeId(choice), choice])));
  const ids = computed(() => choices().map(routeId));
  const modelAction = (choice: ModelChoice) =>
    action({
      connectionId: choice.selection.connectionId,
      modelId: choice.selection.modelId,
      thinkingLevel: sameRoute(owner.backend(), choice.selection)
        ? (owner.backend()?.thinkingLevel ?? null)
        : null,
    });
  const move = (delta: number) => {
    const next = Math.max(0, Math.min(choices().length - 1, index() + delta));
    selected.set(next);
    list?.scrollToIndex({ index: next, animated: false, viewPosition: delta > 0 ? 1 : 0 });
    if (next >= choices().length - 2) void loadModels();
  };
  const activate = () => {
    const choice = choices()[index()];
    if (choice) void submit(modelAction(choice));
  };
  const focusRoot = (which: 'model' | 'effort', node: YNode) => {
    if (focusIntent !== which) return;
    focusIntent = null;
    node.focus();
  };
  void owner.refreshModel();
  if (pane() === 'models') void loadModels(true);

  return (
    <Dialog
      animation="none"
      closeOnEscape={false}
      closeOnBackdropPress={false}
      closeOnSwipe={false}
      onCancel={back}
      style={{
        flex: 1,
        justifyContent: 'center',
        alignItems: 'center',
        backgroundColor: '#00000099',
        padding: 16,
      }}
    >
      <View
        style={{
          width: '100%',
          maxWidth: 640,
          backgroundColor: C.surface,
          padding: 16,
          gap: 16,
          borderWidth: 1,
          borderRadius: 16,
          borderColor: C.line,
        }}
      >
        <View style={{ flexDirection: 'row', justifyContent: 'space-between', gap: 8 }}>
          <Text style={{ color: C.text, fontSize: 18, lineHeight: 24, fontWeight: '600' }}>
            Models
          </Text>
          <Button
            disabled={pending()}
            accessibilityLabel={
              pane() === 'root' ? 'Close model selection' : 'Back to model settings'
            }
            onPress={
              pane() === 'root'
                ? inputAction(close, { instance: props.instance, from: pane() })
                : inputAction(changePane, { instance: props.instance, from: pane(), to: 'root' })
            }
            style={controlStyle}
            hoverStyle={controlHoverStyle}
            focusStyle={controlFocusStyle}
            disabledStyle={controlDisabledStyle}
          >
            <Text style={{ color: C.muted }}>{pane() === 'root' ? 'Close' : 'Back'}</Text>
          </Button>
        </View>
        <If when={pane() === 'root'}>
          <Button
            ref={(node) => focusRoot('model', node)}
            disabled={pending()}
            accessibilityLabel="Choose model"
            onPress={inputAction(changePane, {
              instance: props.instance,
              from: 'root',
              to: 'models',
            })}
            style={{ ...controlStyle, backgroundColor: C.background, gap: 8 }}
            hoverStyle={{ backgroundColor: C.raised }}
            focusStyle={{ backgroundColor: C.raised }}
            disabledStyle={controlDisabledStyle}
          >
            <Text numberOfLines={1} style={{ color: C.text }}>
              Model · {modelCaption(owner.backend(), owner.modelDetails())} ▸
            </Text>
            <Text numberOfLines={1} style={{ color: C.muted }}>
              {readyChoice()?.connectionName ??
                owner.backend()?.connectionId ??
                'Choose a connection and model'}
            </Text>
          </Button>
          <Button
            ref={(node) => focusRoot('effort', node)}
            disabled={pending() || !readyChoice()}
            accessibilityLabel="Choose reasoning effort"
            onPress={inputAction(changePane, {
              instance: props.instance,
              from: 'root',
              to: 'effort',
            })}
            style={{ ...controlStyle, backgroundColor: C.background, gap: 8 }}
            hoverStyle={{ backgroundColor: C.raised }}
            focusStyle={{ backgroundColor: C.raised }}
            disabledStyle={controlDisabledStyle}
          >
            <Text numberOfLines={1} style={{ color: C.text }}>
              Effort · {effortCaption(owner.backend(), owner.modelDetails())} ▸
            </Text>
            <Text style={{ color: C.muted }}>Default inherits this connection’s policy</Text>
          </Button>
        </If>
        <If when={pane() === 'models'}>
          <Input
            ref={(node) => {
              removeSearchKeys();
              const unbind = [
                keymap.bind('up', () => move(-1), { when: 'always', repeat: true, within: node }),
                keymap.bind('down', () => move(1), { when: 'always', repeat: true, within: node }),
                keymap.bind('enter', activate, { when: 'always', within: node }),
              ];
              removeSearchKeys = () => unbind.forEach((remove) => remove());
            }}
            autoFocus
            accessibilityLabel="Search models"
            value={query()}
            readonly={pending()}
            onChange={(text) => {
              batch(() => {
                query.set(text);
                selected.set(0);
              });
              list?.scrollToIndex({ index: 0, animated: false });
              void loadModels(true);
            }}
            placeholder="Search models…"
            maxLength={256}
            style={{
              placeholderColor: C.muted,
              color: C.text,
              height: compact() ? 32 : 40,
              backgroundColor: C.background,
              borderRadius: 8,
              paddingHorizontal: 12,
              fontSize: 14,
            }}
          />
          <List
            each={ids()}
            ref={(node) => {
              list = node;
            }}
            estimatedItemSize={48}
            onLoadMore={() => {
              if (!error()) void loadModels();
            }}
            style={{ height: count() * 48 }}
          >
            {(id) => (
              <Each<string> of={[id()]} key={(key: string) => key}>
                {(key) => {
                  const initial = byId().get(key());
                  if (!initial) return <View />;
                  const choice = () => byId().get(key()) ?? initial;
                  return (
                    <Button
                      accessibilityLabel={`Use ${choice().label} · ${choice().connectionName}`}
                      disabled={pending()}
                      selected={sameRoute(owner.backend(), choice().selection)}
                      onPress={inputAction(submit, modelAction(choice()))}
                      style={{
                        height: 48,
                        paddingHorizontal: 8,
                        backgroundColor:
                          choices()[index()] && routeId(choices()[index()]) === key()
                            ? C.line
                            : C.surface,
                      }}
                      hoverStyle={{ backgroundColor: C.raised }}
                      focusStyle={{ backgroundColor: C.raised }}
                      disabledStyle={controlDisabledStyle}
                    >
                      <Text numberOfLines={1} style={{ color: C.text }}>
                        {choice().label}
                        {sameRoute(owner.backend(), choice().selection) ? ' ✓' : ''}
                      </Text>
                      <Text numberOfLines={1} style={{ color: C.muted }}>
                        {choice().connectionName} · {choice().selection.modelId}
                      </Text>
                    </Button>
                  );
                }}
              </Each>
            )}
          </List>
          <Text style={{ color: C.muted }}>
            {loading()
              ? 'Loading…'
              : `${choices().length} model${choices().length === 1 ? '' : 's'}${page()?.next ? ' · scroll for more' : ''}`}
          </Text>
        </If>
        <If when={pane() === 'effort'}>
          <Scroll style={{ maxHeight: count() * 48 }}>
            <Each<ModelChoice>
              of={readyChoice() ? [readyChoice()!] : []}
              key={(choice: ModelChoice) => routeId(choice)}
            >
              {(item) => (
                <View style={{ gap: 8 }}>
                  <Each<string | null>
                    of={[null, ...item().thinkingLevels]}
                    key={(level: string | null) => level ?? 'default'}
                  >
                    {(level) => (
                      <Button
                        disabled={pending()}
                        accessibilityLabel={
                          level() === null ? 'Use connection default' : `Use effort ${level()}`
                        }
                        selected={owner.backend()?.thinkingLevel === level()}
                        onPress={inputAction(
                          selectEffort,
                          action({
                            connectionId: item().selection.connectionId,
                            modelId: item().selection.modelId,
                            thinkingLevel: level(),
                          }),
                        )}
                        style={{
                          minHeight: 32,
                          backgroundColor: C.background,
                          paddingHorizontal: 8,
                        }}
                        hoverStyle={{ backgroundColor: C.raised }}
                        focusStyle={{ backgroundColor: C.raised }}
                        disabledStyle={controlDisabledStyle}
                      >
                        <Text style={{ color: C.text }}>
                          {owner.backend()?.thinkingLevel === level() ? '✓ ' : '  '}
                          {level() === null ? 'Default · Use connection default' : level()}
                        </Text>
                      </Button>
                    )}
                  </Each>
                </View>
              )}
            </Each>
          </Scroll>
        </If>
        <If when={pane() !== 'models' && owner.modelDetails().kind !== 'ready'}>
          <Text style={{ color: C.muted }}>
            {owner.modelDetails().kind === 'loading'
              ? 'Loading model policy…'
              : 'The saved model policy is unavailable. Its selection is preserved.'}
          </Text>
          <Button
            disabled={pending()}
            onPress={inputAction(retry, { instance: props.instance, from: pane() })}
            accessibilityLabel="Retry model policy"
            style={controlStyle}
            hoverStyle={controlHoverStyle}
            focusStyle={controlFocusStyle}
            disabledStyle={controlDisabledStyle}
          >
            <Text style={{ color: C.accent }}>Retry</Text>
          </Button>
        </If>
        <Button
          disabled={pending()}
          accessibilityLabel="Manage connections and models"
          onPress={inputAction(props.manage, null)}
          style={controlStyle}
          hoverStyle={controlHoverStyle}
          focusStyle={controlFocusStyle}
          disabledStyle={controlDisabledStyle}
        >
          <Text style={{ color: C.accent }}>Manage connections & models</Text>
        </Button>
        <If when={error()}>
          <Text style={{ color: C.danger }}>{error()}</Text>
          <If when={pane() === 'models'}>
            <Button
              disabled={pending()}
              onPress={inputAction(retry, { instance: props.instance, from: pane() })}
              accessibilityLabel="Retry model catalog"
              style={controlStyle}
              hoverStyle={controlHoverStyle}
              focusStyle={controlFocusStyle}
              disabledStyle={controlDisabledStyle}
            >
              <Text style={{ color: C.accent }}>Retry</Text>
            </Button>
          </If>
        </If>
        <If when={pending()}>
          <Text style={{ color: C.muted }}>Saving selection…</Text>
        </If>
      </View>
    </Dialog>
  );
}
