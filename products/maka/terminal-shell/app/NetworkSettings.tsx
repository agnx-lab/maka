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
import type { ProxyProtocol } from '../../../../packages/plugin-sdk/src/generated/runtime';
import { SettingsField } from './SettingsField';
import type { NetworkSettingsOwner } from './networkSettings';
import { C, controlStyle, controlHoverStyle, controlFocusStyle, controlDisabledStyle } from './ui';

export function NetworkSettings(props: { owner: NetworkSettingsOwner }) {
  const owner = props.owner;
  const value = () => owner.draft()!;
  const disabled = () => owner.disabled();
  const protocols: ProxyProtocol[] = ['http', 'https', 'socks5'];
  return (
    <View style={{ flex: 1, gap: 8 }}>
      <If when={owner.loading()}>
        <Text style={{ color: C.muted }}>Loading network settings…</Text>
      </If>
      <If when={owner.draft() && owner.snapshot()}>
        <Scroll style={{ flex: 1 }}>
          <View style={{ gap: 12, padding: 8 }}>
            <Text style={{ color: C.muted }}>
              Applies to this Host’s future network requests. Running requests keep their captured
              policy.
            </Text>
            <Button
              disabled={disabled()}
              selected={value().proxy.enabled}
              accessibilityLabel="Enable Host proxy"
              onPress={inputAction(owner.choose, {
                ...owner.action(),
                kind: 'enabled',
                value: !value().proxy.enabled,
              })}
              style={controlStyle}
              hoverStyle={controlHoverStyle}
              focusStyle={controlFocusStyle}
              disabledStyle={controlDisabledStyle}
            >
              <Text style={{ color: C.text }}>
                {value().proxy.enabled ? '✓ Proxy enabled' : '○ Proxy disabled'}
              </Text>
            </Button>
            <View style={{ flexDirection: 'row', gap: 16 }}>
              <Each of={protocols} key={(protocol) => protocol}>
                {(protocol) => (
                  <Button
                    disabled={disabled()}
                    selected={value().proxy.protocol === protocol()}
                    accessibilityLabel={`Proxy protocol ${protocol()}`}
                    onPress={inputAction(owner.choose, {
                      ...owner.action(),
                      kind: 'protocol',
                      value: protocol(),
                    })}
                    style={controlStyle}
                    hoverStyle={controlHoverStyle}
                    focusStyle={controlFocusStyle}
                    disabledStyle={controlDisabledStyle}
                  >
                    <Text style={{ color: C.accent }}>{protocol()}</Text>
                  </Button>
                )}
              </Each>
            </View>
            <SettingsField
              label="Proxy host"
              value={value().proxy.host}
              disabled={disabled()}
              change={(text) => owner.edit('host', text)}
              hint="Host name or IP address; enter the port separately."
            />
            <SettingsField
              label="Proxy port"
              value={value().port}
              disabled={disabled()}
              change={(text) => owner.edit('port', text)}
            />
            <Button
              disabled={disabled()}
              selected={value().proxy.authEnabled}
              accessibilityLabel="Enable proxy authentication"
              onPress={inputAction(owner.choose, {
                ...owner.action(),
                kind: 'authentication',
                value: !value().proxy.authEnabled,
              })}
              style={controlStyle}
              hoverStyle={controlHoverStyle}
              focusStyle={controlFocusStyle}
              disabledStyle={controlDisabledStyle}
            >
              <Text style={{ color: C.text }}>
                {value().proxy.authEnabled
                  ? '✓ Proxy authentication enabled'
                  : '○ Proxy authentication disabled'}
              </Text>
            </Button>
            <Text style={{ color: C.muted }}>
              Saving with proxy authentication disabled deletes its stored password.
            </Text>
            <If when={value().proxy.authEnabled}>
              <SettingsField
                label="Proxy username"
                value={value().proxy.username}
                disabled={disabled()}
                change={(text) => owner.edit('username', text)}
              />
              <View style={{ flexDirection: 'row', gap: 16 }}>
                <Button
                  disabled={disabled()}
                  selected={value().credential === 'keep'}
                  accessibilityLabel="Keep proxy password"
                  onPress={inputAction(owner.choose, {
                    ...owner.action(),
                    kind: 'credential',
                    value: 'keep',
                  })}
                  style={controlStyle}
                  hoverStyle={controlHoverStyle}
                  focusStyle={controlFocusStyle}
                  disabledStyle={controlDisabledStyle}
                >
                  <Text style={{ color: C.accent }}>Keep stored password</Text>
                </Button>
                <Button
                  disabled={disabled()}
                  selected={value().credential === 'replace'}
                  accessibilityLabel="Replace proxy password"
                  onPress={inputAction(owner.choose, {
                    ...owner.action(),
                    kind: 'credential',
                    value: 'replace',
                  })}
                  style={controlStyle}
                  hoverStyle={controlHoverStyle}
                  focusStyle={controlFocusStyle}
                  disabledStyle={controlDisabledStyle}
                >
                  <Text style={{ color: C.accent }}>Replace password</Text>
                </Button>
              </View>
              <Text style={{ color: C.muted }}>
                {owner.snapshot()?.credential.configured
                  ? 'A proxy password is stored. Its value is never read back.'
                  : 'No proxy password is stored.'}
              </Text>
              <If when={value().credential === 'replace'}>
                <SettingsField
                  label="Proxy password"
                  value={value().secret}
                  masked
                  disabled={disabled()}
                  change={(text) => owner.edit('secret', text)}
                  hint="Saved as a private Host credential, separately from the proxy configuration."
                />
              </If>
            </If>
            <SettingsField
              label="Proxy bypass rules"
              value={value().bypass}
              multiline
              disabled={disabled()}
              change={(text) => owner.edit('bypass', text)}
              hint="One Host bypass rule per line. Automatic rules are retained."
            />
            <If when={owner.snapshot()!.proxy.autoBypassDomains.length > 0}>
              <Text style={{ color: C.muted }}>
                Automatic bypass: {owner.snapshot()!.proxy.autoBypassDomains.join(', ')}
              </Text>
            </If>
            <Button
              disabled={disabled()}
              accessibilityLabel="Save network settings"
              onPress={inputAction(owner.save, owner.action())}
              style={controlStyle}
              hoverStyle={controlHoverStyle}
              focusStyle={controlFocusStyle}
              disabledStyle={controlDisabledStyle}
            >
              <Text style={{ color: C.accent }}>Save network settings</Text>
            </Button>
            <Button
              disabled={disabled()}
              accessibilityLabel="Reload network settings"
              onPress={inputAction(owner.reload, owner.action())}
              style={controlStyle}
              hoverStyle={controlHoverStyle}
              focusStyle={controlFocusStyle}
              disabledStyle={controlDisabledStyle}
            >
              <Text style={{ color: C.muted }}>Discard edits and reload</Text>
            </Button>
          </View>
        </Scroll>
      </If>
      <If when={!owner.draft() && !owner.loading()}>
        <Button
          accessibilityLabel="Reload network settings"
          onPress={inputAction(owner.reload, owner.action())}
          style={controlStyle}
          hoverStyle={controlHoverStyle}
          focusStyle={controlFocusStyle}
          disabledStyle={controlDisabledStyle}
        >
          <Text style={{ color: C.accent }}>Reload network settings</Text>
        </Button>
      </If>
    </View>
  );
}
