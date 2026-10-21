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

import { Input, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import type { FormField } from '../../../../packages/plugin-sdk/src/generated/runtime';
import type { FormDraft, FormRequest as Request } from './interaction-form';
import { C } from './ui';

type Change = Partial<FormDraft>;
function Choice(props: {
  label: string;
  selected?: boolean;
  disabled: boolean;
  onPress: () => void;
}) {
  return (
    <Button
      disabled={props.disabled}
      selected={props.selected}
      accessibilityLabel={props.label}
      onPress={props.onPress}
      style={{ paddingHorizontal: 8, backgroundColor: C.surface }}
    >
      <Text style={{ color: props.selected ? C.accent : C.text, fontSize: 16, lineHeight: 16 }}>
        {props.label}
      </Text>
    </Button>
  );
}
export function FormRequest(props: {
  request: Request;
  drafts: FormDraft[];
  problems: Array<string | null>;
  blocked: boolean;
  action: (index: number, change: Change) => () => void;
  edit: (index: number, original: string, value: string) => void;
}) {
  // Native edit callbacks keep the original form identity, just as presented
  // button actions do. Refreshing another request never retargets a field edit.
  const original = JSON.stringify(props.request);
  return (
    <View style={{ gap: 16 }}>
      <Text selectable style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
        {props.request.requester.name}
      </Text>
      <If when={props.request.requester.source}>
        <Text selectable style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
          {props.request.requester.source}
        </Text>
      </If>
      <Text selectable style={{ color: C.text, fontSize: 16, lineHeight: 16 }}>
        {props.request.message}
      </Text>
      <Each<FormField> of={props.request.fields}>
        {(field, index) => {
          const draft = () => props.drafts[index()];
          const disabled = () => props.blocked || !draft()?.included;
          const placeholder = () => {
            const current = field();
            return current.kind === 'string'
              ? (current.format ?? 'Text')
              : current.kind === 'integer'
                ? 'Whole number'
                : 'Number';
          };
          const options = () => {
            const current = field();
            return current.kind === 'single_select' || current.kind === 'multi_select'
              ? current.options
              : [];
          };
          return (
            <View style={{ gap: 16 }}>
              <Text style={{ color: C.text, fontSize: 16, lineHeight: 16, fontWeight: '600' }}>
                {field().label}
                {field().required ? ' · Required' : ' · Optional'}
              </Text>
              <If when={field().description}>
                <Text selectable style={{ color: C.muted, fontSize: 16, lineHeight: 16 }}>
                  {field().description}
                </Text>
              </If>
              <If when={!field().required}>
                <Choice
                  label={draft()?.included ? `Omit ${field().label}` : `Include ${field().label}`}
                  disabled={props.blocked}
                  selected={draft()?.included}
                  onPress={props.action(index(), { included: !draft()?.included })}
                />
              </If>
              <If when={draft()?.included}>
                <If when={['string', 'number', 'integer'].includes(field().kind)}>
                  <Input
                    accessibilityLabel={field().label}
                    value={String(draft()?.value ?? '')}
                    readonly={disabled()}
                    masked={draft()?.hidden}
                    autoCorrect={false}
                    placeholder={placeholder()}
                    onChange={(value) => props.edit(index(), original, value)}
                    style={{
                      minHeight: 32,
                      paddingHorizontal: 8,
                      color: C.text,
                      backgroundColor: C.surface,
                      placeholderColor: C.muted,
                      fontSize: 16,
                      lineHeight: 16,
                    }}
                  />
                  <If when={field().kind === 'string'}>
                    <Choice
                      label={`${draft()?.hidden ? 'Reveal' : 'Hide'} ${field().label}`}
                      disabled={props.blocked}
                      onPress={props.action(index(), { hidden: !draft()?.hidden })}
                    />
                  </If>
                </If>
                <If when={field().kind === 'boolean'}>
                  <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 16 }}>
                    <Each of={[true, false]}>
                      {(value) => (
                        <Choice
                          label={`${draft()?.value === value() ? '(*)' : '( )'} ${value() ? 'Yes' : 'No'} · ${field().label}`}
                          disabled={disabled()}
                          selected={draft()?.value === value()}
                          onPress={props.action(index(), { value: value() })}
                        />
                      )}
                    </Each>
                  </View>
                </If>
                <If when={field().kind === 'single_select' || field().kind === 'multi_select'}>
                  <Each of={options()}>
                    {(option) => {
                      const selected = () =>
                        field().kind === 'multi_select'
                          ? Array.isArray(draft()?.value) &&
                            (draft().value as string[]).includes(option().value)
                          : draft()?.value === option().value;
                      const next = () =>
                        field().kind === 'multi_select'
                          ? selected()
                            ? (draft().value as string[]).filter((part) => part !== option().value)
                            : [
                                ...(Array.isArray(draft()?.value)
                                  ? (draft().value as string[])
                                  : []),
                                option().value,
                              ]
                          : option().value;
                      return (
                        <Choice
                          label={`${selected() ? '(*)' : '( )'} ${option().label}`}
                          disabled={disabled()}
                          selected={selected()}
                          onPress={props.action(index(), { value: next() })}
                        />
                      );
                    }}
                  </Each>
                </If>
                <If when={props.problems[index()]}>
                  <Text style={{ color: C.warning, fontSize: 16, lineHeight: 16 }}>
                    {props.problems[index()]}
                  </Text>
                </If>
              </If>
            </View>
          );
        }}
      </Each>
    </View>
  );
}
