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
import { Button } from 'nsh/base';
import { TRANSCRIPT_VIEW_MODES } from './presentation';
import type { CollapseTiming, PresentationSettings, TranscriptViewMode } from './presentation';
import { C } from './ui';

const MODE_LABELS: Record<TranscriptViewMode, string> = {
  compact: 'Compact',
  standard: 'Standard',
  detailed: 'Detailed',
  verbose: 'Verbose',
};
const TIMINGS: { value: CollapseTiming; label: string }[] = [
  { value: 'completion', label: 'On completion' },
  { value: 'next-input', label: 'Next input' },
];

export function PresentationControls(props: { settings: PresentationSettings }) {
  const settings = props.settings;
  const expanded = signal(false);
  let alive = true;
  cleanup(() => {
    alive = false;
  });
  const setExpanded = (open: boolean) => {
    if (alive) expanded.set(open);
  };
  const chooseMode = (mode: TranscriptViewMode) => {
    if (alive) settings.setMode(mode);
  };
  const chooseTiming = (timing: CollapseTiming) => {
    if (alive) settings.setCollapseTiming(timing);
  };
  return (
    <View style={{ alignSelf: 'flex-start', maxWidth: '100%' }}>
      <Button
        accessibilityLabel="Work details"
        selected={expanded()}
        onPress={inputAction(setExpanded, !expanded())}
        hoverStyle={{ backgroundColor: C.surface }}
        focusStyle={{ backgroundColor: C.raised }}
        activeStyle={{ backgroundColor: C.line }}
      >
        <Text style={{ color: expanded() ? C.accent : C.secondary }}>
          Details · {MODE_LABELS[settings.mode()]} {expanded() ? '⌃' : '⌄'}
        </Text>
      </Button>
      <If when={expanded()}>
        <View style={{ paddingVertical: 8, gap: 8 }}>
          <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
            <Each of={TRANSCRIPT_VIEW_MODES} key={(mode) => mode}>
              {(mode) => (
                <Button
                  accessibilityLabel={`Work details: ${MODE_LABELS[mode()]}`}
                  selected={settings.mode() === mode()}
                  onPress={inputAction(chooseMode, mode())}
                  style={{ paddingHorizontal: 8 }}
                  hoverStyle={{ backgroundColor: C.surface }}
                  focusStyle={{ backgroundColor: C.raised }}
                  activeStyle={{ backgroundColor: C.line }}
                >
                  <Text style={{ color: settings.mode() === mode() ? C.accent : C.secondary }}>
                    {MODE_LABELS[mode()]}
                  </Text>
                </Button>
              )}
            </Each>
          </View>
          <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
            <Text style={{ color: C.muted }}>Fold:</Text>
            <Each of={TIMINGS} key={(timing) => timing.value}>
              {(timing) => (
                <Button
                  accessibilityLabel={`Fold work details: ${timing().label}`}
                  selected={settings.collapseTiming() === timing().value}
                  onPress={inputAction(chooseTiming, timing().value)}
                  style={{ paddingHorizontal: 8 }}
                  hoverStyle={{ backgroundColor: C.surface }}
                  focusStyle={{ backgroundColor: C.raised }}
                  activeStyle={{ backgroundColor: C.line }}
                >
                  <Text
                    style={{
                      color: settings.collapseTiming() === timing().value ? C.accent : C.secondary,
                    }}
                  >
                    {timing().label}
                  </Text>
                </Button>
              )}
            </Each>
          </View>
        </View>
      </If>
      <If when={settings.notice()}>
        <Text style={{ color: C.warning }}>{settings.notice()}</Text>
      </If>
    </View>
  );
}
