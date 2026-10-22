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
import { thinkingLevels, type ModelSettingsOwner } from './settings';
import { SettingsField } from './SettingsField';
import { ChoiceIndicator } from './ChoiceIndicator';
import { tr } from './locale';
import {
  C,
  bodyTextStyle,
  captionTextStyle,
  controlStyle,
  controlHoverStyle,
  controlFocusStyle,
  controlDisabledStyle,
} from './ui';

export function ConnectionModel(props: { settings: ModelSettingsOwner }) {
  const owner = props.settings;
  const model = () => owner.model()!;
  const disabled = () => owner.busy() || !owner.pendingKnown() || owner.pending().length > 0;
  return (
    <Scroll style={{ flex: 1 }}>
      <View style={{ padding: 8, gap: 16 }}>
        <SettingsField
          label={tr('Model ID')}
          value={model().id}
          readonly={model().originalId !== null}
          disabled={disabled()}
          change={(id) => owner.changeModel({ id })}
          hint={tr('The exact API model ID, including its provider prefix when required.')}
        />
        <SettingsField
          label={tr('Model display name')}
          value={model().label}
          disabled={disabled()}
          change={(label) => owner.changeModel({ label })}
          hint={tr('Optional. Empty uses the provider’s name.')}
        />
        <Text style={{ ...bodyTextStyle, color: C.text, fontWeight: '500' }}>
          {tr('Supported reasoning efforts')}
        </Text>
        <Button
          disabled={disabled()}
          selected={model().thinkingLevels === null}
          onPress={inputAction(owner.useProviderThinking, owner.modelAction())}
          style={{ ...controlStyle, flexDirection: 'row', alignItems: 'center', gap: 12 }}
          hoverStyle={controlHoverStyle}
          focusStyle={controlFocusStyle}
          disabledStyle={controlDisabledStyle}
        >
          <ChoiceIndicator
            kind="radio"
            checked={model().thinkingLevels === null}
            disabled={disabled()}
          />
          <Text style={{ ...captionTextStyle, color: C.text }}>{tr('Use provider defaults')}</Text>
        </Button>
        <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 12 }}>
          <Each of={thinkingLevels} key={(level) => level}>
            {(level) => (
              <Button
                disabled={disabled()}
                selected={model().thinkingLevels?.includes(level()) ?? false}
                accessibilityLabel={tr('Support effort {level}', { level: level() })}
                onPress={inputAction(owner.toggleThinking, {
                  ...owner.modelAction(),
                  level: level(),
                })}
                style={{ ...controlStyle, flexDirection: 'row', alignItems: 'center', gap: 8 }}
                hoverStyle={controlHoverStyle}
                focusStyle={controlFocusStyle}
                disabledStyle={controlDisabledStyle}
              >
                <ChoiceIndicator
                  kind="checkbox"
                  checked={model().thinkingLevels?.includes(level()) ?? false}
                  disabled={disabled()}
                />
                <Text style={{ ...captionTextStyle, color: C.text }}>{level()}</Text>
              </Button>
            )}
          </Each>
        </View>
        <Text style={{ ...bodyTextStyle, color: C.muted }}>
          {tr(
            'Select the reasoning efforts supported by this model. They will be available in conversations.',
          )}
        </Text>
        <Text style={{ ...bodyTextStyle, color: C.text, fontWeight: '500' }}>
          {tr('Connection default reasoning effort')}
        </Text>
        <Button
          disabled={disabled()}
          selected={model().defaultThinkingLevel === null}
          onPress={inputAction(owner.defaultThinking, { ...owner.modelAction(), level: null })}
          style={{ ...controlStyle, flexDirection: 'row', alignItems: 'center', gap: 12 }}
          hoverStyle={controlHoverStyle}
          focusStyle={controlFocusStyle}
          disabledStyle={controlDisabledStyle}
        >
          <ChoiceIndicator
            kind="radio"
            checked={model().defaultThinkingLevel === null}
            disabled={disabled()}
          />
          <Text style={{ ...captionTextStyle, color: C.text }}>{tr('Provider default')}</Text>
        </Button>
        <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 12 }}>
          <Each of={model().thinkingLevels ?? []} key={(level) => level}>
            {(level) => (
              <Button
                disabled={disabled()}
                selected={model().defaultThinkingLevel === level()}
                accessibilityLabel={tr('Default effort {level}', { level: level() })}
                onPress={inputAction(owner.defaultThinking, {
                  ...owner.modelAction(),
                  level: level(),
                })}
                style={{ ...controlStyle, flexDirection: 'row', alignItems: 'center', gap: 8 }}
                hoverStyle={controlHoverStyle}
                focusStyle={controlFocusStyle}
                disabledStyle={controlDisabledStyle}
              >
                <ChoiceIndicator
                  kind="radio"
                  checked={model().defaultThinkingLevel === level()}
                  disabled={disabled()}
                />
                <Text style={{ ...captionTextStyle, color: C.text }}>{level()}</Text>
              </Button>
            )}
          </Each>
        </View>
        <Button
          disabled={disabled()}
          accessibilityLabel={tr('Save model')}
          onPress={inputAction(owner.saveModel, owner.modelAction())}
          style={{ ...controlStyle, alignSelf: 'flex-start', backgroundColor: C.accent }}
          hoverStyle={{ backgroundColor: C.accentHover }}
          focusStyle={{ backgroundColor: C.accentHover }}
          disabledStyle={controlDisabledStyle}
        >
          <Text style={{ ...captionTextStyle, color: C.background, fontWeight: '500' }}>
            {owner.busy() ? tr('Saving…') : tr('Save model')}
          </Text>
        </Button>
        <If when={model().originalId !== null}>
          <Button
            disabled={disabled()}
            onPress={inputAction(owner.confirm, {
              ...owner.action(),
              kind: 'model',
              modelId: model().originalId,
            })}
            style={controlStyle}
            hoverStyle={controlHoverStyle}
            focusStyle={controlFocusStyle}
            disabledStyle={controlDisabledStyle}
          >
            <Text style={{ ...captionTextStyle, color: C.danger }}>{tr('Remove model')}</Text>
          </Button>
        </If>
      </View>
    </Scroll>
  );
}
