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

import { inputAction, Text, View } from 'nsh';
import { Image } from 'nsh/image';
import { Button } from 'nsh/base';
import { windowSize } from 'nsh/device';
import { C, Header } from './ui';
import type { Browsing } from './browsing';
import logo from './assets/maka.png';

export function Home(props: {
  browsing: Browsing;
  switching(): boolean;
  commands(): void;
  browse(): void;
  newConversation(): void;
  selectSession(id: string, expectedSessionId: string | null): Promise<void>;
}) {
  const showHomeLogo = () => windowSize().height >= 336;
  const homeSessionLimit = () => {
    // Header, controls and gaps use 17 rows; the logo adds five.
    const available = Math.floor(windowSize().height / 16) - (showHomeLogo() ? 22 : 17);
    return Math.max(0, Math.min(5, Math.floor(available / 3)));
  };
  const resume = (id: string) => {
    void props.selectSession(id, null);
  };
  return (
    <View style={{ flex: 1 }}>
      <Header title="Your terminal, amplified." commands={props.commands} browse={props.browse} />
      <View
        style={{
          flex: 1,
          width: '100%',
          maxWidth: 800,
          alignSelf: 'center',
          padding: 32,
          gap: 16,
          justifyContent: 'center',
        }}
      >
        <If when={showHomeLogo()}>
          <Image src={logo} resizeMode="contain" style={{ width: 64, height: 64, flexShrink: 0 }} />
        </If>
        <Text style={{ color: C.accent, fontWeight: '700' }}>Make something remarkable.</Text>
        <Text style={{ color: C.muted }}>A quiet place for ambitious work.</Text>
        <Button onPress={props.newConversation} style={{ paddingVertical: 16 }}>
          <Text style={{ color: C.text }}>+ New conversation · Ctrl+N</Text>
        </Button>
        <If when={homeSessionLimit() > 0 && props.browsing.sessions().length > 0}>
          <Text style={{ color: C.muted }}>Sessions</Text>
        </If>
        <Each
          of={props.browsing.sessions().slice(0, homeSessionLimit())}
          key={(session) => session.id}
        >
          {(session) => (
            <Button
              disabled={props.switching()}
              onPress={inputAction(resume, session().id)}
              style={{ paddingVertical: 0 }}
            >
              <Text numberOfLines={1} style={{ color: C.text }}>
                {session().title || 'Untitled conversation'}
              </Text>
              <Text numberOfLines={1} style={{ color: C.muted }}>
                {session().workspace.path}
              </Text>
            </Button>
          )}
        </Each>
        <Button onPress={props.browse}>
          <Text style={{ color: C.muted }}>All conversations · Ctrl+O</Text>
        </Button>
        <Text style={{ color: C.muted }}>Ctrl+P commands · Ctrl+Q exits</Text>
      </View>
    </View>
  );
}
