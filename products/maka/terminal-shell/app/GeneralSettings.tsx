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
import { C } from './ui';

export function GeneralSettings(props: { owner: GeneralSettingsOwner }) {
  const owner = props.owner;
  return (
    <Scroll style={{ flex: 1, minHeight: 0 }}>
      <View style={{ padding: 8, gap: 16 }}>
        <If when={owner.loading()}>
          <Text style={{ color: C.muted }}>Reading preferences…</Text>
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
        focusStyle={{ backgroundColor: C.line }}
      >
        <Text style={{ color: C.accent }}>Refresh preferences</Text>
      </Button>
      <If when={owner.facts()}>
        <View style={{ gap: 8 }}>
          <Text style={{ color: C.text, fontWeight: '700' }}>Assistant language</Text>
          <Text style={{ color: C.muted }}>
            Applies to Host prompts; the terminal interface remains English.
          </Text>
          <If when={owner.facts()?.language == null}>
            <Text style={{ color: C.muted }}>Using the Host's configured language.</Text>
          </If>
          <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
            <Each of={assistantLanguages} key={(language) => language.value}>
              {(language) => (
                <Button
                  selected={owner.facts()?.language === language().value}
                  disabled={owner.disabled()}
                  accessibilityLabel={`Assistant language: ${language().label}`}
                  onPress={inputAction(owner.language, { request, preference: language().value })}
                  style={{ paddingHorizontal: 8 }}
                  hoverStyle={{ backgroundColor: C.raised }}
                  focusStyle={{ backgroundColor: C.line }}
                >
                  <Text
                    style={{
                      color: owner.facts()?.language === language().value ? C.accent : C.text,
                    }}
                  >
                    {owner.facts()?.language === language().value ? '● ' : '○ '}
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
          hoverStyle={{ backgroundColor: C.raised }}
          focusStyle={{ backgroundColor: C.line }}
          style={{ padding: 8 }}
        >
          <Text style={{ color: C.accent }}>Personalization ›</Text>
          <Text numberOfLines={1} style={{ color: C.muted }}>
            {owner.facts()?.displayName || 'Display name and assistant tone'}
          </Text>
        </Button>
        <Button
          disabled={owner.disabled()}
          onPress={inputAction(owner.open, { request, kind: 'compaction' })}
          accessibilityLabel="Edit compaction prompt"
          hoverStyle={{ backgroundColor: C.raised }}
          focusStyle={{ backgroundColor: C.line }}
          style={{ padding: 8 }}
        >
          <Text style={{ color: C.accent }}>Compaction prompt ›</Text>
          <Text style={{ color: C.muted }}>
            Instructions used when the Host compacts conversation context.
          </Text>
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
      style={{ padding: 8, flexDirection: 'row', gap: 8 }}
      hoverStyle={{ backgroundColor: C.raised }}
      focusStyle={{ backgroundColor: C.line }}
    >
      <Text style={{ color: props.enabled ? C.accent : C.muted }}>{props.enabled ? '☑' : '☐'}</Text>
      <Text style={{ color: C.text, flex: 1 }}>{props.label}</Text>
      <Text style={{ color: C.muted }}>{props.enabled ? 'On' : 'Off'}</Text>
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
      <Text style={{ color: C.muted }}>Changes apply when you save this page.</Text>
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
          focusStyle={{ backgroundColor: C.line }}
        >
          <Text style={{ color: C.accent }}>Use Host default</Text>
        </Button>
      </If>
      <View style={{ flexDirection: 'row', gap: 16 }}>
        <Button
          disabled={owner.disabled() || !owner.dirty()}
          onPress={inputAction(owner.save, request)}
          accessibilityLabel="Save this preference"
          focusStyle={{ backgroundColor: C.line }}
        >
          <Text style={{ color: C.accent }}>Save</Text>
        </Button>
        <Button
          disabled={owner.busy()}
          accessibilityLabel="Cancel"
          onPress={inputAction(owner.cancel, request)}
          focusStyle={{ backgroundColor: C.line }}
        >
          <Text style={{ color: C.muted }}>Cancel</Text>
        </Button>
      </View>
    </View>
  );
}
