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

import {
  contracts,
  type AgentConfiguration,
  type AcpExecutorConfiguration,
  type SkillLibraryConfiguration,
  type WorkspaceProcessConfiguration,
  type WorkspaceWebConfiguration,
  type ComputerConfiguration,
  type WorkspaceJevConfiguration,
  type CompositionInput,
  type CompositionNode,
  type PackageRef,
  type RemoteObservation,
  type Target as BindingTarget,
} from '../../../../packages/plugin-sdk/src/index.js';
import { welcomeMark } from '../../../../plugins/shared/welcome.js';
export { installation } from './installation.js';
export const packageNames = [
  'agent-screen',
  'workhub-root',
  'workhub',
  'recovery',
  'command-palette',
  'execution-picker',
  'directory-picker',
  'projects',
  'conversation-import',
  'assistant-preferences',
  'network-settings',
  'session-defaults',
  'web-settings',
  'jev-settings',
  'subagent-settings',
  'insights',
  'recall',
  'scheduler',
  'apps',
  'skill-library',
  'skills',
  'skill-outcomes',
  'session-details',
  'queue-editor',
  'question-editor',
  'form-editor',
  'forms',
  'service-approval',
  'permission-approval',
  'questions',
  'workspace-web',
  'workspace-computer',
  'workspace-jev',
  'workspace-files',
  'workspace-processes',
  'protocol-processes',
  'acp-executor',
  'help',
  'studio',
  'studio-gateway',
  'recovery-gateway',
  'deployment',
  'deployment-settings',
  'host-deployment-manager',
  'host-logs',
  'deployment-manager',
  'deployment-gateway',
  'image-viewer',
  'composer',
  'composer-footer',
  'ui-authoring',
  'ui-authoring-gateway',
  'navigation',
  'timeline',
  'tool-details',
  'diff',
  'markdown',
  'markdown-text',
  'source-text',
  'terminal-view',
  'terminal-panel',
  'reading-area',
  'label',
  'rows',
  'checkpoint',
  'text-file',
  'morph-mark',
  'appearance',
  'locale',
  'surface',
  'editor',
  'file',
  'reader',
  'history',
  'models',
  'model-default',
  'settings',
  'settings-authentication',
  'settings-setup',
  'connections',
  'credential',
  'connection-form',
  'input',
  'agent',
  'receipts',
  'gateway',
] as const;
export type PackageName = (typeof packageNames)[number];
const baseInstanceNames = [
  ...packageNames,
  'recovery-reading',
  'ui-authoring-worker',
  'interface-deployment',
  'service-deployment',
  'host-deployment-gateway',
] as const;
export const instanceNames = [
  ...baseInstanceNames,
  ...baseInstanceNames.map((name): `coordinator-${typeof name}` => `coordinator-${name}`),
] as const;
export type InstanceName = (typeof instanceNames)[number];
type ExecutorInstanceName = `acp-${string}` | `coordinator-acp-${string}`;
import type { AppearanceConfiguration } from '../../../../plugins/shared/terminal-theme.js';
export interface AcpExecutor {
  name: string;
  configuration: AcpExecutorConfiguration;
}
export interface CompositionConfiguration {
  presentation: 'tui';
  locale: string;
  scheme: 'light' | 'dark';
  appearance?: Omit<AppearanceConfiguration, 'scheme'>;
  /** Fixed local custom-theme import/export file; absent disables file operations. */
  themeFile?: string;
  packages: Record<PackageName, PackageRef>;
  keys: Record<InstanceName, string> & Partial<Record<ExecutorInstanceName, string>>;
  agent: AgentConfiguration;
  processes?: WorkspaceProcessConfiguration;
  web?: WorkspaceWebConfiguration;
  computer?: ComputerConfiguration;
  jev?: WorkspaceJevConfiguration;
  coordinator?: AgentConfiguration;
  coordinatorProcesses?: WorkspaceProcessConfiguration;
  acpExecutors?: AcpExecutor[];
  executorProcesses?: WorkspaceProcessConfiguration;
  coordinatorAcpExecutors?: AcpExecutor[];
  coordinatorExecutorProcesses?: WorkspaceProcessConfiguration;
  skills?: SkillLibraryConfiguration;
  hostRoot: string;
  hostDeployment: string;
  managementDeployment: string;
}
/** The launcher assigns identities without knowing the product's instance structure. */
export function installationNames(
  configuration: Pick<CompositionConfiguration, 'acpExecutors' | 'coordinatorAcpExecutors'>,
): string[] {
  return [
    ...new Set([
      ...instanceNames,
      ...(configuration.acpExecutors ?? []).map((e) => `acp-${e.name}`),
      ...(configuration.coordinatorAcpExecutors ?? []).map((e) => `coordinator-acp-${e.name}`),
    ]),
  ];
}
interface ApplicationSources {
  host: CompositionInput;
  managementHost: CompositionInput;
  ui: CompositionInput;
  remoteExports: RemoteObservation[];
  connections: unknown;
}
/** Shared ordinary settings/management graph; each product adds its own root and capabilities. */
function settingsComposition(
  configuration: Omit<CompositionConfiguration, 'agent'>,
  gateway: 'gateway' | 'studio-gateway',
) {
  const { packages, keys } = configuration;
  const local = (name: InstanceName, exportName: string): BindingTarget => ({
    kind: 'instance',
    path: [name],
    export: exportName,
  });
  const remote = (
    name:
      | 'ui-authoring'
      | 'agent'
      | 'settings'
      | 'settings-authentication'
      | 'settings-setup'
      | 'receipts'
      | 'workspace-processes'
      | 'workspace-web'
      | 'workspace-jev'
      | 'skills'
      | 'skill-outcomes'
      | 'host-logs'
      | 'host-deployment-manager',
    exportName: string,
  ): BindingTarget => ({
    kind: 'remote',
    deployment: 'local',
    instance: [keys[name]],
    export: exportName,
  });
  const node = (
    name: PackageName,
    values: Record<string, unknown> = {},
    bindings: Record<string, BindingTarget[]> = {},
    identity: InstanceName = name,
  ): CompositionNode => ({
    key: keys[identity],
    package: { source: 'pinned', package: packages[name] },
    usePackage: 'entry',
    configuration: { values },
    bindings,
  });
  const host: CompositionInput = {
    environment: { location: 'host' },
    instances: {
      'host-deployment-manager': node('host-deployment-manager'),
      ...(gateway === 'gateway' ? { 'host-logs': node('host-logs') } : {}),
      'host-deployment-gateway': node(
        'deployment-gateway',
        {},
        {
          management: [local('host-deployment-manager', 'Management')],
          receipts: [local('receipts', 'Receipts')],
        },
        'host-deployment-gateway',
      ),
      settings: node('settings', { locale: configuration.locale }),
      'settings-authentication': node('settings-authentication'),
      'settings-setup': node('settings-setup'),
      'skill-outcomes': node('skill-outcomes'),
      receipts: node('receipts'),
      'recovery-gateway': node(
        'recovery-gateway',
        {},
        {
          skillOutcomes: [local('skill-outcomes', 'Outcomes')],
          receipts: [local('receipts', 'Receipts')],
        },
      ),
      gateway: node(
        gateway,
        {},
        {
          ...(gateway === 'gateway' ? { hostLogs: [local('host-logs', 'HostLogs')] } : {}),
          settings: [local('settings', 'Settings')],
          authentication: [local('settings-authentication', 'Authentication')],
          setup: [local('settings-setup', 'Setup')],
        },
      ),
    },
  };
  const managementHost: CompositionInput = {
    environment: { location: 'host' },
    instances: {
      management: node('deployment-manager'),
      receipts: node('receipts'),
      gateway: node(
        'deployment-gateway',
        {},
        {
          management: [{ kind: 'instance', path: ['management'], export: 'Management' }],
          receipts: [local('receipts', 'Receipts')],
        },
      ),
    },
  };
  const ui: CompositionInput = {
    environment: { location: 'ui', presentation: configuration.presentation },
    instances: {
      recovery: node(
        'recovery',
        { locale: configuration.locale },
        {
          deployment: [local('deployment', 'Deployment')],
          history: [local('history', 'DeploymentHistory')],
          skillOutcomes: [remote('skill-outcomes', 'Outcomes')],
          receipts: [remote('receipts', 'Receipts')],
          managementReceipts: [
            {
              kind: 'remote',
              deployment: 'manager',
              instance: [keys.receipts],
              export: 'Receipts',
            },
          ],
          appearance: [local('appearance', 'Appearance')],
          locale: [local('locale', 'Locale')],
          reading: [local('recovery-reading', 'ReadingArea')],
          surface: [local('surface', 'Surface')],
        },
      ),
      checkpoint: node('checkpoint'),
      'source-text': node('source-text'),
      deployment: node(
        'deployment-settings',
        {},
        {
          checkpoint: [local('checkpoint', 'Checkpoint')],
          interface: [local('interface-deployment', 'Deployment')],
          services: [local('service-deployment', 'Deployment')],
        },
        'deployment',
      ),
      'service-deployment': node(
        'deployment',
        {},
        {
          checkpoint: [local('checkpoint', 'Checkpoint')],
          input: [local('input', 'TextInput')],
          reading: [local('recovery-reading', 'ReadingArea')],
          editor: [local('editor', 'Textarea')],
          reader: [local('reader', 'Reader')],
          file: [local('file', 'FilePicker')],
          management: [remote('host-deployment-manager', 'Management')],
          receipts: [remote('receipts', 'Receipts')],
          history: [local('history', 'History')],
        },
        'service-deployment',
      ),
      'interface-deployment': node(
        'deployment',
        {},
        {
          checkpoint: [local('checkpoint', 'Checkpoint')],
          input: [local('input', 'TextInput')],
          reading: [local('recovery-reading', 'ReadingArea')],
          editor: [local('editor', 'Textarea')],
          reader: [local('reader', 'Reader')],
          file: [local('file', 'FilePicker')],
          management: [
            {
              kind: 'remote',
              deployment: 'manager',
              instance: [keys['deployment-manager']],
              export: 'Management',
            },
          ],
          receipts: [
            {
              kind: 'remote',
              deployment: 'manager',
              instance: [keys.receipts],
              export: 'Receipts',
            },
          ],
          history: [local('history', 'History')],
        },
        'interface-deployment',
      ),
      'assistant-preferences': node(
        'assistant-preferences',
        {},
        {
          agent: [remote('agent', 'agent')],
          settings: [remote('settings', 'Settings')],
          input: [local('input', 'TextInput')],
          editor: [local('editor', 'Textarea')],
          reader: [local('reader', 'Reader')],
          receipts: [remote('receipts', 'Receipts')],
          history: [local('history', 'History')],
        },
      ),
      'network-settings': node(
        'network-settings',
        {},
        {
          settings: [remote('settings', 'Settings')],
          input: [local('input', 'TextInput')],
          secret: [local('input', 'SecretInput')],
          reader: [local('reader', 'Reader')],
          receipts: [remote('receipts', 'Receipts')],
          history: [local('history', 'History')],
        },
      ),
      insights: node(
        'insights',
        {},
        {
          checkpoint: [local('checkpoint', 'Checkpoint')],
          settings: [remote('settings', 'Settings')],
          input: [local('input', 'TextInput')],
          reader: [local('reader', 'Reader')],
          receipts: [remote('receipts', 'Receipts')],
          history: [local('history', 'History')],
        },
      ),
      connections: node(
        'connections',
        {},
        {
          settings: [remote('settings', 'Settings')],
          credential: [local('credential', 'Credential')],
          form: [local('connection-form', 'ConnectionForm')],
          input: [local('input', 'TextInput')],
          reader: [local('reader', 'Reader')],
          reading: [local('reading-area', 'ReadingArea')],
        },
      ),
      credential: node(
        'credential',
        {},
        {
          settings: [remote('settings', 'Settings')],
          reading: [local('recovery-reading', 'ReadingArea')],
          authentication: [remote('settings-authentication', 'Authentication')],
          setup: [remote('settings-setup', 'Setup')],
          secret: [local('input', 'SecretInput')],
          receipts: [remote('receipts', 'Receipts')],
          history: [local('history', 'History')],
        },
      ),
      input: node('input'),
      'connection-form': node(
        'connection-form',
        {},
        {
          reading: [local('reading-area', 'ReadingArea')],
          settings: [remote('settings', 'Settings')],
          authentication: [remote('settings-authentication', 'Authentication')],
          input: [local('input', 'TextInput')],
          editor: [local('editor', 'Textarea')],
          reader: [local('reader', 'Reader')],
          receipts: [remote('receipts', 'Receipts')],
          history: [local('history', 'History')],
        },
      ),
      models: node(
        'models',
        {},
        {
          settings: [remote('settings', 'Settings')],
          defaults: [local('model-default', 'ModelDefault')],
          reading: [local('reading-area', 'ReadingArea')],
          input: [local('input', 'TextInput')],
          reader: [local('reader', 'Reader')],
        },
      ),
      'model-default': node(
        'model-default',
        {},
        {
          input: [local('input', 'TextInput')],
          reader: [local('reader', 'Reader')],
          reading: [local('recovery-reading', 'ReadingArea')],
          settings: [remote('settings', 'Settings')],
          receipts: [remote('receipts', 'Receipts')],
          history: [local('history', 'History')],
        },
      ),
      appearance: node('appearance', { scheme: configuration.scheme, ...configuration.appearance }),
      locale: node('locale'),
      surface: node('surface'),
      editor: node('editor'),
      file: node('file'),
      reader: node('reader'),
      'reading-area': node('reading-area', {}, { source: [local('source-text', 'SourceText')] }),
      'recovery-reading': node(
        'reading-area',
        {},
        { source: [local('source-text', 'SourceText')] },
        'recovery-reading',
      ),
      history: node('history'),
    },
    recovery: local('recovery', 'Recovery'),
  };
  const remoteExports: RemoteObservation[] = [
    {
      capability: { deployment: 'local', instance: [keys['skill-outcomes']], export: 'Outcomes' },
      declaration: { contract: contracts.skillOutcomes, availability: 'required' },
    },
    {
      capability: { deployment: 'local', instance: [keys.receipts], export: 'Receipts' },
      declaration: { contract: contracts.receiptReader, availability: 'required' },
    },
  ];
  remoteExports.push({
    capability: {
      deployment: 'local',
      instance: [keys['host-deployment-manager']],
      export: 'Management',
    },
    declaration: { contract: contracts.deploymentManager, availability: 'required' },
  });
  remoteExports.push({
    capability: { deployment: 'local', instance: [keys.settings], export: 'Settings' },
    declaration: { contract: contracts.settings, availability: 'required' },
  });
  remoteExports.push({
    capability: {
      deployment: 'local',
      instance: [keys['settings-authentication']],
      export: 'Authentication',
    },
    declaration: { contract: contracts.settingsAuthentication, availability: 'required' },
  });
  remoteExports.push({
    capability: {
      deployment: 'local',
      instance: [keys['settings-setup']],
      export: 'Setup',
    },
    declaration: { contract: contracts.settingsSetup, availability: 'required' },
  });
  remoteExports.push(
    {
      capability: {
        deployment: 'manager',
        instance: [keys['deployment-manager']],
        export: 'Management',
      },
      declaration: { contract: contracts.deploymentManager, availability: 'required' },
    },
    {
      capability: { deployment: 'manager', instance: [keys.receipts], export: 'Receipts' },
      declaration: { contract: contracts.receiptReader, availability: 'required' },
    },
  );
  const expose = (consumer: InstanceName, name: string, methods: string[], recovery: boolean) => ({
    consumer: [keys[consumer]],
    import: name,
    grant: { methods, recovery },
  });
  const connections = {
    hosts: [
      {
        name: 'manager',
        location: { kind: 'cohost' },
        deployment: configuration.managementDeployment,
        endpoints: {
          management: expose(
            'deployment-gateway',
            'management',
            [
              'read',
              'instances',
              'setEnabled',
              'readSettings',
              'configure',
              'bindingCandidates',
              'packageSettings',
              'editComposition',
              'readDocument',
              'replace',
              'install',
              'installSource',
              'installUrl',
              'readPackage',
              'exportPackage',
              'replaceSource',
            ],
            false,
          ),
          receipts: expose('deployment-gateway', 'receipts', ['read'], true),
        },
      },
      {
        name: 'local',
        location: { kind: 'public', root: configuration.hostRoot },
        deployment: configuration.hostDeployment,
        endpoints: {
          management: expose(
            'host-deployment-gateway',
            'management',
            [
              'read',
              'instances',
              'setEnabled',
              'readSettings',
              'configure',
              'bindingCandidates',
              'packageSettings',
              'editComposition',
              'readDocument',
              'replace',
              'install',
              'installSource',
              'installUrl',
              'readPackage',
              'exportPackage',
              'replaceSource',
            ],
            false,
          ),
          receipts: expose('recovery-gateway', 'receipts', ['read'], true),
          settings: expose(
            gateway,
            'settings',
            Object.keys(contracts.settings.interface.methods),
            true,
          ),
          authentication: expose(
            gateway,
            'authentication',
            Object.keys(contracts.settingsAuthentication.interface.methods),
            true,
          ),
          setup: expose(
            gateway,
            'setup',
            Object.keys(contracts.settingsSetup.interface.methods),
            true,
          ),
          skillOutcomes: expose('recovery-gateway', 'skillOutcomes', ['read'], true),
        },
      },
    ],
  };
  return { host, managementHost, ui, remoteExports, connections, local, remote, node, expose };
}
/** Product source assembled exclusively from ordinary checked plugin bindings.
 * Keys and the Host deployment belong to the installation, not to a build.
 */
export function composition(configuration: CompositionConfiguration): ApplicationSources {
  return configuration.coordinator
    ? workhubComposition(configuration, 'sessions')
    : agentComposition(configuration);
}

function agentComposition(configuration: CompositionConfiguration): ApplicationSources {
  const { keys } = configuration;
  const { host, managementHost, ui, remoteExports, connections, local, remote, node, expose } =
    settingsComposition(configuration, 'gateway');
  const executors = configuration.acpExecutors ?? [];
  if (
    new Set(executors.map((e) => e.name)).size !== executors.length ||
    executors.some((e) => !/^[a-zA-Z][a-zA-Z0-9._:-]{0,127}$/.test(e.name))
  )
    throw new Error('ACP backends require valid distinct executor names');
  if (configuration.agent.executors?.length)
    throw new Error('The standard composition selects executor imports through acpExecutors');
  if (executors.length && !configuration.executorProcesses)
    throw new Error('ACP backends require an explicit executorProcesses installation ceiling');
  const executorPath = (name: string): ExecutorInstanceName => `acp-${name}`;
  const computerMethods = [
    ...new Set(
      configuration.agent.tools
        .filter((tool) => tool.import === 'computer')
        .map((tool) => tool.method),
    ),
  ];
  if (computerMethods.length && !configuration.computer)
    throw new Error('Computer tools require an explicit native installation');
  if (computerMethods.some((method) => !['evaluate', 'reset'].includes(method)))
    throw new Error('Model Computer tools select only evaluate and reset');
  if (computerMethods.length)
    host.instances['workspace-computer'] = node('workspace-computer', {
      ...configuration.computer,
    });
  const webMethods = [
    ...new Set(
      configuration.agent.tools.filter((tool) => tool.import === 'web').map((tool) => tool.method),
    ),
  ];
  if (webMethods.length && !configuration.web)
    throw new Error('Web tools require an explicit network installation ceiling');
  if (webMethods.some((method) => method !== 'fetch' && method !== 'search'))
    throw new Error('Model Web tools select only fetch and search');
  const asksForms = configuration.agent.tools.some((tool) => tool.import === 'forms');
  const asksQuestions = configuration.agent.tools.some((tool) => tool.import === 'questions');
  const fileMethods = [
    ...new Set(
      configuration.agent.tools
        .filter((tool) => tool.import === 'files')
        .map((tool) => tool.method),
    ),
  ];
  const processMethods = [
    ...new Set(
      configuration.agent.tools
        .filter((tool) => tool.import === 'processes')
        .map((tool) => tool.method),
    ),
  ];
  const skillMethods = [
    ...new Set(
      configuration.agent.tools
        .filter((tool) => tool.import === 'skills')
        .map((tool) => tool.method),
    ),
  ];
  if (configuration.agent.skillLibrary != null && configuration.agent.skillLibrary !== 'skills')
    throw new Error('Skill input preparation selects the skills Library import');
  const skillCalls = [
    ...new Set([
      ...skillMethods,
      ...(configuration.agent.skillLibrary ? ['prepare', 'search'] : []),
    ]),
  ];
  if (skillCalls.length && !configuration.skills)
    throw new Error('Skill tools require explicitly selected skill collections');
  if (skillMethods.some((method) => method !== 'search' && method !== 'load'))
    throw new Error('Model skill tools select only search and load');
  if (processMethods.length && !configuration.processes)
    throw new Error(
      'Process tools require an explicit installation filesystem and network ceiling',
    );
  host.instances.agent = node(
    'agent',
    {
      ...configuration.agent,
      executors: executors.map((e, member) => ({
        name: e.name,
        import: 'executors',
        member,
        ...(configuration.executorProcesses?.setupDirectory
          ? { setup: { import: 'executorSetups', member } }
          : {}),
      })),
      // These imports bind known native v1 contracts. Freeze presentation in
      // the configuration; tool names and response shapes cannot opt into it.
      tools: configuration.agent.tools.map((tool) => {
        if (tool.import === 'web') return { ...tool, input: 'web' as const };
        if (tool.import === 'skills') return { ...tool, input: 'skill' as const };
        if (tool.import === 'processes' && tool.method === 'execute')
          tool = { ...tool, input: 'shell' };
        if (tool.presentation !== undefined) return tool;
        const contract =
          tool.import === 'files' &&
          ['readText', 'glob', 'grep', 'write', 'edit', 'applyPatch'].includes(tool.method)
            ? 'maka.workspace.Files'
            : tool.import === 'processes' &&
                ['execute', 'read', 'writeStdin', 'stop'].includes(tool.method)
              ? 'maka.workspace.Processes'
              : null;
        return contract ? { ...tool, presentation: `${contract}@1/${tool.method}` } : tool;
      }),
    },
    {
      ...(executors.length
        ? {
            ...(configuration.executorProcesses?.setupDirectory
              ? {
                  executorSetups: executors.map((e) => ({
                    kind: 'instance' as const,
                    path: [executorPath(e.name)],
                    export: 'setup',
                  })),
                }
              : {}),
            executors: executors.map((e) => ({
              kind: 'instance' as const,
              path: [executorPath(e.name)],
              export: 'executor',
            })),
          }
        : {}),
      ...(computerMethods.length ? { computer: [local('workspace-computer', 'computer')] } : {}),
      ...(webMethods.length ? { web: [local('workspace-web', 'web')] } : {}),
      ...(skillCalls.length ? { skills: [local('skills', 'skills')] } : {}),
      ui: [local('ui-authoring', 'Authoring')],
      ...(asksForms ? { forms: [local('forms', 'forms')] } : {}),
      ...(asksQuestions ? { questions: [local('questions', 'questions')] } : {}),
      ...(fileMethods.length ? { files: [local('workspace-files', 'files')] } : {}),
      ...(processMethods.length ? { processes: [local('workspace-processes', 'processes')] } : {}),
    },
  );
  host.instances['ui-authoring'] = node('ui-authoring');
  host.instances['ui-authoring-gateway'] = node(
    'ui-authoring-gateway',
    {},
    { requests: [local('ui-authoring', 'Authoring')] },
  );
  remoteExports.push({
    capability: { deployment: 'local', instance: [keys['ui-authoring']], export: 'Authoring' },
    declaration: { contract: contracts.uiAuthoring, availability: 'required' },
  });
  Object.assign(connections.hosts.find((host) => host.name === 'local')!.endpoints, {
    uiAuthoring: expose('ui-authoring-gateway', 'requests', ['watch', 'claim', 'complete'], true),
  });
  if (asksForms || executors.length) host.instances.forms = node('forms');
  if (asksQuestions) host.instances.questions = node('questions');
  if (fileMethods.length || executors.length)
    host.instances['workspace-files'] = node('workspace-files');
  if (executors.length) {
    host.instances['protocol-processes'] = node('protocol-processes', {
      ...configuration.executorProcesses!,
      sessionOwner: [keys.agent],
      sessionKind: 'agent',
    });
    for (const executor of executors) {
      host.instances[executorPath(executor.name)] = {
        ...node(
          'acp-executor',
          { ...executor.configuration },
          {
            processes: [local('protocol-processes', 'protocol')],
            files: [local('workspace-files', 'files')],
            forms: [local('forms', 'forms')],
          },
        ),
        key: keys[executorPath(executor.name)]!,
      };
    }
  }
  if (configuration.processes)
    host.instances['workspace-processes'] = node(
      'workspace-processes',
      {
        ...configuration.processes,
        sessionOwner: [keys.agent],
        sessionKind: 'agent',
      },
      configuration.web && processMethods.includes('health')
        ? { web: [local('workspace-web', 'web')] }
        : {},
    );
  host.instances.gateway!.bindings!.agent = [local('agent', 'agent')];
  if (configuration.processes)
    host.instances.gateway!.bindings!.terminal = [local('workspace-processes', 'terminal')];
  ui.instances['session-defaults'] = node(
    'session-defaults',
    { instance: [keys.agent] },
    {
      management: [remote('host-deployment-manager', 'Management')],
      receipts: [remote('receipts', 'Receipts')],
      history: [local('history', 'History')],
    },
  );
  if (configuration.web) {
    ui.instances['web-settings'] = node(
      'web-settings',
      {},
      {
        web: [remote('workspace-web', 'web')],
        secret: [local('input', 'SecretInput')],
        receipts: [remote('receipts', 'Receipts')],
        history: [local('history', 'History')],
      },
    );
    host.instances['workspace-web'] = node('workspace-web', { ...configuration.web });
    host.instances.gateway!.bindings!.web = [local('workspace-web', 'web')];
    remoteExports.push({
      capability: { deployment: 'local', instance: [keys['workspace-web']], export: 'web' },
      declaration: { contract: contracts.workspaceWeb, availability: 'required' },
    });
    Object.assign(connections.hosts.find((host) => host.name === 'local')!.endpoints, {
      web: expose('gateway', 'web', ['snapshot', 'save', 'saveKey', 'deleteKey', 'checkKey'], true),
    });
  }
  if (configuration.jev) {
    ui.instances['jev-settings'] = node(
      'jev-settings',
      {},
      {
        jev: [remote('workspace-jev', 'jev')],
        secret: [local('input', 'SecretInput')],
        input: [local('input', 'TextInput')],
        reader: [local('reader', 'Reader')],
        receipts: [remote('receipts', 'Receipts')],
        history: [local('history', 'History')],
      },
    );
    host.instances['workspace-jev'] = node('workspace-jev', { ...configuration.jev });
    host.instances.gateway!.bindings!.jev = [local('workspace-jev', 'jev')];
    remoteExports.push({
      capability: { deployment: 'local', instance: [keys['workspace-jev']], export: 'jev' },
      declaration: { contract: contracts.workspaceJev, availability: 'required' },
    });
    Object.assign(connections.hosts.find((host) => host.name === 'local')!.endpoints, {
      jev: expose('gateway', 'jev', ['snapshot', 'save', 'credential', 'evaluate'], true),
    });
  }
  if (configuration.skills) {
    host.instances.skills = node('skills', { ...configuration.skills });
    host.instances.gateway!.bindings!.skills = [local('skills', 'skills')];
    ui.instances['skill-library'] = node(
      'skill-library',
      {},
      {
        skills: [remote('skills', 'skills')],
        outcomes: [remote('skill-outcomes', 'Outcomes')],
        receipts: [remote('receipts', 'Receipts')],
        reading: [local('reading-area', 'ReadingArea')],
        source: [local('source-text', 'SourceText')],
        file: [local('file', 'FilePicker')],
        input: [local('input', 'TextInput')],
        reader: [local('reader', 'Reader')],
      },
    );
    remoteExports.push({
      capability: { deployment: 'local', instance: [keys.skills], export: 'skills' },
      declaration: { contract: contracts.skillLibrary, availability: 'required' },
    });
    Object.assign(connections.hosts.find((host) => host.name === 'local')!.endpoints, {
      skills: expose(
        'gateway',
        'skills',
        [
          'list',
          'read',
          ...(configuration.skills.manage
            ? ['preview', 'apply', 'importSource', 'deleteSource']
            : []),
          ...(configuration.skills.preferences ? ['setPreference'] : []),
        ],
        !!(configuration.skills.manage || configuration.skills.preferences),
      ),
    });
  }
  Object.assign(ui.instances, {
    'agent-screen': node(
      'agent-screen',
      {
        terminals: !!configuration.processes,
        hostLogs: true,
        skills: !!configuration.skills,
        scheduledTasks: !!configuration.agent.scheduledTasks,
        web: !!configuration.web,
        jev: !!configuration.jev,
        place: configuration.agent.workspaceRoots[0]?.path ?? '',
      },
      {
        agent: [remote('agent', 'agent')],
        hostLogs: [remote('host-logs', 'HostLogs')],
        reading: [local('reading-area', 'ReadingArea')],
        file: [local('file', 'FilePicker')],
        receipts: [remote('receipts', 'Receipts')],
        reader: [local('reader', 'Reader')],
        input: [local('input', 'TextInput')],
        editor: [local('editor', 'Textarea')],
        history: [local('history', 'History')],
        composer: [local('composer', 'Composer')],
        footer: [local('composer-footer', 'Footer')],
        authoring: [local('ui-authoring-worker', 'Worker')],
        rows: [local('rows', 'Rows')],
        checkpoint: [local('checkpoint', 'Checkpoint')],
        themeFile: [local('text-file', 'TextFile')],
        palette: [local('command-palette', 'CommandPalette')],
        directories: [local('directory-picker', 'DirectoryPicker')],
        projects: [local('projects', 'Projects')],
        imports: [local('conversation-import', 'Imports')],
        assistantPreferences: [local('assistant-preferences', 'AssistantPreferences')],
        networkSettings: [local('network-settings', 'NetworkSettings')],
        sessionDefaults: [local('session-defaults', 'SessionDefaults')],
        ...(configuration.web ? { webSettings: [local('web-settings', 'WebSettings')] } : {}),
        ...(configuration.jev ? { jevSettings: [local('jev-settings', 'JevSettings')] } : {}),
        insights: [local('insights', 'Insights')],
        recall: [local('recall', 'Recall')],
        scheduler: [local('scheduler', 'Scheduler')],
        apps: [local('apps', 'Apps')],
        details: [local('session-details', 'SessionDetails')],
        terminals: [local('terminal-panel', 'TerminalPanel')],
        ...(configuration.skills ? { skills: [local('skill-library', 'SkillLibrary')] } : {}),
        queueEditor: [local('queue-editor', 'QueueEditor')],
        formEditor: [local('form-editor', 'FormEditor')],
        questionEditor: [local('question-editor', 'QuestionEditor')],
        serviceApproval: [local('service-approval', 'ServiceApproval')],
        permissionApproval: [local('permission-approval', 'PermissionApproval')],
        help: [local('help', 'Help')],
        models: [local('execution-picker', 'ExecutionPicker')],
        connections: [local('connections', 'Connections')],
        deployment: [local('deployment', 'Deployment')],
        navigation: [local('navigation', 'Navigation')],
        timeline: [local('timeline', 'Timeline')],
        imageViewer: [local('image-viewer', 'ImageViewer')],
        appearance: [local('appearance', 'Appearance')],
        settings: [remote('settings', 'Settings')],
        surface: [local('surface', 'Surface')],
      },
    ),
    'execution-picker': node(
      'execution-picker',
      {},
      {
        models: [local('models', 'Models')],
        agent: [remote('agent', 'agent')],
        receipts: [remote('receipts', 'Receipts')],
        input: [local('input', 'TextInput')],
        reader: [local('reader', 'Reader')],
      },
    ),
    'command-palette': node(
      'command-palette',
      {},
      {
        agent: [remote('agent', 'agent')],
        input: [local('input', 'TextInput')],
        reader: [local('reader', 'Reader')],
        reading: [local('reading-area', 'ReadingArea')],
      },
    ),
    recall: node(
      'recall',
      {},
      {
        agent: [remote('agent', 'agent')],
        reading: [local('reading-area', 'ReadingArea')],
        input: [local('input', 'TextInput')],
        reader: [local('reader', 'Reader')],
      },
    ),
    scheduler: node(
      'scheduler',
      {},
      {
        agent: [remote('agent', 'agent')],
        reading: [local('reading-area', 'ReadingArea')],
        input: [local('input', 'TextInput')],
        editor: [local('editor', 'Textarea')],
        reader: [local('reader', 'Reader')],
        models: [local('models', 'Models')],
        receipts: [remote('receipts', 'Receipts')],
        history: [local('history', 'History')],
      },
    ),
    projects: node(
      'projects',
      {},
      {
        agent: [remote('agent', 'agent')],
        directories: [local('directory-picker', 'DirectoryPicker')],
        reading: [local('reading-area', 'ReadingArea')],
        input: [local('input', 'TextInput')],
        reader: [local('reader', 'Reader')],
        receipts: [remote('receipts', 'Receipts')],
        history: [local('history', 'History')],
      },
    ),
    apps: node(
      'apps',
      { root: 'apps', entries: [] },
      {
        pages: [],
        reading: [local('reading-area', 'ReadingArea')],
        input: [local('input', 'TextInput')],
        editor: [local('editor', 'Textarea')],
        file: [local('file', 'FilePicker')],
        reader: [local('reader', 'Reader')],
        history: [local('history', 'History')],
        management: [
          {
            kind: 'remote',
            deployment: 'manager',
            instance: [keys['deployment-manager']],
            export: 'Management',
          },
        ],
        receipts: [
          { kind: 'remote', deployment: 'manager', instance: [keys.receipts], export: 'Receipts' },
        ],
      },
    ),
    'conversation-import': node(
      'conversation-import',
      {},
      {
        agent: [remote('agent', 'agent')],
        directories: [local('directory-picker', 'DirectoryPicker')],
        reading: [local('reading-area', 'ReadingArea')],
        input: [local('input', 'TextInput')],
        reader: [local('reader', 'Reader')],
        receipts: [remote('receipts', 'Receipts')],
        history: [local('history', 'History')],
      },
    ),
    'directory-picker': node(
      'directory-picker',
      {},
      {
        agent: [remote('agent', 'agent')],
        reading: [local('reading-area', 'ReadingArea')],
        input: [local('input', 'TextInput')],
        reader: [local('reader', 'Reader')],
      },
    ),
    'session-details': node(
      'session-details',
      {},
      {
        agent: [remote('agent', 'agent')],
        input: [local('input', 'TextInput')],
        editor: [local('editor', 'Textarea')],
        reader: [local('reader', 'Reader')],
        reading: [local('reading-area', 'ReadingArea')],
        receipts: [remote('receipts', 'Receipts')],
        history: [local('history', 'History')],
      },
    ),
    'terminal-view': node('terminal-view'),
    'terminal-panel': node(
      'terminal-panel',
      {},
      {
        ...(configuration.processes
          ? { terminal: [remote('workspace-processes', 'terminal')] }
          : {}),
        view: [local('terminal-view', 'TerminalView')],
        input: [local('input', 'TextInput')],
        reader: [local('reader', 'Reader')],
        reading: [local('reading-area', 'ReadingArea')],
        receipts: [remote('receipts', 'Receipts')],
        history: [local('history', 'History')],
      },
    ),
    help: node('help', {}, { reading: [local('reading-area', 'ReadingArea')] }),
    'queue-editor': node('queue-editor', {}, { editor: [local('editor', 'Textarea')] }),
    'form-editor': node(
      'form-editor',
      {},
      { editor: [local('editor', 'Textarea')], reader: [local('reader', 'Reader')] },
    ),
    'question-editor': node('question-editor', {}, { editor: [local('editor', 'Textarea')] }),
    'permission-approval': node(
      'permission-approval',
      {},
      { reading: [local('reading-area', 'ReadingArea')] },
    ),
    'service-approval': node(
      'service-approval',
      {},
      {
        agent: [remote('agent', 'agent')],
        timeline: [local('timeline', 'Timeline')],
        reading: [local('reading-area', 'ReadingArea')],
      },
    ),
    composer: node(
      'composer',
      {},
      {
        editor: [local('editor', 'Textarea')],
        reader: [local('reader', 'Reader')],
        agent: [remote('agent', 'agent')],
        history: [local('history', 'History')],
        receipts: [remote('receipts', 'Receipts')],
      },
    ),
    'composer-footer': node('composer-footer'),
    'ui-authoring-worker': node(
      'ui-authoring',
      {},
      {
        requests: [remote('ui-authoring', 'Authoring')],
        management: [
          {
            kind: 'remote',
            deployment: 'manager',
            instance: [keys['deployment-manager']],
            export: 'Management',
          },
        ],
        managerReceipts: [
          { kind: 'remote', deployment: 'manager', instance: [keys.receipts], export: 'Receipts' },
        ],
        receipts: [remote('receipts', 'Receipts')],
        history: [local('history', 'History')],
      },
      'ui-authoring-worker',
    ),
    rows: node('rows'),
    checkpoint: node('checkpoint'),
    'text-file': node('text-file', { path: configuration.themeFile ?? null }),
    'morph-mark': node('morph-mark', { ...welcomeMark }),
    navigation: node('navigation', {}, { reading: [local('reading-area', 'ReadingArea')] }),
    'image-viewer': node('image-viewer'),
    timeline: node(
      'timeline',
      {},
      {
        reading: [local('reading-area', 'ReadingArea')],
        input: [local('input', 'TextInput')],
        reader: [local('reader', 'Reader')],
        agent: [remote('agent', 'agent')],
        formatter: [local('markdown-text', 'MarkdownText')],
      },
    ),
    'reading-area': node(
      'reading-area',
      {},
      {
        source: [local('source-text', 'SourceText')],
        markdown: [local('markdown', 'Markdown')],
        tools: [local('tool-details', 'ToolDetails')],
        label: [local('label', 'Label')],
      },
    ),
    markdown: node('markdown'),
    'markdown-text': node('markdown-text'),
    label: node('label'),
    'tool-details': node(
      'tool-details',
      {},
      {
        agent: [remote('agent', 'agent')],
        reading: [local('recovery-reading', 'ReadingArea')],
        source: [local('source-text', 'SourceText')],
        diff: [local('diff', 'Diff')],
      },
    ),
    diff: node('diff'),
  });
  ui.launch = local('agent-screen', 'AgentScreen');
  remoteExports.push({
    capability: { deployment: 'local', instance: [keys['host-logs']], export: 'HostLogs' },
    declaration: { contract: contracts.hostLogs, availability: 'required' },
  });
  remoteExports.unshift({
    capability: { deployment: 'local', instance: [keys.agent], export: 'agent' },
    declaration: { contract: contracts.agent, availability: 'required' },
  });
  Object.assign(connections.hosts.find((host) => host.name === 'local')!.endpoints, {
    agent: expose('gateway', 'agent', Object.keys(contracts.agent.interface.methods), true),
    hostLogs: expose('gateway', 'hostLogs', ['read', 'watch'], false),
  });
  if (configuration.processes) {
    remoteExports.push({
      capability: {
        deployment: 'local',
        instance: [keys['workspace-processes']],
        export: 'terminal',
      },
      declaration: { contract: contracts.terminal, availability: 'required' },
    });
    Object.assign(connections.hosts.find((host) => host.name === 'local')!.endpoints, {
      terminal: expose(
        'gateway',
        'terminal',
        Object.keys(contracts.terminal.interface.methods),
        true,
      ),
    });
  }
  if (
    configuration.agent.tools.some((tool) => tool.import === 'ui') ||
    asksForms ||
    asksQuestions ||
    fileMethods.length ||
    processMethods.length ||
    skillCalls.length ||
    executors.length
  )
    Object.assign(connections.hosts.find((host) => host.name === 'local')!, {
      native: [
        ...(configuration.agent.tools.some((tool) => tool.import === 'ui')
          ? [
              {
                origin: [keys.agent],
                consumer: [keys.agent],
                provider: [keys['ui-authoring']],
                export: 'Authoring',
                methods: ['submit', 'result'],
                recovery: true,
              },
            ]
          : []),
        ...executors.flatMap((executor) => [
          ...(configuration.executorProcesses?.setupDirectory
            ? [
                {
                  origin: [keys.agent],
                  consumer: [keys.agent],
                  provider: [keys[executorPath(executor.name)]!],
                  export: 'setup',
                  methods: ['setup'],
                  recovery: true,
                },
              ]
            : []),
          {
            origin: [keys.agent],
            consumer: [keys.agent],
            provider: [keys[executorPath(executor.name)]!],
            export: 'executor',
            methods: ['execute'],
            recovery: true,
          },
          ...[
            {
              provider: keys['protocol-processes'],
              export: 'protocol',
              methods: [
                'run',
                'input',
                ...(configuration.executorProcesses?.setupDirectory ? ['launch'] : []),
              ],
            },
            { provider: keys['workspace-files'], export: 'files', methods: ['readText', 'write'] },
            { provider: keys.forms, export: 'forms', methods: ['ask'] },
          ].map((grant) => ({
            ...grant,
            origin: [keys[executorPath(executor.name)]!],
            consumer: [keys[executorPath(executor.name)]!],
            provider: [grant.provider],
            recovery: true,
          })),
        ]),
        ...(computerMethods.length
          ? [
              {
                origin: [keys.agent],
                consumer: [keys.agent],
                provider: [keys['workspace-computer']],
                export: 'computer',
                methods: [...computerMethods, 'state'],
                recovery: true,
              },
            ]
          : []),
        ...(webMethods.length
          ? [
              {
                origin: [keys.agent],
                consumer: [keys.agent],
                provider: [keys['workspace-web']],
                export: 'web',
                methods: [...webMethods, 'capture'],
                recovery: true,
              },
            ]
          : []),
        ...(skillCalls.length
          ? [
              {
                origin: [keys.agent],
                consumer: [keys.agent],
                provider: [keys.skills],
                export: 'skills',
                methods: skillCalls,
                recovery: false,
              },
            ]
          : []),
        ...(asksForms
          ? [
              {
                origin: [keys.agent],
                consumer: [keys.agent],
                provider: [keys.forms],
                export: 'forms',
                methods: ['ask'],
                recovery: true,
              },
            ]
          : []),
        ...(asksQuestions
          ? [
              {
                origin: [keys.agent],
                consumer: [keys.agent],
                provider: [keys.questions],
                export: 'questions',
                methods: ['ask'],
                recovery: true,
              },
            ]
          : []),
        ...(fileMethods.length
          ? [
              {
                origin: [keys.agent],
                consumer: [keys.agent],
                provider: [keys['workspace-files']],
                export: 'files',
                methods: fileMethods,
                recovery: true,
              },
            ]
          : []),
        ...(processMethods.length
          ? [
              {
                origin: [keys.agent],
                consumer: [keys.agent],
                provider: [keys['workspace-processes']],
                export: 'processes',
                methods: processMethods,
                recovery: true,
              },
            ]
          : []),
        ...(processMethods.includes('health') && configuration.web
          ? [
              {
                origin: [keys.agent],
                consumer: [keys['workspace-processes']],
                provider: [keys['workspace-web']],
                export: 'web',
                methods: ['probe'],
                recovery: true,
              },
            ]
          : []),
      ],
    });
  return { host, managementHost, ui, remoteExports, connections };
}
/** Independent configuration workbench: no Agent capability or conversation UI. */
export function studioComposition(
  configuration: Omit<CompositionConfiguration, 'agent'>,
): ApplicationSources {
  const { host, managementHost, ui, remoteExports, connections, local, node } = settingsComposition(
    configuration,
    'studio-gateway',
  );
  ui.instances.studio = node(
    'studio',
    { locale: configuration.locale },
    {
      connections: [local('connections', 'Connections')],
      models: [local('models', 'Models')],
      deployment: [local('deployment', 'Deployment')],
      appearance: [local('appearance', 'Appearance')],
      locale: [local('locale', 'Locale')],
      surface: [local('surface', 'Surface')],
    },
  );
  ui.launch = local('studio', 'Workbench');
  return { host, managementHost, ui, remoteExports, connections };
}

/** One native coordinator and an explicitly selected worker, using ordinary UI bindings. */
export function workhubComposition(
  configuration: CompositionConfiguration,
  initialRoute: 'coordinator' | 'sessions' = 'coordinator',
): ApplicationSources {
  if (!configuration.coordinator)
    throw new Error('WorkHub requires an explicit coordinator Agent configuration');
  const { keys, packages } = configuration;
  const worker = agentComposition(configuration);
  const coordinator = agentComposition({
    ...configuration,
    agent: configuration.coordinator,
    processes: configuration.coordinatorProcesses,
    acpExecutors: configuration.coordinatorAcpExecutors,
    executorProcesses: configuration.coordinatorExecutorProcesses,
    keys: {
      ...keys,
      ...Object.fromEntries(
        (configuration.coordinatorAcpExecutors ?? []).map((e) => [
          `acp-${e.name}`,
          keys[`coordinator-acp-${e.name}`],
        ]),
      ),
    },
  });
  const local = (name: string, exportName: string): BindingTarget => ({
    kind: 'instance',
    path: [name],
    export: exportName,
  });
  const processName = 'coordinator-workspace-processes';
  const protocolName = 'coordinator-protocol-processes';
  const coordinatorExecutors = new Set(
    (configuration.coordinatorAcpExecutors ?? []).map((e) => `acp-${e.name}`),
  );
  const hostName = (name: string) =>
    name === 'agent'
      ? 'workhub'
      : name === 'workspace-processes'
        ? processName
        : name === 'protocol-processes'
          ? protocolName
          : coordinatorExecutors.has(name)
            ? `coordinator-${name}`
            : name;
  const remapHost = (target: BindingTarget): BindingTarget =>
    target.kind === 'instance' && target.path.length === 1
      ? {
          ...target,
          path: [hostName(target.path[0])],
        }
      : target;
  const remapBindings = (node: CompositionNode, map: (target: BindingTarget) => BindingTarget) =>
    Object.fromEntries(
      Object.entries(node.bindings ?? {}).map(([name, targets]) => [name, targets?.map(map)]),
    );
  for (const [name, node] of Object.entries(coordinator.host.instances)) {
    if (!node) continue;
    if (name === 'agent' || name === 'gateway' || name === 'workspace-processes') continue;
    const mapped = hostName(name);
    if (!worker.host.instances[mapped])
      worker.host.instances[mapped] = {
        ...node,
        ...(name === 'protocol-processes'
          ? {
              key: keys[protocolName],
              configuration: {
                values: {
                  ...node.configuration!.values,
                  sessionOwner: [keys.workhub],
                  sessionKind: 'workhub',
                },
              },
            }
          : {}),
        bindings: remapBindings(node, remapHost),
      };
  }
  const agent = coordinator.host.instances.agent!;
  worker.host.instances.workhub = {
    ...agent,
    key: keys.workhub,
    package: { source: 'pinned', package: packages.workhub },
    configuration: { values: { agent: agent.configuration!.values, workers: ['worker'] } },
    bindings: { ...remapBindings(agent, remapHost), worker: [local('agent', 'agent')] },
  };
  if (configuration.coordinatorProcesses) {
    const processes = coordinator.host.instances['workspace-processes']!;
    worker.host.instances[processName] = {
      ...processes,
      key: keys[processName],
      configuration: {
        values: {
          ...processes.configuration!.values,
          sessionOwner: [keys.workhub],
          sessionKind: 'workhub',
        },
      },
    };
  }
  const gateway = worker.host.instances.gateway!.bindings!;
  gateway.coordinator = [local('workhub', 'agent')];
  gateway.workhub = [local('workhub', 'workhub')];
  if (configuration.coordinatorProcesses)
    gateway.coordinatorTerminal = [local(processName, 'terminal')];

  // Clone exactly the dependency closure whose bindings reach the coordinator.
  // Unrelated shared components remain ordinary instances; component state belongs
  // to each mounted occurrence, so hidden views keep their original custody.
  const selected = new Set<string>();
  const ownRemote = (target: BindingTarget) =>
    target.kind === 'remote' &&
    target.deployment === 'local' &&
    (target.instance[0] === keys.agent || target.instance[0] === keys['workspace-processes']);
  let changed = true;
  while (changed) {
    changed = false;
    for (const [name, node] of Object.entries(coordinator.ui.instances)) {
      if (
        !selected.has(name) &&
        Object.values(node?.bindings ?? {})
          .flatMap((targets) => targets ?? [])
          .some(
            (target) =>
              ownRemote(target) || (target.kind === 'instance' && selected.has(target.path[0])),
          )
      ) {
        selected.add(name);
        changed = true;
      }
    }
  }
  for (const name of selected) {
    const node = coordinator.ui.instances[name]!;
    worker.ui.instances[`coordinator-${name}`] = {
      ...node,
      key: keys[`coordinator-${name}` as InstanceName],
      bindings: remapBindings(node, (target) => {
        if (ownRemote(target) && target.kind === 'remote')
          return {
            ...target,
            instance: [target.instance[0] === keys.agent ? keys.workhub : keys[processName]],
          };
        if (target.kind === 'instance' && selected.has(target.path[0]))
          return { ...target, path: [`coordinator-${target.path[0]}`, ...target.path.slice(1)] };
        return target;
      }),
    };
  }
  const coordinatorScreen = worker.ui.instances['coordinator-agent-screen']!;
  coordinatorScreen.configuration!.values!.graphs = true;
  coordinatorScreen.bindings!.workhub = [
    { kind: 'remote', deployment: 'local', instance: [keys.workhub], export: 'workhub' },
  ];
  worker.ui.instances['workhub-root'] = {
    key: keys['workhub-root'],
    package: { source: 'pinned', package: packages['workhub-root'] },
    usePackage: 'entry',
    configuration: { values: { worker: 'worker', initialRoute } },
    bindings: {
      workhub: [
        { kind: 'remote', deployment: 'local', instance: [keys.workhub], export: 'workhub' },
      ],
      coordinator: [local('coordinator-agent-screen', 'AgentScreen')],
      authoring: [local('ui-authoring-worker', 'Worker')],
      worker: [local('agent-screen', 'AgentScreen')],
      directories: [local('coordinator-directory-picker', 'DirectoryPicker')],
      reading: [local('reading-area', 'ReadingArea')],
      source: [local('source-text', 'SourceText')],
      receipts: [
        { kind: 'remote', deployment: 'local', instance: [keys.receipts], export: 'Receipts' },
      ],
      history: [local('history', 'History')],
      appearance: [local('appearance', 'Appearance')],
      settings: worker.ui.instances['agent-screen']!.bindings!.settings,
      surface: [local('surface', 'Surface')],
    },
  };
  worker.ui.instances['subagent-settings'] = {
    key: keys['subagent-settings'],
    package: { source: 'pinned', package: packages['subagent-settings'] },
    usePackage: 'entry',
    configuration: { values: {} },
    bindings: {
      workhub: [
        { kind: 'remote', deployment: 'local', instance: [keys.workhub], export: 'workhub' },
      ],
      agent: [{ kind: 'remote', deployment: 'local', instance: [keys.agent], export: 'agent' }],
      models: [local('models', 'Models')],
      input: [local('input', 'TextInput')],
      reader: [local('reader', 'Reader')],
      receipts: [
        { kind: 'remote', deployment: 'local', instance: [keys.receipts], export: 'Receipts' },
      ],
      history: [local('history', 'History')],
    },
  };
  worker.ui.instances['agent-screen']!.bindings!.subagentSettings = [
    local('subagent-settings', 'SubagentSettings'),
  ];
  worker.ui.launch = local('workhub-root', 'WorkHub');
  worker.remoteExports.push(
    {
      capability: { deployment: 'local', instance: [keys.workhub], export: 'agent' },
      declaration: { contract: contracts.agent, availability: 'required' },
    },
    {
      capability: { deployment: 'local', instance: [keys.workhub], export: 'workhub' },
      declaration: { contract: contracts.workhubCoordinator, availability: 'required' },
    },
  );
  if (configuration.coordinatorProcesses)
    worker.remoteExports.push({
      capability: { deployment: 'local', instance: [keys[processName]], export: 'terminal' },
      declaration: { contract: contracts.terminal, availability: 'required' },
    });
  type Grant = {
    origin: string[];
    consumer: string[];
    provider: string[];
    export: string;
    methods: string[];
    recovery: boolean;
  };
  type Connections = {
    hosts: { name: string; endpoints: Record<string, unknown>; native?: Grant[] }[];
  };
  const host = (worker.connections as Connections).hosts.find((host) => host.name === 'local')!;
  const coordinatorHost = (coordinator.connections as Connections).hosts.find(
    (host) => host.name === 'local',
  )!;
  const hostIdentity = (identity: string[]) =>
    identity[0] === keys.agent
      ? [keys.workhub]
      : identity[0] === keys['workspace-processes']
        ? [keys[processName]]
        : identity[0] === keys['protocol-processes']
          ? [keys[protocolName]]
          : identity;
  const endpoint = (name: string, methods: string[]) => ({
    consumer: [keys.gateway],
    import: name,
    grant: { methods, recovery: true },
  });
  host.endpoints.coordinator = endpoint(
    'coordinator',
    Object.keys(contracts.agent.interface.methods),
  );
  host.endpoints.workhub = endpoint(
    'workhub',
    Object.keys(contracts.workhubCoordinator.interface.methods),
  );
  if (configuration.coordinatorProcesses)
    host.endpoints.coordinatorTerminal = endpoint(
      'coordinatorTerminal',
      Object.keys(contracts.terminal.interface.methods),
    );
  host.native = [
    ...(host.native ?? []),
    ...(coordinatorHost.native ?? []).map((grant) => ({
      ...grant,
      origin: hostIdentity(grant.origin),
      consumer: hostIdentity(grant.consumer),
      provider: hostIdentity(grant.provider),
    })),
    {
      origin: [keys.workhub],
      consumer: [keys.workhub],
      provider: [keys.agent],
      export: 'agent',
      methods: [
        'list',
        'directories',
        'create',
        'uploadAttachment',
        'send',
        'execution',
        'execute',
        'executors',
        'tools',
        'snapshotPatch',
        'readPatch',
        'work',
        'observe',
        'watch',
        'stop',
        'resume',
        'queue',
        'editQueue',
      ],
      recovery: true,
    },
  ];
  return worker;
}
