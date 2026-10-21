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

import { cleanup, inputAction, signal, Text, View } from 'nsh';
import { Button, Dialog } from 'nsh/base';
import { createSettingsOperations } from './settingsOperations';
import { createModelSettings } from './settings';
import { createGeneralSettings } from './generalSettings';
import { createNetworkSettings } from './networkSettings';
import { ConnectionSettings } from './ConnectionSettings';
import { GeneralSettings } from './GeneralSettings';
import { NetworkSettings } from './NetworkSettings';
import { C } from './ui';

type Section = 'general' | 'connections' | 'network';
type BackAction = { section: Section; identity: string };
const sections: { id: Section; label: string }[] = [
  { id: 'general', label: 'General' },
  { id: 'connections', label: 'Connections & models' },
  { id: 'network', label: 'Network' },
];

// One native Dialog owns focus restoration. Forms keep their own canonical
// snapshot and unsaved edits; all writes share the original native journal.
export function ModelSettings(props: { close(): void; changed(): void }) {
  const section = signal<Section>('connections');
  const operations = createSettingsOperations();
  const connections = createModelSettings(
    props.changed,
    operations,
    () => section() === 'connections',
  );
  const general = createGeneralSettings(operations, () => section() === 'general');
  const network = createNetworkSettings(operations, () => section() === 'network');
  const pages = { connections, general, network };
  let alive = true;
  cleanup(() => {
    alive = false;
  });
  const choose = (request: { from: Section; to: Section }) => {
    if (!alive || operations.busy() || section() !== request.from || request.from === request.to)
      return;
    pages[request.from].deactivate();
    if (!operations.pending().length) {
      operations.error.set('');
      operations.notice.set('');
    }
    section.set(request.to);
    pages[request.to].activate();
  };
  const backAction = (): BackAction => ({
    section: section(),
    identity: pages[section()].navigation(),
  });
  const back = (request: BackAction) => {
    if (
      !alive ||
      operations.busy() ||
      section() !== request.section ||
      pages[request.section].navigation() !== request.identity
    )
      return;
    if (pages[request.section].back()) props.close();
  };
  const cancel = (request: BackAction) => () => back(request);
  const recover = () =>
    operations.recover((method) => {
      if (method === 'updateAssistantPreferences') general.reset();
      else if (method === 'updateNetwork') network.reset();
      else {
        connections.reset();
        props.changed();
      }
    });
  const title = () => {
    const current = section();
    return current === 'connections'
      ? {
          connections: 'Connections & models',
          providers: 'Add connection',
          connection: 'Connection settings',
          models: 'Connection models',
          model: 'Model settings',
        }[connections.page()]
      : pages[current].title();
  };
  const nested = () =>
    section() === 'connections'
      ? connections.page() !== 'connections' || !!connections.confirmation()
      : section() === 'general' && general.title() !== 'General settings';
  return (
    <Dialog
      animation="none"
      closeOnEscape={false}
      closeOnBackdropPress={false}
      closeOnSwipe={false}
      onCancel={cancel(backAction())}
      style={{
        flex: 1,
        justifyContent: 'center',
        alignItems: 'center',
        padding: 16,
        backgroundColor: '#00000099',
      }}
    >
      <View
        style={{
          width: '100%',
          maxWidth: 768,
          height: '100%',
          maxHeight: 800,
          padding: 8,
          gap: 8,
          backgroundColor: C.surface,
          borderWidth: 8,
          borderColor: C.line,
        }}
      >
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
          <Text style={{ flex: 1, color: C.accent, fontWeight: '700' }}>{title()}</Text>
          <Button
            disabled={operations.busy()}
            accessibilityLabel={
              section() === 'connections'
                ? 'Back from connection settings'
                : `Back from ${section()} settings`
            }
            onPress={inputAction(back, backAction())}
          >
            <Text style={{ color: C.muted }}>{nested() ? 'Back · Esc' : 'Close · Esc'}</Text>
          </Button>
        </View>
        <View style={{ flexDirection: 'row', gap: 16 }}>
          <Each of={sections} key={(item) => item.id}>
            {(item) => (
              <Button
                disabled={operations.busy()}
                selected={section() === item().id}
                accessibilityLabel={`Settings section ${item().label}`}
                onPress={inputAction(choose, { from: section(), to: item().id })}
              >
                <Text style={{ color: section() === item().id ? C.accent : C.muted }}>
                  {item().label}
                </Text>
              </Button>
            )}
          </Each>
        </View>
        <If when={operations.error()}>
          <Text selectable style={{ color: C.danger }}>
            {operations.error()}
          </Text>
        </If>
        <If when={operations.notice()}>
          <Text style={{ color: C.muted }}>{operations.notice()}</Text>
        </If>
        <If when={!operations.pendingKnown() || operations.pending().length > 0}>
          <View style={{ gap: 4 }}>
            <Text style={{ color: C.warning }}>
              Earlier settings actions must be resolved before another change.
            </Text>
            <Button
              disabled={operations.busy()}
              accessibilityLabel="Recover settings actions"
              onPress={inputAction(recover, null)}
            >
              <Text style={{ color: C.accent }}>
                {operations.busy() ? 'Checking receipts…' : 'Recover original settings actions'}
              </Text>
            </Button>
            <Each
              of={operations.pending().filter((item) => !item.reference)}
              key={(item) => item.id}
            >
              {(item) => (
                <Button
                  disabled={operations.busy()}
                  onPress={inputAction(operations.abandon, item().id)}
                >
                  <Text style={{ color: C.warning }}>Abandon unconfirmed {item().method}</Text>
                </Button>
              )}
            </Each>
          </View>
        </If>
        <If when={section() === 'connections'}>
          <ConnectionSettings owner={connections} />
        </If>
        <If when={section() === 'general'}>
          <GeneralSettings owner={general} />
        </If>
        <If when={section() === 'network'}>
          <NetworkSettings owner={network} />
        </If>
      </View>
    </Dialog>
  );
}
