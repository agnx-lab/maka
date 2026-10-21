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

import { inputAction, Scroll, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import type { ModelSettingsOwner } from './settings';
import { SettingsField } from './SettingsField';
import { ConnectionLogin } from './ConnectionLogin';
import { C, controlStyle, controlHoverStyle, controlFocusStyle, controlDisabledStyle } from './ui';

export function ConnectionForm(props: { settings: ModelSettingsOwner }) {
  const owner = props.settings;
  const form = () => owner.editor()!;
  const disabled = () =>
    owner.busy() || owner.loading() || !owner.pendingKnown() || owner.pending().length > 0;
  const models = () => form().baseline?.editing?.enabledModelIds ?? [];
  return (
    <Scroll style={{ flex: 1 }}>
      <View style={{ padding: 8, gap: 12 }}>
        <Text style={{ color: C.muted }}>{form().provider.descriptor.label}</Text>
        <SettingsField
          label="Connection name"
          value={form().name}
          change={(text) => owner.edit('name', text)}
          disabled={disabled()}
        />
        <SettingsField
          label="Connection ID"
          value={form().slug}
          change={(text) => owner.edit('slug', text)}
          disabled={disabled()}
          readonly={!!form().baseline}
          hint="A unique lowercase name, such as openrouter-work. It stays fixed after creation."
        />
        <If when={owner.editableBaseUrl()}>
          <SettingsField
            label="Base URL"
            value={form().baseUrl}
            change={(text) => owner.edit('baseUrl', text)}
            disabled={disabled()}
            hint="Use the API endpoint supplied by your provider."
          />
        </If>
        <Button
          disabled={disabled()}
          selected={form().enabled}
          accessibilityLabel="Enable connection"
          onPress={inputAction(owner.toggleEnabled, {
            ...owner.action(),
            enabled: !form().enabled,
          })}
          style={controlStyle}
          hoverStyle={controlHoverStyle}
          focusStyle={controlFocusStyle}
          disabledStyle={controlDisabledStyle}
        >
          <Text style={{ color: C.text }}>{form().enabled ? '✓ Enabled' : '○ Disabled'}</Text>
        </Button>
        <If
          when={form().provider.descriptor.authentication.some(
            (method) => method.id === 'api-key' && !method.interactive,
          )}
        >
          <SettingsField
            label="API key"
            value={form().apiKey}
            masked
            change={(text) => owner.edit('apiKey', text)}
            disabled={disabled()}
            hint={
              owner.credential()?.configured
                ? 'A provider credential is stored. Leave blank to keep it; enter a key to replace it.'
                : 'The key is saved in the Host credential vault and is never read back here.'
            }
          />
        </If>
        <ConnectionLogin owner={owner} disabled={disabled()} />
        <If when={owner.credential()?.configured}>
          <Button
            disabled={disabled() || owner.dirty()}
            accessibilityLabel="Remove stored provider credential"
            onPress={inputAction(owner.confirm, {
              ...owner.action(),
              kind: 'key',
              modelId: null,
            })}
            style={controlStyle}
            hoverStyle={controlHoverStyle}
            focusStyle={controlFocusStyle}
            disabledStyle={controlDisabledStyle}
          >
            <Text style={{ color: C.danger }}>Remove stored provider credential</Text>
          </Button>
          <If when={owner.dirty()}>
            <Text style={{ color: C.muted }}>
              Save or discard connection edits before removing the stored credential.
            </Text>
          </If>
        </If>
        <SettingsField
          label="Advanced request JSON"
          value={form().overlay}
          multiline
          change={(text) => owner.edit('overlay', text)}
          disabled={disabled()}
          hint={
            'Merged into model requests. For OpenRouter: {"provider":{"only":["openrouter","deepseek"]}}. Use null to clear.'
          }
        />
        <Button
          disabled={disabled()}
          accessibilityLabel="Save connection"
          onPress={inputAction(owner.saveConnection, owner.action())}
          style={{ ...controlStyle, alignSelf: 'flex-start', backgroundColor: C.accent }}
          hoverStyle={{ backgroundColor: C.accentHover }}
          focusStyle={{ backgroundColor: C.accentHover }}
          disabledStyle={controlDisabledStyle}
        >
          <Text style={{ color: C.background, fontWeight: '500' }}>
            {owner.busy() ? 'Saving…' : 'Save connection'}
          </Text>
        </Button>
        <If when={form().baseline}>
          <View style={{ height: 1, backgroundColor: C.line }} />
          <View style={{ flexDirection: 'row', gap: 16 }}>
            <Text style={{ color: C.text, flex: 1 }}>Models · {models().length}</Text>
            <Button
              disabled={disabled() || owner.dirty()}
              accessibilityLabel="Add model manually"
              onPress={inputAction(owner.editModel, { ...owner.action(), id: null })}
              style={controlStyle}
              hoverStyle={controlHoverStyle}
              focusStyle={controlFocusStyle}
              disabledStyle={controlDisabledStyle}
            >
              <Text style={{ color: C.accent }}>Add model</Text>
            </Button>
            <If when={form().provider.descriptor.discovery}>
              <Button
                disabled={disabled() || owner.dirty()}
                accessibilityLabel="Fetch models"
                onPress={inputAction(owner.discover, owner.action())}
                style={controlStyle}
                hoverStyle={controlHoverStyle}
                focusStyle={controlFocusStyle}
                disabledStyle={controlDisabledStyle}
              >
                <Text style={{ color: C.accent }}>Fetch models</Text>
              </Button>
            </If>
          </View>
          <If when={!models().length}>
            <Text style={{ color: C.muted }}>
              Fetch the provider’s catalog or add its exact model ID.
            </Text>
          </If>
          <Button
            disabled={disabled() || owner.dirty()}
            accessibilityLabel="Manage models"
            onPress={inputAction(owner.showModels, owner.action())}
            style={controlStyle}
            hoverStyle={controlHoverStyle}
            focusStyle={controlFocusStyle}
            disabledStyle={controlDisabledStyle}
          >
            <Text style={{ color: C.accent }}>Manage models · {models().length}</Text>
          </Button>
          <If when={owner.dirty()}>
            <Text style={{ color: C.muted }}>Save changes before fetching or editing models.</Text>
          </If>
          <If when={owner.defaultTarget()?.connectionId === form().baseline?.target.connectionId}>
            <Button
              disabled={disabled()}
              onPress={inputAction(owner.setDefault, { ...owner.action(), id: null })}
              style={controlStyle}
              hoverStyle={controlHoverStyle}
              focusStyle={controlFocusStyle}
              disabledStyle={controlDisabledStyle}
            >
              <Text style={{ color: C.muted }}>Clear default model</Text>
            </Button>
          </If>
          <View style={{ flexDirection: 'row', gap: 16 }}>
            <Button
              disabled={disabled()}
              onPress={inputAction(owner.openConnection, {
                connectionId: form().baseline!.target.connectionId,
                revision: form().baseline!.target.revision,
              })}
              style={controlStyle}
              hoverStyle={controlHoverStyle}
              focusStyle={controlFocusStyle}
              disabledStyle={controlDisabledStyle}
            >
              <Text style={{ color: C.muted }}>Discard changes and reload</Text>
            </Button>
            <Button
              disabled={disabled()}
              onPress={inputAction(owner.confirm, {
                ...owner.action(),
                kind: 'connection',
                modelId: null,
              })}
              style={controlStyle}
              hoverStyle={controlHoverStyle}
              focusStyle={controlFocusStyle}
              disabledStyle={controlDisabledStyle}
            >
              <Text style={{ color: C.danger }}>Delete connection</Text>
            </Button>
          </View>
        </If>
      </View>
    </Scroll>
  );
}
