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

import { Input, inputAction, Scroll, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import type { SettingValue } from '../../../../packages/plugin-sdk/src/index.js';
import { sameValue } from '../../../../plugins/shared/value';
import { pluginTitle, settingCaption } from './pluginSettings';
import type { PluginSettingsOwner, PluginAction } from './pluginSettings';
import { SettingsField } from './SettingsField';
import { C } from './ui';

export function PluginSettings(props: { owner: PluginSettingsOwner }) {
  const owner = props.owner;
  return (
    <View style={{ flex: 1, minHeight: 0, gap: 8 }}>
      <Button
        disabled={owner.loading() || owner.busy()}
        accessibilityLabel="Refresh plugin configuration"
        onPress={inputAction(owner.reload, owner.action())}
      >
        <Text style={{ color: C.accent }}>
          {owner.editor() ? 'Discard edit and refresh plugins' : 'Refresh plugins'}
        </Text>
      </Button>
      <If when={owner.loading()}>
        <Text style={{ color: C.muted }}>Reading Host plugins…</Text>
      </If>
      <If when={!owner.loading() && !owner.inventory()}>
        <Text style={{ color: C.warning }}>
          Plugin information is unavailable. This Host must expose plugin management.
        </Text>
      </If>
      <If when={owner.inventory()}>
        <Each of={[owner.navigation()]} key={(identity) => identity}>
          {() => {
            const request = owner.action();
            return (
              <Scroll style={{ flex: 1 }}>
                <View style={{ padding: 8, gap: 12 }}>
                  <If when={!owner.selected()}>
                    <Text style={{ color: C.muted }}>
                      Plugins on the connected Host. Changing them keeps this conversation and its
                      draft.
                    </Text>
                    <Input
                      accessibilityLabel="Search Host plugins"
                      value={owner.query()}
                      onChange={(value) => owner.query.set(value)}
                      placeholder="Search plugins…"
                      style={{
                        color: C.text,
                        backgroundColor: C.background,
                        height: 32,
                        paddingHorizontal: 8,
                      }}
                    />
                    <Each
                      of={owner.filtered()}
                      key={(instance) => JSON.stringify(instance.instance)}
                    >
                      {(instance) => (
                        <Button
                          disabled={owner.disabled()}
                          accessibilityLabel={`Open plugin ${instance().path.join(' / ')}`}
                          onPress={inputAction(owner.open, {
                            request,
                            instance: instance().instance,
                          })}
                          style={{ padding: 8, backgroundColor: C.raised }}
                        >
                          <Text style={{ color: C.text }}>{pluginTitle(instance())}</Text>
                          <Text style={{ color: C.muted }}>
                            {instance().path.join(' / ')} ·{' '}
                            {instance().disabled ? 'Disabled' : 'Enabled'}
                            {instance().parentDisabled ? ' · Parent disabled' : ''}
                          </Text>
                        </Button>
                      )}
                    </Each>
                    <If when={!owner.filtered().length}>
                      <Text style={{ color: C.muted }}>No matching Host plugins.</Text>
                    </If>
                  </If>
                  <If when={owner.selected()}>
                    <Text selectable style={{ color: C.text }}>
                      {owner.selected()!.path.join(' / ')}
                    </Text>
                    <Text selectable style={{ color: C.muted }}>
                      {owner.selected()!.package.id} · {owner.selected()!.package.revision}
                    </Text>
                    <Text numberOfLines={3} style={{ color: C.muted }}>
                      {owner.selected()!.presentation?.description}
                    </Text>
                    <Text style={{ color: C.muted }}>
                      Desired configuration · revision {owner.inventory()!.source.revision}. Enabled
                      describes configuration; services may still be reconciling.
                    </Text>
                    <If when={!owner.editor()}>
                      <Button
                        disabled={
                          owner.disabled() ||
                          (owner.selected()!.disabled && owner.selected()!.parentDisabled)
                        }
                        accessibilityLabel={
                          owner.selected()!.disabled ? 'Enable Host plugin' : 'Disable Host plugin'
                        }
                        onPress={inputAction(owner.toggle, {
                          request,
                          enabled: owner.selected()!.disabled,
                        })}
                      >
                        <Text style={{ color: owner.selected()!.disabled ? C.accent : C.warning }}>
                          {owner.selected()!.disabled ? 'Enable plugin' : 'Disable plugin'}
                        </Text>
                      </Button>
                      <If when={owner.selected()!.parentDisabled}>
                        <Text style={{ color: C.warning }}>
                          Enable the parent plugin before enabling this child.
                        </Text>
                      </If>
                      <If when={owner.selected()!.structural}>
                        <Text style={{ color: C.muted }}>
                          This is a plugin group. Open an executable child to edit its settings.
                        </Text>
                      </If>
                      <If when={owner.fields()}>
                        <Text style={{ color: C.accent }}>
                          Configuration
                          {owner.fields()!.path.length
                            ? ` / ${owner.fields()!.path.join(' / ')}`
                            : ''}
                        </Text>
                        <Each of={owner.fields()!.fields} key={(field) => field.key}>
                          {(field) => {
                            const group = () => ['object', 'array'].includes(field().kind);
                            return (
                              <Button
                                disabled={
                                  owner.disabled() ||
                                  !field().editable ||
                                  (group() && request.path.length >= 16)
                                }
                                accessibilityLabel={`${group() ? 'Open plugin group' : 'Edit plugin setting'} ${field().label}`}
                                onPress={
                                  group()
                                    ? inputAction(owner.group, { request, key: field().key })
                                    : inputAction(owner.edit, { request, key: field().key })
                                }
                                style={{ padding: 8, backgroundColor: C.raised }}
                              >
                                <Text style={{ color: C.text }}>
                                  {field().label}
                                  {group() ? ' ›' : ''}
                                </Text>
                                <Text numberOfLines={2} style={{ color: C.muted }}>
                                  {group()
                                    ? field().kind
                                    : field().editable
                                      ? settingCaption(field().value)
                                      : 'Read only or private'}
                                </Text>
                              </Button>
                            );
                          }}
                        </Each>
                        <If when={!owner.fields()!.fields.length}>
                          <Text style={{ color: C.muted }}>No editable fields in this group.</Text>
                        </If>
                      </If>
                    </If>
                    <If when={owner.editor()}>
                      <PluginField owner={owner} request={request} />
                    </If>
                  </If>
                </View>
              </Scroll>
            );
          }}
        </Each>
      </If>
    </View>
  );
}

function PluginField(props: { owner: PluginSettingsOwner; request: PluginAction }) {
  const owner = props.owner,
    request = props.request;
  const field = owner.editor()!.field;
  const value = () => owner.editor()?.value ?? null;
  const change = (next: SettingValue | null) => owner.change({ request, value: next });
  return (
    <View style={{ gap: 12 }}>
      <Text style={{ color: C.text, fontWeight: '700' }}>{field.label}</Text>
      <If when={field.description}>
        <Text style={{ color: C.muted }}>{field.description}</Text>
      </If>
      <If when={field.options.length > 0}>
        <Each of={field.options} key={(choice) => JSON.stringify(choice)}>
          {(choice) => (
            <Button
              disabled={owner.disabled()}
              selected={sameValue(value(), choice())}
              accessibilityLabel={`Choose plugin setting ${settingCaption(choice())}`}
              onPress={inputAction(owner.change, { request, value: choice() })}
            >
              <Text style={{ color: C.text }}>{settingCaption(choice())}</Text>
            </Button>
          )}
        </Each>
      </If>
      <If when={field.options.length === 0 && (field.kind === 'text' || field.kind === 'number')}>
        <SettingsField
          label={`Plugin value ${field.label}`}
          value={
            value()?.kind === 'text' || value()?.kind === 'number'
              ? (value() as { text: string }).text
              : ''
          }
          disabled={owner.disabled()}
          change={(text) =>
            change(field.kind === 'number' ? { kind: 'number', text } : { kind: 'text', text })
          }
          hint={
            field.kind === 'number'
              ? 'Validated by the Host; decimal text is kept exactly.'
              : undefined
          }
        />
      </If>
      <If when={field.options.length === 0 && field.kind === 'boolean'}>
        <Each of={[false, true]}>
          {(choice) => (
            <Button
              disabled={owner.disabled()}
              selected={
                value()?.kind === 'boolean' && (value() as { value: boolean }).value === choice()
              }
              onPress={inputAction(owner.change, {
                request,
                value: { kind: 'boolean', value: choice() },
              })}
            >
              <Text style={{ color: C.text }}>{choice() ? 'On' : 'Off'}</Text>
            </Button>
          )}
        </Each>
      </If>
      <If when={field.nullable}>
        <Button
          disabled={owner.disabled()}
          selected={value()?.kind === 'null'}
          onPress={inputAction(owner.change, { request, value: { kind: 'null' } })}
        >
          <Text style={{ color: C.text }}>Set null</Text>
        </Button>
      </If>
      <If when={!field.required}>
        <Button
          disabled={owner.disabled()}
          selected={value() === null}
          onPress={inputAction(owner.change, { request, value: null })}
        >
          <Text style={{ color: C.text }}>Remove explicit value</Text>
        </Button>
      </If>
      <Button
        disabled={owner.disabled()}
        accessibilityLabel="Save Host plugin setting"
        onPress={inputAction(owner.save, { request, value: value() })}
      >
        <Text style={{ color: C.accent }}>Save plugin setting</Text>
      </Button>
    </View>
  );
}
