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

import type { RowPosition } from './history-position.js';
import {
  contracts as native,
  type Contract,
  type AgentSession,
  type ModelSelection,
  type AgentExecutorSelection,
  type AgentWorkspace,
  type AgentDataSource,
  type InteractionRecord,
  type SettingsModelChoice,
  type ThinkingLevel,
  type SettingsDefaultModel,
  type TextareaAppearance,
  type TerminalPalette,
  type MarkdownAppearance,
  type InputAppearance,
  type ConnectionCredentialTarget,
  type SettingsConnectionDetail,
  type MenuAppearance,
  type EditorCommand,
  type EditorObservation,
  type FilePasteCommand,
  type FilePastePolicy,
  type Role,
  type AgentHistoryChunk,
  type HistoryTurnTiming,
  type SourceField,
  type SourceFormat,
} from '../../packages/plugin-sdk/src/index.js';
import type { AttachmentDrafts } from './attachments.js';

export type Schema = Record<string, unknown>;
export const object = (properties: Schema, required = Object.keys(properties)): Schema => ({
  type: 'object',
  properties,
  required,
  additionalProperties: false,
});
export const string = { type: 'string' };
export const locale = { type: 'string', minLength: 1 };
import { appearanceConfigurationSchema, type AppearanceConfiguration } from './terminal-theme.js';
export interface Theme {
  configuration: AppearanceConfiguration;
  terminal: TerminalPalette | null;
  motion: boolean;
  ascii: boolean;
  thinking: number;
  success: number;
  warning: number;
  error: number;
  subtle: number;
  scrollbar: number;
  selectionText: number;
  search: number;
  searchActive: number;
  searchText: number;
  background: number;
  surface: number;
  text: number;
  muted: number;
  accent: number;
  border: number;
  selected: number;
  textarea: TextareaAppearance;
  input: InputAppearance;
  menu: MenuAppearance;
  markdown: MarkdownAppearance;
}
const rowPosition = object({
  sourceSequence: { type: 'integer', minimum: 1 },
  itemIndex: { type: 'integer', minimum: 0 },
});
export interface Message {
  origin?: AgentHistoryChunk['origin'];
  model?: AgentHistoryChunk['model'];
  compaction?: { durationMs: number };
  pending?: boolean;
  id: string;
  // Stable message field/byte-segment identity, including the streaming prefix.
  contentKey: string;
  role: Role | 'thinking';
  timestamp: number | null;
  text: string;
  tool: AgentHistoryChunk['tool'];
  continued: boolean;
  source: RowPosition | null;
  image: boolean;
}
export interface ImageViewerProps {
  theme: Theme;
  locale: string;
  active: boolean;
  image: { sessionId: string; through: number; position: RowPosition } | null;
}
export interface SelectedModel {
  choice: SettingsModelChoice;
  thinkingLevel: ThinkingLevel | null;
}
export interface ModelsProps {
  selectionKey?: string;
  preferredModel?: {
    connectionId: string;
    modelId: string;
    thinkingLevel: ThinkingLevel | null;
  } | null;
  /** Forms can require an explicit choice instead of adopting the chat default. */
  suggestDefault?: boolean;
  catalogEpoch: number;
  openRequest: number | null;
  theme: Theme;
  locale: string;
  selection: SelectedModel | null;
  busy: boolean;
}
export interface ExecutionPickerProps extends ModelsProps {
  executor: AgentExecutorSelection | null;
}
export interface ModelSetup {
  catalogEpoch: number;
  kind: 'loading' | 'unavailable' | 'connect' | 'choose';
}
export type ToolDisplayStatus = 'succeeded' | 'failed' | 'running' | 'cancelled' | 'unknown';
export interface ToolDetailsContext {
  sessionId: string;
  through: number;
  toolUseId: string;
  presentation: string | null;
  call: RowPosition | null;
  result: RowPosition | null;
  path: string | null;
  status: ToolDisplayStatus | null;
  expandable: boolean;
}
export interface TimelineFileContext {
  sessionId: string;
  through: number;
  landing: number;
  path: string | null;
}
export interface MessageCopyObservation {
  request: number;
  sessionId: string;
  through: number;
  landing: number;
  field: SourceField;
  status: 'loading' | 'ready' | 'empty' | 'unavailable' | 'streaming';
  source: string | null;
  message: string | null;
}
export interface PaletteCommand {
  id: string;
  label: string;
  hint: string;
  disabled: boolean;
  copyText?: string;
}
export interface PaletteProps {
  theme: Theme;
  locale: string;
  open: boolean;
  busy: boolean;
  selectionDisabled: boolean;
  commands: PaletteCommand[];
}
export interface HelpProps {
  theme: Theme;
  locale: string;
  open: boolean;
}
export interface SkillLibraryProps extends HelpProps {
  workspace?: AgentWorkspace | null;
}
export interface PageShellProps {
  shell?: { back: number };
}
export interface PageChrome {
  title: string;
  backEnabled: boolean;
}
export interface ProjectsProps extends PageShellProps {
  theme: Theme;
  locale: string;
  open: boolean;
  busy: boolean;
}
export type ImportsProps = ProjectsProps;
export type RecallProps = ProjectsProps;
export interface SchedulerProps extends ProjectsProps {
  catalogEpoch: number;
  requested: { sequence: number; taskId: string } | null;
}
export interface DirectoryPickerProps {
  purpose?: 'workspace' | 'import';
  chooseLabel?: string;
  theme: Theme;
  locale: string;
  open: boolean;
  busy: boolean;
}
export interface SessionDetailsProps {
  theme: Theme;
  locale: string;
  open: boolean;
  busy: boolean;
  session: AgentSession | null;
}
export interface TerminalPanelProps extends SessionDetailsProps {
  enabled: boolean;
}
export type QuestionDraft =
  | { kind: 'option'; index: number }
  | { kind: 'skip' }
  | { kind: 'text'; observation: EditorObservation };
export interface QuestionEditorProps {
  theme: Theme;
  locale: string;
  open: boolean;
  busy: boolean;
  checking: boolean;
  status: string;
  record: InteractionRecord;
}
export interface ModelDefaultProps {
  theme: Theme;
  locale: string;
  selection: SettingsModelChoice | null;
  defaultModel: SettingsDefaultModel | null;
  catalogRevision: number | null;
  busy: boolean;
}
export interface NavigationProps {
  theme: Theme;
  locale: string;
  sessions: AgentSession[];
  selected: string | null;
  busy: boolean;
  loading?: boolean;
  older?: boolean;
  newer?: boolean;
}
export interface SourcePage {
  kind: 'message' | SourceField;
  fields: SourceField[];
  format: SourceFormat;
  ready: boolean;
  key: string;
  text: string;
  atStart: boolean;
  atEnd: boolean;
  loading: boolean;
  failed: boolean;
  navigation: number;
  landing: 'start' | 'end';
}
export interface ToolDetailsProps {
  load?: boolean;
  theme: Theme;
  locale: string;
  context: { sessionId: string; through: number };
  tool: NonNullable<Message['tool']>;
  expanded: boolean;
  focusOnPointer: boolean;
}
export interface TimelineProps {
  active?: boolean;
  leadingPrompt?: { position: RowPosition; text: string } | null;
  activeOutput?: Message | null;
  copyRequest: number | null;
  browse: number | null;
  timings: (HistoryTurnTiming & { turnId: string; afterMessageId: string })[];
  markReadEnabled: boolean;
  toolContext: ToolDetailsProps['context'] | null;
  waitingTools: string[];
  liveTools: string[];
  theme: Theme;
  locale: string;
  messages: Message[];
  source: SourcePage | null;
  search: { sessionId: string; epoch: number; through: number } | null;
  loading: boolean;
  older: boolean;
  newer: boolean;
  latest: boolean;
  landing: { sequence: number; edge: 'start' | 'end' };
  live: boolean;
}
export interface ServiceApprovalProps {
  theme: Theme;
  locale: string;
  open: boolean;
  busy: boolean;
  checking: boolean;
  status: string;
  record: InteractionRecord;
}
export interface QueueEditorProps {
  hasAttachments: boolean;
  theme: Theme;
  locale: string;
  open: boolean;
  ready: boolean;
  busy: boolean;
  uncertain: boolean;
  checking: boolean;
  status: string;
  command: EditorCommand | null;
}
export type ComposerBackend =
  | { kind: 'model'; model: ModelSelection; thinkingLevel: ThinkingLevel | null }
  | { kind: 'executor'; selection: AgentExecutorSelection };
export interface ComposerSubmission {
  context: string;
  dataSource: AgentDataSource;
  key: string;
  text: string;
  attachments: string[];
  status: 'pending' | 'unknown' | 'accepted' | 'removed';
  messageId: string | null;
}
export interface ComposerSent {
  context: string;
  dataSource: AgentDataSource;
  attachments: string[];
}
export interface ComposerCommand {
  id: string;
  command: string;
  label: string;
  disabled: boolean;
}
export interface ComposerProps {
  skillContext?: string;
  commands?: ComposerCommand[];
  approvalPending?: boolean;
  backend: ComposerBackend | null;
  sendLabel?: string;
  dataSource: AgentDataSource | null;
  attachments: AttachmentDrafts['files'];
  context: string | null;
  filePaste: FilePastePolicy | null;
  fileCommand: FilePasteCommand | null;
  autoFocus: boolean;
  theme: Theme;
  locale: string;
  busy: boolean;
  running: boolean;
  enabled: boolean;
}
// Reuse the exact native schemas; this package defines only its own interfaces.
const editor = native.textarea.interface;
const session = native.agent.interface.methods.list.output;
const defs = {
  ...native.agent.interface.methods.send.input.$defs,
  ...native.agent.interface.methods.execute.input.$defs,
  ...native.agent.interface.methods.respond.input.$defs,
  ...native.agent.interface.methods.directories.output.$defs,
  ...native.agent.interface.methods.projects.output.$defs,
  ...editor.props.$defs,
  ...editor.events.changed.$defs,
  ...editor.events.files.$defs,
  ...editor.events.checkpoint.$defs,
  ...native.agent.interface.methods.uploadAttachment.output.$defs,
  ...native.markdown.interface.props.$defs,
  ...native.secretInput.interface.props.$defs,
  ...session.$defs,
  ...native.agent.interface.methods.history.output.$defs,
  ...native.agent.interface.methods.observe.output.$defs,
  ...native.agent.interface.methods.source.output.$defs,
  ...native.agent.interface.methods.respond.output.$defs,
};
const themeSchema = object({
  configuration: appearanceConfigurationSchema,
  terminal: { anyOf: [{ $ref: '#/$defs/TerminalPalette' }, { type: 'null' }] },
  motion: { type: 'boolean' },
  ascii: { type: 'boolean' },
  ...Object.fromEntries(
    [
      'thinking',
      'success',
      'warning',
      'error',
      'subtle',
      'scrollbar',
      'selectionText',
      'search',
      'searchActive',
      'searchText',
    ].map((role) => [role, { type: 'integer', minimum: 0, maximum: 4294967295 }]),
  ),
  background: { type: 'integer', minimum: 0, maximum: 4294967295 },
  surface: { type: 'integer', minimum: 0, maximum: 4294967295 },
  text: { type: 'integer', minimum: 0, maximum: 4294967295 },
  muted: { type: 'integer', minimum: 0, maximum: 4294967295 },
  accent: { type: 'integer', minimum: 0, maximum: 4294967295 },
  border: { type: 'integer', minimum: 0, maximum: 4294967295 },
  selected: { type: 'integer', minimum: 0, maximum: 4294967295 },
  textarea: editor.props.properties.appearance,
  input: native.secretInput.interface.props.properties.appearance,
  menu: { $ref: '#/$defs/MenuAppearance' },
  markdown: { $ref: '#/$defs/MarkdownAppearance' },
});
const messageSchema = object(
  {
    pending: { type: 'boolean' },
    origin: native.agent.interface.methods.history.output.$defs.AgentHistoryChunk.properties.origin,
    model: native.agent.interface.methods.history.output.$defs.AgentHistoryChunk.properties.model,
    id: string,
    contentKey: { type: 'string', minLength: 1, pattern: '^[A-Za-z0-9_-]+$' },
    role: {
      enum: ['user', 'assistant', 'thinking', 'tool_call', 'tool_result', 'compaction', 'failure'],
    },
    compaction: object({ durationMs: { type: 'integer', minimum: 0, maximum: 9007199254740991 } }),
    timestamp: {
      anyOf: [{ type: 'integer', minimum: 0, maximum: 9007199254740991 }, { type: 'null' }],
    },
    text: { type: 'string' },
    tool: { anyOf: [{ $ref: '#/$defs/HistoryTool' }, { type: 'null' }] },
    continued: { type: 'boolean' },
    image: { type: 'boolean' },
    source: { anyOf: [rowPosition, { type: 'null' }] },
  },
  ['id', 'contentKey', 'role', 'timestamp', 'text', 'tool', 'continued', 'image', 'source'],
);
const withDefs = (schema: Schema, additional: Schema = {}): Schema => {
  const { $defs: local, ...body } = schema;
  const available: Record<string, unknown> = {
    ...defs,
    Theme: themeSchema,
    Message: messageSchema,
    ...additional,
    ...(local as Record<string, unknown>),
  };
  const used: Record<string, unknown> = {};
  const visit = (value: unknown): void => {
    if (!value || typeof value !== 'object') return;
    if ('$ref' in value && typeof value.$ref === 'string' && value.$ref.startsWith('#/$defs/')) {
      const name = value.$ref.slice('#/$defs/'.length);
      if (!Object.hasOwn(used, name)) {
        if (!Object.hasOwn(available, name)) throw new Error('Missing schema definition: ' + name);
        used[name] = available[name];
        visit(available[name]);
      }
    }
    for (const child of Object.values(value)) visit(child);
  };
  visit(body);
  return Object.keys(used).length ? { ...body, $defs: used } : body;
};
const component = (
  name: string,
  props: Schema,
  events: Record<string, Schema> = {},
  captures: Extract<Contract['interface'], { kind: 'component' }>['captures'] = {},
): Contract => ({
  name,
  version: 1,
  transport: 'local',
  interface: { kind: 'component', props: withDefs(props), events, captures },
});
export interface AgentScreenProps {
  languageRevision?: number;
  workhub?: boolean;
  embedded?: boolean;
  fixed?: boolean;
  session?: { request: number; id: string };
}
export const workhubRootContract = component('maka.workhub.Workspace', object({}));
export const agentScreenContract = component(
  'maka.agent.AgentScreen',
  object(
    {
      languageRevision: { type: 'integer', minimum: 0 },
      workhub: { type: 'boolean' },
      embedded: { type: 'boolean' },
      fixed: { type: 'boolean' },
      session: object({ request: { type: 'integer', minimum: 0 }, id: string }),
    },
    [],
  ),
  { workhub: object({}), languageChanged: object({}) },
);
export const paletteContract = component(
  'maka.agent.CommandPalette',
  object({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
    selectionDisabled: { type: 'boolean' },
    commands: {
      type: 'array',
      items: object(
        {
          id: { type: 'string', minLength: 1, pattern: '^[A-Za-z0-9:._-]+$' },
          label: { type: 'string', minLength: 1 },
          hint: { type: 'string' },
          disabled: { type: 'boolean' },
          copyText: { type: 'string', minLength: 1 },
        },
        ['id', 'label', 'hint', 'disabled'],
      ),
    },
  }),
  {
    invoked: object({
      id: { type: 'string', minLength: 1, pattern: '^[A-Za-z0-9:._-]+$' },
    }),
    selected: withDefs({ $ref: '#/$defs/AgentSession' }),
    closed: object({}),
  },
);
export const helpContract = component(
  'maka.agent.Help',
  object({ theme: { $ref: '#/$defs/Theme' }, locale, open: { type: 'boolean' } }),
  { closed: object({}) },
);
export const skillLibraryContract = component(
  'maka.agent.SkillLibrary',
  object(
    {
      theme: { $ref: '#/$defs/Theme' },
      locale,
      open: { type: 'boolean' },
      workspace: { anyOf: [{ $ref: '#/$defs/AgentWorkspace' }, { type: 'null' }] },
    },
    ['theme', 'locale', 'open'],
  ),
  { closed: object({}) },
);
const pageProps = (properties: Schema, required = Object.keys(properties)): Schema =>
  object(
    {
      ...properties,
      shell: object({ back: { type: 'integer', minimum: 0, maximum: Number.MAX_SAFE_INTEGER } }),
    },
    required,
  );
const pageChrome = object({
  title: { type: 'string' },
  backEnabled: { type: 'boolean' },
});

export const projectsContract = component(
  'maka.agent.Projects',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
  }),
  {
    selected: withDefs(
      object({
        project: { $ref: '#/$defs/AgentProjectRef' },
        selection: { $ref: '#/$defs/AgentDirectorySelection' },
      }),
    ),
    changed: object({}),
    chrome: pageChrome,
    closed: object({}),
  },
);
export interface AppEntry {
  id: string;
  label: string;
}
export interface AppsConfiguration {
  root: string;
  entries: AppEntry[];
}
export interface AppsProps extends ConnectionsProps {
  open: boolean;
  busy: boolean;
}
export interface ApplicationPageProps extends ConnectionsProps {
  open: boolean;
}
export const appEntrySchema = object({
  id: { type: 'string', minLength: 1, pattern: '^[A-Za-z0-9._-]+$' },
  label: { type: 'string', minLength: 1 },
});
export const appsContract = component(
  'maka.product.Apps',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
  }),
  { chrome: pageChrome, closed: object({}), manage: object({}) },
);
// An ordinary product extension point. The kernel has no application-page role.
export const applicationPageContract = component(
  'maka.product.ApplicationPage',
  object({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
  }),
  { closed: object({}) },
);
export const recallContract = component(
  'maka.agent.Recall',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
  }),
  {
    selected: withDefs(
      object({
        session: { $ref: '#/$defs/AgentSession' },
        anchor: { anyOf: [rowPosition, { type: 'null' }] },
      }),
    ),
    chrome: pageChrome,
    closed: object({}),
  },
);
export const schedulerContract = component(
  'maka.agent.Scheduler',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
    catalogEpoch: { type: 'integer', minimum: 0 },
    requested: {
      anyOf: [
        object({
          sequence: { type: 'integer', minimum: 0 },
          taskId: { type: 'string' },
        }),
        { type: 'null' },
      ],
    },
  }),
  {
    selected: withDefs({ $ref: '#/$defs/AgentSession' }),
    chrome: pageChrome,
    closed: object({}),
    connections: object({}),
  },
);
export const sessionDefaultsContract = component(
  'maka.agent.SessionDefaults',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
  }),
  { chrome: pageChrome, closed: object({}) },
);
export const networkSettingsContract = component(
  'maka.agent.NetworkSettings',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
  }),
  { chrome: pageChrome, closed: object({}) },
);
export const assistantPreferencesContract = component(
  'maka.agent.AssistantPreferences',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
  }),
  { chrome: pageChrome, closed: object({}), changed: object({}) },
);
export const webSettingsContract = component(
  'maka.agent.WebSettings',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
  }),
  { chrome: pageChrome, closed: object({}) },
);
export const jevSettingsContract = component(
  'maka.agent.JevSettings',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
  }),
  { chrome: pageChrome, closed: object({}) },
);
export const subagentSettingsContract = component(
  'maka.agent.SubagentSettings',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
  }),
  { chrome: pageChrome, closed: object({}) },
);
export interface InsightsProps extends ProjectsProps {
  scope: { sequence: number; sessionId: string | null; label: string } | null;
}
export const insightsContract = component(
  'maka.agent.Insights',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
    scope: {
      anyOf: [
        object({
          sequence: { type: 'integer', minimum: 0 },
          sessionId: { type: ['string', 'null'] },
          label: { type: 'string' },
        }),
        { type: 'null' },
      ],
    },
  }),
  { chrome: pageChrome, closed: object({}) },
);
export const importsContract = component(
  'maka.agent.Imports',
  pageProps({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
  }),
  {
    selected: withDefs(object({ session: { $ref: '#/$defs/AgentSession' } })),
    changed: object({}),
    chrome: pageChrome,
    closed: object({}),
  },
);
export const directoryPickerContract = component(
  'maka.agent.DirectoryPicker',
  object(
    {
      theme: { $ref: '#/$defs/Theme' },
      locale,
      open: { type: 'boolean' },
      busy: { type: 'boolean' },
      chooseLabel: string,
      purpose: { type: 'string', enum: ['workspace', 'import'] },
    },
    ['theme', 'locale', 'open', 'busy'],
  ),
  { selected: withDefs({ $ref: '#/$defs/AgentDirectorySelection' }), closed: object({}) },
);
export const sessionDetailsContract = component(
  'maka.agent.SessionDetails',
  object({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
    session: { anyOf: [{ $ref: '#/$defs/AgentSession' }, { type: 'null' }] },
  }),
  { changed: object({ sessionId: string }), closed: object({}) },
);
export const terminalPanelContract = component(
  'maka.agent.TerminalPanel',
  object({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
    enabled: { type: 'boolean' },
    session: { anyOf: [{ $ref: '#/$defs/AgentSession' }, { type: 'null' }] },
  }),
  { closed: object({}) },
);
export const studioContract = component('maka.studio.Workbench', object({}));
export const recoveryContract = component('maka.agent.Recovery', object({}));
export const navigationContract = component(
  'maka.agent.Navigation',
  object(
    {
      theme: { $ref: '#/$defs/Theme' },
      locale,
      sessions: { type: 'array', items: { $ref: '#/$defs/AgentSession' } },
      selected: { anyOf: [string, { type: 'null' }] },
      busy: { type: 'boolean' },
      loading: { type: 'boolean' },
      older: { type: 'boolean' },
      newer: { type: 'boolean' },
    },
    ['theme', 'locale', 'sessions', 'selected', 'busy'],
  ),
  {
    older: object({}),
    newer: object({}),
    select: object(
      {
        sessionId: string,
        details: { type: 'boolean' },
        usage: { type: 'boolean' },
        action: { enum: ['archive', 'remove', 'stop'] },
      },
      ['sessionId'],
    ),
  },
);
export const imageViewerContract = component(
  'maka.agent.ImageViewer',
  object({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    active: { type: 'boolean' },
    image: {
      anyOf: [
        object({
          sessionId: string,
          through: { type: 'integer', minimum: 1 },
          position: rowPosition,
        }),
        { type: 'null' },
      ],
    },
  }),
  { closed: object({}) },
);
const filePath = { anyOf: [{ type: 'string', minLength: 1 }, { type: 'null' }] };
export const toolDetailsContract = component(
  'maka.agent.ToolDetails',
  object(
    {
      load: { type: 'boolean' },
      theme: { $ref: '#/$defs/Theme' },
      locale,
      context: object({ sessionId: string, through: { type: 'integer', minimum: 1 } }),
      tool: { $ref: '#/$defs/HistoryTool' },
      expanded: { type: 'boolean' },
      focusOnPointer: { type: 'boolean' },
    },
    ['theme', 'locale', 'context', 'tool', 'expanded', 'focusOnPointer'],
  ),
  {
    context: object({
      sessionId: string,
      through: { type: 'integer', minimum: 1 },
      toolUseId: string,
      presentation: { anyOf: [string, { type: 'null' }] },
      call: { anyOf: [rowPosition, { type: 'null' }] },
      result: { anyOf: [rowPosition, { type: 'null' }] },
      path: filePath,
      status: { enum: ['succeeded', 'failed', 'running', 'cancelled', 'unknown', null] },
      expandable: { type: 'boolean' },
    }),
  },
);
export const timelineContract = component(
  'maka.agent.Timeline',
  object(
    {
      active: { type: 'boolean' },
      leadingPrompt: {
        anyOf: [object({ position: rowPosition, text: string }), { type: 'null' }],
      },
      activeOutput: { anyOf: [{ $ref: '#/$defs/Message' }, { type: 'null' }] },
      copyRequest: { anyOf: [{ type: 'integer', minimum: 1 }, { type: 'null' }] },
      browse: { anyOf: [{ type: 'integer', minimum: 1 }, { type: 'null' }] },
      timings: {
        type: 'array',
        items: object({
          turnId: string,
          afterMessageId: string,
          startedAt: { type: 'integer', minimum: 0, maximum: Number.MAX_SAFE_INTEGER },
          finished: { anyOf: [{ $ref: '#/$defs/HistoryTurnFinished' }, { type: 'null' }] },
        }),
      },
      markReadEnabled: { type: 'boolean' },
      toolContext: {
        anyOf: [
          object({ sessionId: string, through: { type: 'integer', minimum: 1 } }),
          { type: 'null' },
        ],
      },
      waitingTools: { type: 'array', items: string },
      liveTools: { type: 'array', uniqueItems: true, items: string },
      theme: { $ref: '#/$defs/Theme' },
      locale,
      messages: { type: 'array', items: { $ref: '#/$defs/Message' } },
      search: {
        anyOf: [
          object({
            sessionId: string,
            epoch: { type: 'integer', minimum: 0 },
            through: { type: 'integer', minimum: 0 },
          }),
          { type: 'null' },
        ],
      },
      source: {
        anyOf: [
          object({
            kind: { anyOf: [{ const: 'message' }, { $ref: '#/$defs/SourceField' }] },
            fields: {
              type: 'array',
              uniqueItems: true,
              items: { $ref: '#/$defs/SourceField' },
            },
            format: { $ref: '#/$defs/SourceFormat' },
            ready: { type: 'boolean' },
            key: string,
            text: { type: 'string' },
            atStart: { type: 'boolean' },
            atEnd: { type: 'boolean' },
            loading: { type: 'boolean' },
            failed: { type: 'boolean' },
            navigation: { type: 'integer', minimum: 1 },
            landing: { enum: ['start', 'end'] },
          }),
          { type: 'null' },
        ],
      },
      loading: { type: 'boolean' },
      older: { type: 'boolean' },
      newer: { type: 'boolean' },
      latest: { type: 'boolean' },
      landing: object({
        sequence: { type: 'integer', minimum: 0 },
        edge: { enum: ['start', 'end'] },
      }),
      live: { type: 'boolean' },
    },
    [
      'copyRequest',
      'browse',
      'timings',
      'markReadEnabled',
      'toolContext',
      'waitingTools',
      'liveTools',
      'theme',
      'locale',
      'messages',
      'search',
      'source',
      'loading',
      'older',
      'newer',
      'latest',
      'landing',
      'live',
    ],
  ),
  {
    older: object({}),
    newer: object({}),
    fill: object({}),
    prompt: object({ position: rowPosition }),
    latest: object({}),
    source: object({
      position: rowPosition,
      field: { enum: ['body', 'thinking'] },
    }),
    markRead: object({
      sessionId: string,
      position: rowPosition,
      field: { enum: ['body', 'thinking'] },
    }),
    toolSource: object({
      position: rowPosition,
      field: { enum: ['arguments', 'result'] },
    }),
    viewImage: object({ position: rowPosition }),
    sourceFirst: object({}),
    sourcePrevious: object({}),
    sourceNext: object({}),
    sourceLast: object({}),
    sourceBack: object({}),
    sourceRetry: object({}),
    sourceField: withDefs(object({ field: { $ref: '#/$defs/SourceField' } })),
    searchClosed: object({}),
    browseClosed: object({ request: { type: 'integer', minimum: 1 } }),
    messageCopy: withDefs({
      anyOf: [
        object({
          request: { type: 'integer', minimum: 1 },
          sessionId: string,
          through: { type: 'integer', minimum: 1 },
          landing: { type: 'integer', minimum: 0 },
          field: { $ref: '#/$defs/SourceField' },
          status: { enum: ['loading', 'ready', 'empty', 'unavailable', 'streaming'] },
          source: { anyOf: [{ type: 'string' }, { type: 'null' }] },
          message: { anyOf: [{ type: 'string' }, { type: 'null' }] },
        }),
        { type: 'null' },
      ],
    }),
    fileContext: {
      anyOf: [
        object({
          sessionId: string,
          through: { type: 'integer', minimum: 1 },
          landing: { type: 'integer', minimum: 0 },
          path: filePath,
        }),
        { type: 'null' },
      ],
    },
    detached: object({ landing: { type: 'integer', minimum: 0 }, value: { type: 'boolean' } }),
    tailReady: object({
      landing: { type: 'integer', minimum: 0 },
      readingRevision: { type: 'integer', minimum: 0 },
    }),
  },
  native.readingArea.interface.captures,
);
export const queueEditorContract = component(
  'maka.agent.QueueEditor',
  object({
    hasAttachments: { type: 'boolean' },
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    ready: { type: 'boolean' },
    busy: { type: 'boolean' },
    uncertain: { type: 'boolean' },
    checking: { type: 'boolean' },
    status: string,
    command: editor.props.properties.command,
  }),
  {
    save: editor.events.changed,
    changed: editor.events.changed,
    closed: object({}),
    reload: object({}),
    check: object({}),
  },
  editor.captures,
);
export const questionEditorContract = component(
  'maka.agent.QuestionEditor',
  object({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
    checking: { type: 'boolean' },
    status: string,
    record: { $ref: '#/$defs/InteractionRecord' },
  }),
  {
    closed: object({}),
    check: object({}),
    submit: withDefs(
      object({
        requestId: string,
        answers: {
          type: 'array',
          minItems: 1,
          maxItems: 3,
          items: {
            oneOf: [
              object({
                kind: { const: 'option' },
                index: { type: 'integer', minimum: 0, maximum: 2 },
              }),
              object({ kind: { const: 'skip' } }),
              object({ kind: { const: 'text' }, observation: editor.events.changed }),
            ],
          },
        },
      }),
    ),
  },
  {
    snapshot: {
      ...editor.captures.snapshot,
      input: withDefs(
        object({
          requestId: string,
          index: { type: 'integer', minimum: 0, maximum: 2 },
          snapshot: editor.captures.snapshot.input,
        }),
      ),
    },
  },
);
export const formEditorContract = component(
  'maka.agent.FormEditor',
  object({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
    checking: { type: 'boolean' },
    status: string,
    record: { $ref: '#/$defs/InteractionRecord' },
  }),
  {
    closed: object({}),
    check: object({}),
    submit: withDefs(object({ requestId: string, answer: { $ref: '#/$defs/InteractionAnswer' } })),
  },
);
export const serviceApprovalContract = component(
  'maka.agent.ServiceApproval',
  object({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
    checking: { type: 'boolean' },
    status: string,
    record: { $ref: '#/$defs/InteractionRecord' },
  }),
  {
    closed: object({}),
    check: object({}),
    submit: object({ requestId: string, decision: { enum: ['allow', 'deny'] } }),
  },
);
export const permissionApprovalContract = component(
  'maka.agent.PermissionApproval',
  object({
    theme: { $ref: '#/$defs/Theme' },
    locale,
    open: { type: 'boolean' },
    busy: { type: 'boolean' },
    checking: { type: 'boolean' },
    status: string,
    record: { $ref: '#/$defs/InteractionRecord' },
  }),
  {
    closed: object({}),
    check: object({}),
    submit: object({
      requestId: string,
      decision: { enum: ['allow', 'deny'] },
      scope: { enum: ['once', 'turn', 'session'] },
    }),
  },
);
export const uiAuthoringContract = component('maka.application.AuthoringWorker', object({}));
export const composerFooterContract = component(
  'maka.agent.ComposerFooter',
  object({
    sessionId: { anyOf: [string, { type: 'null' }] },
    dataSource: { anyOf: [{ $ref: '#/$defs/AgentDataSource' }, { type: 'null' }] },
    running: { type: 'boolean' },
    compacting: { type: 'boolean' },
    metrics: { anyOf: [{ $ref: '#/$defs/AgentSessionMetrics' }, { type: 'null' }] },
    theme: { $ref: '#/$defs/Theme' },
    locale,
  }),
);
const composerBackend = {
  anyOf: [
    object({
      kind: { const: 'model' },
      model: native.agent.interface.methods.send.input.properties.model,
      thinkingLevel: native.agent.interface.methods.send.input.properties.thinkingLevel,
    }),
    object({
      kind: { const: 'executor' },
      selection: native.agent.interface.methods.execute.input.properties.executor,
    }),
    { type: 'null' },
  ],
};
export const composerContract = component(
  'maka.agent.Composer',
  object(
    {
      skillContext: string,
      approvalPending: { type: 'boolean' },
      commands: {
        type: 'array',
        items: object({
          id: string,
          command: string,
          label: string,
          disabled: { type: 'boolean' },
        }),
      },
      backend: composerBackend,
      sendLabel: string,
      context: { anyOf: [{ type: 'string', minLength: 1 }, { type: 'null' }] },
      dataSource: { anyOf: [{ $ref: '#/$defs/AgentDataSource' }, { type: 'null' }] },
      attachments: {
        type: 'array',
        items: object({
          ...native.agent.interface.methods.uploadAttachment.output.$defs.AgentAttachment
            .properties,
          dataSource: { $ref: '#/$defs/AgentDataSource' },
        }),
      },
      filePaste: editor.props.properties.filePaste,
      fileCommand: editor.props.properties.fileCommand,
      autoFocus: { type: 'boolean' },
      theme: { $ref: '#/$defs/Theme' },
      locale,
      busy: { type: 'boolean' },
      running: { type: 'boolean' },
      enabled: { type: 'boolean' },
    },
    [
      'backend',
      'context',
      'dataSource',
      'attachments',
      'filePaste',
      'fileCommand',
      'autoFocus',
      'theme',
      'locale',
      'busy',
      'running',
      'enabled',
    ],
  ),
  {
    submission: withDefs(
      object({
        context: string,
        dataSource: { $ref: '#/$defs/AgentDataSource' },
        key: string,
        text: { type: 'string' },
        attachments: { type: 'array', items: string },
        status: { enum: ['pending', 'unknown', 'accepted', 'removed'] },
        messageId: { anyOf: [string, { type: 'null' }] },
      }),
    ),
    sent: withDefs(
      object({
        context: string,
        dataSource: { $ref: '#/$defs/AgentDataSource' },
        attachments: { type: 'array', items: string },
      }),
    ),
    command: withDefs(
      object({ context: string, dataSource: { $ref: '#/$defs/AgentDataSource' }, id: string }),
    ),
    setup: object({ context: string }),
    readiness: withDefs({
      type: 'array',
      items: object({
        context: string,
        dataSource: { $ref: '#/$defs/AgentDataSource' },
        ready: { type: 'boolean' },
      }),
    }),
    restored: withDefs(
      object({
        files: {
          type: 'array',
          items: object({
            ...native.agent.interface.methods.uploadAttachment.output.$defs.AgentAttachment
              .properties,
            dataSource: { $ref: '#/$defs/AgentDataSource' },
          }),
        },
      }),
    ),
    files: withDefs(
      object({
        ...editor.events.files.properties,
        dataSource: { $ref: '#/$defs/AgentDataSource' },
      }),
    ),
    stop: object({}),
    attachments: object({}),
  },
  editor.captures,
);
export const themeContract: Contract = {
  name: 'maka.agent.Appearance',
  version: 1,
  transport: 'local',
  interface: { kind: 'value', schema: withDefs(themeSchema) },
};
export type ComposerObservation = EditorObservation;

const modelDefs = native.settings.interface.methods.models.output.$defs;
const preferredModel = {
  anyOf: [
    object({
      connectionId: string,
      modelId: string,
      thinkingLevel: { anyOf: [{ $ref: '#/$defs/ThinkingLevel' }, { type: 'null' }] },
    }),
    { type: 'null' },
  ],
};
const selectedModel = object({
  choice: { $ref: '#/$defs/SettingsModelChoice' },
  thinkingLevel: { anyOf: [{ $ref: '#/$defs/ThinkingLevel' }, { type: 'null' }] },
});
export const modelsContract: Contract = {
  name: 'maka.agent.Models',
  version: 1,
  transport: 'local',
  interface: {
    kind: 'component',
    props: {
      ...withDefs(
        object(
          {
            suggestDefault: { type: 'boolean' },
            selectionKey: string,
            preferredModel,
            catalogEpoch: { type: 'integer', minimum: 0 },
            openRequest: { anyOf: [{ type: 'integer', minimum: 0 }, { type: 'null' }] },
            theme: { $ref: '#/$defs/Theme' },
            locale,
            selection: { anyOf: [selectedModel, { type: 'null' }] },
            busy: { type: 'boolean' },
          },
          ['catalogEpoch', 'openRequest', 'theme', 'locale', 'selection', 'busy'],
        ),
        modelDefs,
      ),
    },
    events: {
      selected: { ...selectedModel, $defs: modelDefs },
      resolved: {
        ...object({
          selectionKey: string,
          selection: { anyOf: [selectedModel, { type: 'null' }] },
        }),
        $defs: modelDefs,
      },
      setup: object({
        catalogEpoch: { type: 'integer', minimum: 0 },
        kind: { enum: ['loading', 'unavailable', 'connect', 'choose'] },
      }),
      connections: { type: 'null' },
    },
    captures: {},
  },
};

const executorDefs = native.agent.interface.methods.execute.input.$defs;
export const executionPickerContract: Contract = {
  name: 'maka.agent.ExecutionPicker',
  version: 1,
  transport: 'local',
  interface: {
    kind: 'component',
    props: withDefs(
      object(
        {
          selectionKey: string,
          preferredModel,
          suggestDefault: { type: 'boolean' },
          catalogEpoch: { type: 'integer', minimum: 0 },
          openRequest: { anyOf: [{ type: 'integer', minimum: 0 }, { type: 'null' }] },
          theme: { $ref: '#/$defs/Theme' },
          locale,
          selection: { anyOf: [selectedModel, { type: 'null' }] },
          executor: { anyOf: [{ $ref: '#/$defs/AgentExecutorSelection' }, { type: 'null' }] },
          busy: { type: 'boolean' },
        },
        ['catalogEpoch', 'openRequest', 'theme', 'locale', 'selection', 'executor', 'busy'],
      ),
      { ...modelDefs, ...executorDefs },
    ),
    events: {
      selected: { ...selectedModel, $defs: modelDefs },
      resolved: {
        ...object({
          selectionKey: string,
          selection: { anyOf: [selectedModel, { type: 'null' }] },
        }),
        $defs: modelDefs,
      },
      setup: object({
        catalogEpoch: { type: 'integer', minimum: 0 },
        kind: { enum: ['loading', 'unavailable', 'connect', 'choose'] },
      }),
      connections: { type: 'null' },
      executor: {
        anyOf: [{ $ref: '#/$defs/AgentExecutorSelection' }, { type: 'null' }],
        $defs: executorDefs,
      },
      backends: object({ available: { type: 'boolean' } }),
    },
    captures: {},
  },
};

export const modelDefaultContract: Contract = {
  name: 'maka.agent.ModelDefault',
  version: 1,
  transport: 'local',
  interface: {
    kind: 'component',
    props: {
      ...withDefs(
        object({
          theme: { $ref: '#/$defs/Theme' },
          locale,
          selection: { anyOf: [{ $ref: '#/$defs/SettingsModelChoice' }, { type: 'null' }] },
          defaultModel: { anyOf: [{ $ref: '#/$defs/SettingsDefaultModel' }, { type: 'null' }] },
          catalogRevision: { anyOf: [{ type: 'integer', minimum: 0 }, { type: 'null' }] },
          busy: { type: 'boolean' },
        }),
        modelDefs,
      ),
    },
    events: { changed: object({}) },
    captures: {},
  },
};
export interface ConnectionsProps extends PageShellProps {
  setupRequest?: number | null;
  theme: Theme;
  locale: string;
}
export interface DeploymentProps extends ConnectionsProps {
  bookmarkKey?: string;
  navigationReady?: boolean;
}
export interface CredentialProps extends ConnectionsProps {
  target: ConnectionCredentialTarget;
}
export interface ConnectionFormProps extends ConnectionsProps {
  removeRequested?: boolean;
  catalogRevision: number;
  existingSlugs?: string[];
  connection: Extract<SettingsConnectionDetail, { kind: 'ready' }> | null;
}
export const connectionFormContract: Contract = {
  name: 'maka.agent.ConnectionForm',
  version: 1,
  transport: 'local',
  interface: {
    kind: 'component',
    props: {
      ...pageProps(
        {
          removeRequested: { type: 'boolean' },
          existingSlugs: { type: 'array', items: string },
          theme: { $ref: '#/$defs/Theme' },
          locale,
          catalogRevision: { type: 'integer', minimum: 0 },
          connection: {
            anyOf: [native.settings.interface.methods.connection.output.oneOf[1], { type: 'null' }],
          },
        },
        ['theme', 'locale', 'catalogRevision', 'connection'],
      ),
      $defs: {
        ...defs,
        ...native.settings.interface.methods.connection.output.$defs,
        Theme: themeSchema,
      },
    },
    events: {
      chrome: pageChrome,
      closed: object({}),
      changed: object({ created: native.settings.interface.methods.connection.input }, []),
    },
    captures: {},
  },
};
export const credentialContract: Contract = {
  name: 'maka.agent.Credential',
  version: 1,
  transport: 'local',
  interface: {
    kind: 'component',
    props: {
      ...pageProps({
        theme: { $ref: '#/$defs/Theme' },
        locale,
        target: { $ref: '#/$defs/ConnectionCredentialTarget' },
      }),
      $defs: {
        ...defs,
        ...native.settings.interface.methods.saveCredential.input.$defs,
        Theme: themeSchema,
      },
    },
    events: { chrome: pageChrome, closed: object({}), changed: object({}), completed: object({}) },
    captures: {},
  },
};
export const connectionsContract = component(
  'maka.agent.Connections',
  pageProps(
    {
      theme: { $ref: '#/$defs/Theme' },
      locale,
      setupRequest: { type: ['integer', 'null'], minimum: 0 },
    },
    ['theme', 'locale'],
  ),
  { chrome: pageChrome, closed: object({}), changed: object({}), completed: object({}) },
);

export const deploymentContract = component(
  'maka.agent.Deployment',
  pageProps(
    {
      theme: { $ref: '#/$defs/Theme' },
      locale,
      bookmarkKey: string,
      navigationReady: { type: 'boolean' },
    },
    ['theme', 'locale'],
  ),
  { chrome: pageChrome, closed: object({}) },
);

export { appearanceConfigurationSchema } from './terminal-theme.js';
