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
import {
  assistantLanguages,
  type GeneralAction,
  type GeneralSettingsOwner,
} from './generalSettings';
import { SettingsField } from './SettingsField';
import { ChoiceIndicator } from './ChoiceIndicator';
import { Icon } from './Icon';
import {
  bodyTextStyle,
  captionTextStyle,
  C,
  controlStyle,
  controlHoverStyle,
  controlFocusStyle,
  controlDisabledStyle,
} from './ui';

export function GeneralSettings(props: { owner: GeneralSettingsOwner }) {
  const owner = props.owner;
  return (
    <Scroll style={{ flex: 1, minHeight: 0 }}>
      <View style={{ padding: 8, gap: 16 }}>
        <If when={owner.loading()}>
          <Text style={{ ...bodyTextStyle, color: C.muted }}>Reading preferences…</Text>
        </If>
        <Each of={[owner.action()]} key={(request) => JSON.stringify(request)}>
          {(action) => {
            const request = action();
            if (request.kind === 'root') return <GeneralRoot owner={owner} request={request} />;
            return <PreferenceEditor owner={owner} request={request} />;
          }}
        </Each>
      </View>
    </Scroll>
  );
}

function GeneralRoot(props: { owner: GeneralSettingsOwner; request: GeneralAction }) {
  const owner = props.owner;
  const request = props.request;
  return (
    <View style={{ gap: 16 }}>
      <Button
        disabled={owner.loading() || owner.busy()}
        onPress={inputAction(owner.reload, request)}
        accessibilityLabel="Refresh general preferences"
        focusStyle={controlFocusStyle}
        style={{ ...controlStyle, alignSelf: 'flex-start' }}
        hoverStyle={controlHoverStyle}
        disabledStyle={controlDisabledStyle}
      >
        <Text style={{ ...bodyTextStyle, color: C.secondary }}>Refresh preferences</Text>
      </Button>
      <If when={owner.facts()}>
        <View style={{ gap: 8 }}>
          <Text style={{ ...bodyTextStyle, color: C.text, fontWeight: '600' }}>
            Assistant language
          </Text>
          <Text style={{ ...captionTextStyle, color: C.muted }}>
            Language used by the assistant. The interface remains English.
          </Text>
          <If when={owner.facts()?.language == null}>
            <Text style={{ ...captionTextStyle, color: C.muted }}>Using the default language.</Text>
          </If>
          <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
            <Each of={assistantLanguages} key={(language) => language.value}>
              {(language) => (
                <Button
                  selected={owner.facts()?.language === language().value}
                  disabled={owner.disabled()}
                  accessibilityLabel={`Assistant language: ${language().label}`}
                  onPress={inputAction(owner.language, { request, preference: language().value })}
                  style={{
                    ...controlStyle,
                    minHeight: 40,
                    flexDirection: 'row',
                    alignItems: 'center',
                    gap: 8,
                  }}
                  selectedStyle={{ backgroundColor: C.raised }}
                  hoverStyle={controlHoverStyle}
                  focusStyle={controlFocusStyle}
                  disabledStyle={controlDisabledStyle}
                >
                  <ChoiceIndicator
                    kind="radio"
                    checked={owner.facts()?.language === language().value}
                    disabled={owner.disabled()}
                  />
                  <Text
                    style={{
                      ...bodyTextStyle,
                      color: owner.facts()?.language === language().value ? C.accent : C.text,
                    }}
                  >
                    {language().label}
                  </Text>
                </Button>
              )}
            </Each>
          </View>
        </View>
        <View style={{ gap: 8 }}>
          <PreferenceToggle
            owner={owner}
            request={request}
            kind="instructions"
            label="Workspace instructions"
            enabled={owner.facts()?.workspaceInstructions ?? false}
          />
          <PreferenceToggle
            owner={owner}
            request={request}
            kind="privacy"
            label="Incognito mode"
            enabled={owner.facts()?.incognitoActive ?? false}
          />
          <PreferenceToggle
            owner={owner}
            request={request}
            kind="titles"
            label="Automatic shell titles"
            enabled={owner.facts()?.shellTitles ?? false}
          />
        </View>
        <Button
          disabled={owner.disabled()}
          onPress={inputAction(owner.open, { request, kind: 'personalization' })}
          accessibilityLabel="Edit personalization"
          hoverStyle={controlHoverStyle}
          focusStyle={controlFocusStyle}
          style={{
            ...controlStyle,
            minHeight: 56,
            flexDirection: 'row',
            alignItems: 'center',
            gap: 12,
          }}
          disabledStyle={controlDisabledStyle}
        >
          <View pointerEvents="none" style={{ flex: 1, minWidth: 0, gap: 4 }}>
            <Text style={{ ...bodyTextStyle, color: C.text }}>Personalization</Text>
            <Text numberOfLines={1} style={{ ...captionTextStyle, color: C.muted }}>
              {owner.facts()?.displayName || 'Display name and assistant tone'}
            </Text>
          </View>
          <Icon name="right" color={C.muted} />
        </Button>
        <Button
          disabled={owner.disabled()}
          onPress={inputAction(owner.open, { request, kind: 'compaction' })}
          accessibilityLabel="Edit compaction prompt"
          hoverStyle={controlHoverStyle}
          focusStyle={controlFocusStyle}
          style={{
            ...controlStyle,
            minHeight: 56,
            flexDirection: 'row',
            alignItems: 'center',
            gap: 12,
          }}
          disabledStyle={controlDisabledStyle}
        >
          <View pointerEvents="none" style={{ flex: 1, minWidth: 0, gap: 4 }}>
            <Text style={{ ...bodyTextStyle, color: C.text }}>Compaction prompt</Text>
            <Text style={{ ...captionTextStyle, color: C.muted }}>
              Instructions for summarizing conversation context.
            </Text>
          </View>
          <Icon name="right" color={C.muted} />
        </Button>
      </If>
    </View>
  );
}

function PreferenceToggle(props: {
  owner: GeneralSettingsOwner;
  request: GeneralAction;
  kind: 'instructions' | 'privacy' | 'titles';
  label: string;
  enabled: boolean;
}) {
  return (
    <Button
      selected={props.enabled}
      disabled={props.owner.disabled()}
      accessibilityLabel={`${props.label}: ${props.enabled ? 'On' : 'Off'}`}
      onPress={inputAction(props.owner.toggle, { request: props.request, kind: props.kind })}
      style={{
        ...controlStyle,
        minHeight: 40,
        flexDirection: 'row',
        alignItems: 'center',
        gap: 12,
      }}
      hoverStyle={controlHoverStyle}
      focusStyle={controlFocusStyle}
      disabledStyle={controlDisabledStyle}
    >
      <Text style={{ ...bodyTextStyle, color: C.text, flex: 1 }}>{props.label}</Text>
      <ChoiceIndicator kind="switch" checked={props.enabled} disabled={props.owner.disabled()} />
    </Button>
  );
}

function PreferenceEditor(props: { owner: GeneralSettingsOwner; request: GeneralAction }) {
  const owner = props.owner;
  const request = props.request;
  const name = () => {
    const editor = owner.editor();
    return editor?.kind === 'personalization' ? editor.displayName : '';
  };
  const tone = () => {
    const editor = owner.editor();
    return editor?.kind === 'personalization' ? editor.assistantTone : '';
  };
  const prompt = () => {
    const editor = owner.editor();
    return editor?.kind === 'compaction' ? editor.prompt : '';
  };
  return (
    <View style={{ gap: 16 }}>
      <Text style={{ ...captionTextStyle, color: C.muted }}>
        Changes apply when you save this page.
      </Text>
      <If when={request.kind === 'personalization'}>
        <SettingsField
          label="Display name"
          value={name()}
          change={(text) => owner.editName(request, 'displayName', text)}
          disabled={owner.disabled()}
          hint="Up to 256 characters."
        />
        <SettingsField
          label="Assistant tone"
          value={tone()}
          change={(text) => owner.editName(request, 'assistantTone', text)}
          disabled={owner.disabled()}
          multiline
          hint="How the assistant should respond. Up to 4096 characters."
        />
      </If>
      <If when={request.kind === 'compaction'}>
        <SettingsField
          label="Compaction prompt"
          value={prompt()}
          change={(text) => owner.editPrompt(request, text)}
          disabled={owner.disabled()}
          multiline
        />
        <Button
          disabled={owner.disabled()}
          accessibilityLabel="Use Host default"
          onPress={inputAction(owner.useDefault, request)}
          focusStyle={controlFocusStyle}
          style={controlStyle}
          hoverStyle={controlHoverStyle}
          disabledStyle={controlDisabledStyle}
        >
          <Text style={{ ...bodyTextStyle, color: C.secondary }}>Use default prompt</Text>
        </Button>
      </If>
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
        <Button
          disabled={owner.disabled() || !owner.dirty()}
          onPress={inputAction(owner.save, request)}
          accessibilityLabel="Save this preference"
          focusStyle={{ backgroundColor: C.accentHover }}
          style={{ ...controlStyle, alignSelf: 'flex-start', backgroundColor: C.accent }}
          hoverStyle={{ backgroundColor: C.accentHover }}
          disabledStyle={controlDisabledStyle}
        >
          <Text style={{ ...bodyTextStyle, color: C.background, fontWeight: '500' }}>Save</Text>
        </Button>
        <Button
          disabled={owner.busy()}
          accessibilityLabel="Cancel"
          onPress={inputAction(owner.cancel, request)}
          focusStyle={controlFocusStyle}
          style={controlStyle}
          hoverStyle={controlHoverStyle}
          disabledStyle={controlDisabledStyle}
        >
          <Text style={{ ...bodyTextStyle, color: C.secondary }}>Cancel</Text>
        </Button>
      </View>
    </View>
  );
}
