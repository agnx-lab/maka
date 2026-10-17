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

import type {
  EditorObservation,
  EditorSnapshot,
  InputProps,
  ModelSelection,
  AgentSend,
  AgentResume,
  WorkHubResolve,
  WorkHubCoordinator,
  WorkHubTask,
  WorkHubControl,
  WorkHubControlRead,
  WorkHubConfiguration,
  AgentFileListRead,
  AgentFileWatch,
  AgentFilePage,
  AgentFileChange,
  WorkspacePath,
  AgentWorkRead,
  AgentExecutionResult,
  AgentHistoryPage,
  AgentSessionPage,
  AgentList,
  HistoryPosition,
  SetDefaultConnectionTargetInput,
  SettingsModelsPage,
  ConnectionCatalogEntryUpdate,
  ConnectionCredentialTarget,
  SettingsSaveCredential,
  SettingsCredentialSaveResult,
  SettingsConnectionsPage,
  SettingsProvidersRead,
  CredentialMutationResult,
  CatalogMutationResult,
  Action,
  Capture,
  ActionOutcome,
  CallerHistoryPage,
  CallerIntent,
  Component,
  ComponentOutput,
  ComponentInput,
  ComponentValues,
  UiExport,
  Contract,
  HostRequest,
  RequestContext,
  RecoveryRef,
  Surface,
} from '@maka-agent/plugin-sdk';

function originalReceiptQuery(reference: RecoveryRef): Action {
  // Explicitly bind the original reader independently of the business import.
  return {
    key: 'receipt',
    binding: 'receipts',
    member: 0,
    method: 'read',
    input: reference,
    resources: {},
  };
}
function forgedReceiptReference(reference: RecoveryRef) {
  // @ts-expect-error A reference cannot select a Root or convey a recovery grant.
  const forged = { ...reference, root: '/other', grant: 'approved' } satisfies RecoveryRef;
  void forged;
}
void [originalReceiptQuery, forgedReceiptReference];

function originalUiReceipt(outcome: ActionOutcome) {
  if (outcome.status !== 'operation') return;
  const original = outcome.reply.reference;
  // @ts-expect-error Unavailable/not-recorded receipts do not describe an outcome.
  const premature = outcome.reply.receipt.outcome;
  if (outcome.reply.receipt.kind === 'recorded') {
    const known = outcome.reply.receipt.outcome;
    void known;
  }
  void [original, premature];
}
void originalUiReceipt;

const historyPage = { after: 0, limit: 16 } satisfies CallerHistoryPage;
// @ts-expect-error The kernel selects the actual caller namespace.
const forgedHistoryPage = { after: 0, limit: 16, consumer: ['other'] } satisfies CallerHistoryPage;
function originalCallerIntent(intent: CallerIntent) {
  const original = intent.reference;
  // Local correlation accepts scalar JSON too, and readers must validate it.
  const scalar: Action['recovery'] = 'draft-version';
  const list: Action['recovery'] = [1, null, { revision: 2 }];
  // @ts-expect-error Persisted correlation has no trusted product shape.
  const uncheckedRevision = intent.recovery.editor.revision;
  // @ts-expect-error Caller history does not assert business journal facts.
  const businessOutcome = intent.receipt;
  // @ts-expect-error Observation data contains no dispatch authorization token.
  const token = intent.token;
  void [original, scalar, list, uncheckedRevision, businessOutcome, token];
}
void [historyPage, forgedHistoryPage, originalCallerIntent];

const terminal = { kind: 'terminal', binding: 'surface' } satisfies Surface;
// @ts-expect-error Terminal surfaces have no pixel width.
const badTerminal = { kind: 'terminal', binding: 'surface', width: 400 } satisfies Surface;
// @ts-expect-error The removed window presentation is not a terminal surface.
const badWindow = { kind: 'window', binding: 'surface' } satisfies Surface;
// @ts-expect-error Component notifications carry the exact child and event name.
const incompleteEvent = { kind: 'component', key: 'changed', event: {} } satisfies ComponentInput;

const callbacks: Component = {
  // @ts-expect-error Foreground callbacks cannot return promises.
  async mount() {
    return {
      frame: {
        revision: 0,
        state: {},
        surface: terminal,
        view: { kind: 'button', key: 'ok', label: 'OK' },
      },
      events: [],
      actions: [],
    };
  },
  update() {
    throw Error('unused');
  },
  event() {
    throw Error('unused');
  },
};
const fabricated = {
  operation: 'read',
  slot: 'text',
  start: 0,
  end: 1,
  // @ts-expect-error Resource slots cannot carry fabricated bearer handles.
  token: 'grant',
} satisfies HostRequest;
const incompleteCall = {
  operation: 'call',
  import: 'echo',
  provider: 0,
  method: 'echo',
  input: null,
} as const;
// @ts-expect-error Every forward maps actual resource slots explicitly.
const missingSlots: HostRequest = incompleteCall;
async function unknownResult(context: RequestContext) {
  const result = await context.invoke({
    operation: 'call',
    import: 'echo',
    provider: 0,
    method: 'echo',
    input: null,
    attachments: {},
  });
  // @ts-expect-error An unchecked service result is not consumer-selected T.
  const text: string = result;
}
void [
  terminal,
  badTerminal,
  badWindow,
  incompleteEvent,
  callbacks,
  fabricated,
  incompleteCall,
  missingSlots,
  unknownResult,
];

const mutation = {
  operation: 'mutate',
  import: 'echo',
  provider: 0,
  method: 'echo',
  input: null,
  operationId: '00000000-0000-4000-8000-000000000001',
  attachments: {},
} satisfies HostRequest;
const incompleteMutation = {
  operation: 'mutate',
  import: 'echo',
  provider: 0,
  method: 'echo',
  input: null,
  attachments: {},
} as const;
// @ts-expect-error An explicit durable operation identity is required.
const missingOperation: HostRequest = incompleteMutation;
const fabricatedMutation = {
  ...mutation,
  // @ts-expect-error The original bundle identity is private Host custody.
  capture: '00000000-0000-4000-8000-000000000001',
  attachments: { text: 'text' },
} satisfies HostRequest;
void [mutation, missingOperation, fabricatedMutation];

async function durableReceipt(context: RequestContext) {
  const reply = await context.invoke(mutation);
  const operation: string = reply.reference.operation;
  // @ts-expect-error Acceptance never proves that the journal has a result.
  const prematureOutcome = reply.receipt.outcome;
  if (reply.receipt.kind === 'recorded' && reply.receipt.outcome.kind === 'returned') {
    // @ts-expect-error Business output must still be checked against its contract.
    const text: string = reply.receipt.outcome.value;
    void text;
  }
  const recovered = await context.invoke({ operation: 'recover', reference: reply.reference });
  const sameSource = recovered.reference.source;
  if (recovered.receipt.kind === 'unavailable') {
    const reason: string = recovered.receipt.diagnostic;
    void reason;
  }
  void [operation, prematureOutcome, sameSource];
}
const incompleteRecovery = { operation: 'recover' } as const;
// @ts-expect-error Recovery requires the original reference, not a current provider alias.
const missingReference: HostRequest = incompleteRecovery;
void [durableReceipt, missingReference];

function immutableValueExports(contract: Contract, values: ComponentValues) {
  const value = { kind: 'value', contract, value: { tone: 'sage' } } satisfies UiExport;
  // @ts-expect-error All exports use the explicit tagged ABI.
  const old = { contract, component: ['app'] } satisfies UiExport;
  // @ts-expect-error Value exports carry data rather than a callback path.
  const wrong = { kind: 'value', contract, component: ['app'] } satisfies UiExport;
  // @ts-expect-error Captured import members cannot be reassigned.
  values.brand = [];
  if (values.brand) {
    // @ts-expect-error The supplied member list is read-only.
    values.brand.push('replacement');
  }
  void [value, old, wrong];
}
void immutableValueExports;

async function publishSubscriptionItem(context: RequestContext) {
  const published: null = await context.invoke({ operation: 'emit', value: { delta: 'hello' } });
  const forged = {
    operation: 'emit',
    value: 'hello',
    // @ts-expect-error Publishing is request-local, not selected by another request ID.
    subscriptionId: 'other',
  } satisfies HostRequest;
  void [published, forged];
}
void publishSubscriptionItem;

const desiredStream = {
  key: 'timeline',
  binding: 'tasks',
  member: 0,
  method: 'watch',
  input: { task: 'current' },
  resources: {},
} satisfies Action;
function streamingOutput(output: ComponentOutput, outcome: ActionOutcome) {
  const desired: ComponentOutput = { ...output, subscriptions: [desiredStream] };
  if (outcome.status === 'updated') {
    const item: unknown = outcome.value;
    // @ts-expect-error Original stream request identity is private to the kernel.
    const token = outcome.request;
    void [item, token];
  }
  return desired;
}
void streamingOutput;

const selectedModel: ModelSelection = {
  connectionId: 'observed-connection',
  revision: 3,
  modelId: 'chosen-model',
};
const explicitSend: AgentSend = {
  sessionId: 'conversation',
  message: 'Hello',
  model: selectedModel,
};
const resumeOriginal: AgentResume = {
  sessionId: 'conversation',
  sourceInvocationId: 'original-invocation',
  sourceHighWater: 12,
  model: selectedModel,
};
function resumeReceipt(value: AgentExecutionResult) {
  return value.kind === 'found' ? value.execution.invocationId : null;
}
void [resumeOriginal, resumeReceipt];
const clearDefault: SetDefaultConnectionTargetInput = { expectedCatalogRevision: 5, target: null };
// @ts-expect-error Absence is not an explicit clear.
const missingDefault: SetDefaultConnectionTargetInput = { expectedCatalogRevision: 5 };
const exhaustedModels: SettingsModelsPage = {
  kind: 'ready',
  revision: 5,
  choices: [],
  defaultModel: { kind: 'none' },
  next: null,
};
// @ts-expect-error Serialized nullable cursor requires field presence.
const missingCursor: SettingsModelsPage = {
  kind: 'ready',
  revision: 5,
  choices: [],
  defaultModel: { kind: 'none' },
};
// @ts-expect-error Every ready page must explicitly observe the saved default.
const missingDefaultProjection: SettingsModelsPage = {
  kind: 'ready',
  revision: 5,
  choices: [],
  next: null,
};
// @ts-expect-error A send cannot inherit an implicit activation model.
const implicitSend: AgentSend = { sessionId: 'conversation', message: 'Hello' };
void [
  explicitSend,
  clearDefault,
  missingDefault,
  exhaustedModels,
  missingCursor,
  missingDefaultProjection,
  implicitSend,
];

// Absence is distinct from the explicit no-composition/no-receipt observation.
function nativeEditorMetadata(snapshot: EditorSnapshot, props: InputProps) {
  const observed: EditorObservation = { revision: 0, snapshot, receipt: null };
  const selection = snapshot.selection;
  const required: EditorSnapshot = {
    text: { owner: 'native', revision: 0, bytes: 0 },
    selection,
    reversed: false,
    marked: null,
  };
  // @ts-expect-error Metadata must explicitly declare whether there is a receipt.
  const missingReceipt: EditorObservation = { revision: 0, snapshot };
  // @ts-expect-error Metadata must explicitly declare whether IME is composing.
  const missingMarked: EditorSnapshot = { text: required.text, selection, reversed: false };
  void [observed, required, missingReceipt, missingMarked, props.command];
}
void nativeEditorMetadata;

// Basic editing must not require resending hidden model inventories.
const nameOnly: ConnectionCatalogEntryUpdate = { name: 'Personal' };
const explicitEmpty: ConnectionCatalogEntryUpdate = { enabled: false, enabledModelIds: [] };
const clearPatches: ConnectionCatalogEntryUpdate = {
  modelOverrides: null,
  requestBodyOverlay: null,
};
// @ts-expect-error A present basic field cannot be null.
const nullName: ConnectionCatalogEntryUpdate = { name: null };
// @ts-expect-error An override field may be omitted but not supplied as null.
const nullVision: ConnectionCatalogEntryUpdate = { modelOverrides: { model: { vision: null } } };
const providersRead: SettingsProvidersRead = { offset: 0 };
// @ts-expect-error Provider paging uses an explicit offset.
const missingOffset: SettingsProvidersRead = {};
function settingsObservations(target: ConnectionCredentialTarget) {
  const rejectedCatalog: CatalogMutationResult = {
    kind: 'rejected',
    reason: 'invalid_configuration',
  };
  const rejectedCredential: CredentialMutationResult = {
    kind: 'rejected',
    reason: 'invalid_input',
  };
  const rejectedSave: SettingsCredentialSaveResult = { kind: 'rejected', reason: 'invalid_secret' };
  // @ts-expect-error Rejections carry a closed reason, never arbitrary diagnostics.
  const rawError: CatalogMutationResult = { kind: 'rejected', reason: 'user payload' };
  // @ts-expect-error Confirmed rejection requires an explicit reason.
  const missingReason: CredentialMutationResult = { kind: 'rejected' };
  const leakedInput: SettingsCredentialSaveResult = {
    kind: 'rejected',
    reason: 'invalid_secret',
    // @ts-expect-error An original secret never enters the public result.
    secret: 'private',
  };
  const absent: SettingsSaveCredential = { expected: target, expectedCredential: null };
  // @ts-expect-error Omission does not prove that no credential was observed.
  const unobserved: SettingsSaveCredential = { expected: target };
  const secretInJson: SettingsSaveCredential = {
    ...absent,
    // @ts-expect-error Secrets are request-local resource captures, never input JSON.
    secret: 'private',
  };
  const page: SettingsConnectionsPage = {
    kind: 'ready',
    revision: 1,
    connections: [],
    defaultTarget: null,
    next: null,
  };
  // @ts-expect-error Serialized nullable fields require explicit presence.
  const incomplete: SettingsConnectionsPage = { kind: 'ready', revision: 1, connections: [] };
  const basis = { connectionId: target.connectionId, revision: target.revision };
  const removed: CredentialMutationResult = {
    kind: 'connection_stale',
    expected: basis,
    actual: null,
  };
  // @ts-expect-error A stale result explicitly describes absence.
  const missingActual: CredentialMutationResult = { kind: 'connection_stale', expected: basis };
  const noConnectionCommit: CatalogMutationResult = { kind: 'committed', catalogRevision: 2 };
  // @ts-expect-error A stale catalog result explicitly describes absence.
  const missingCatalogActual: CatalogMutationResult = { kind: 'connection_stale', expected: basis };
  void [
    rejectedCatalog,
    rejectedCredential,
    rejectedSave,
    rawError,
    missingReason,
    leakedInput,
    absent,
    unobserved,
    secretInJson,
    page,
    incomplete,
    removed,
    missingActual,
    noConnectionCommit,
    missingCatalogActual,
  ];
}
void [
  nameOnly,
  explicitEmpty,
  clearPatches,
  nullName,
  nullVision,
  providersRead,
  missingOffset,
  settingsObservations,
];

const historyWindow = {
  kind: 'ready',
  through: 16,
  chunks: [],
  before: null,
  after: { sequence: 4096, offset: 0 },
} satisfies AgentHistoryPage;
// @ts-expect-error Missing boundaries are not end-of-history receipts.
const missingHistoryBoundary: AgentHistoryPage = {
  kind: 'ready',
  through: 16,
  chunks: [],
  after: null,
};
const removedHistoryPosition = {
  // @ts-expect-error The removed one-way cursor position is not an alias.
  kind: 'cursor',
  cursor: { sequence: 4096, offset: 0 },
} satisfies HistoryPosition;
void [historyWindow, missingHistoryBoundary, removedHistoryPosition];

const nativeContinuation = {
  input: { type: 'null' },
  output: { kind: 'continuation' },
} satisfies Capture;
const capturedResource = {
  input: {},
  output: { kind: 'resource', resource: { name: 'text', version: 1 } },
} satisfies Capture;
const removedCapture: Capture = {
  input: {},
  // @ts-expect-error Removed resource-only capture declaration has no output kind.
  resource: { name: 'text', version: 1 },
};
const stalePresentation = { status: 'stale' } satisfies ActionOutcome;
const guardedQuery = {
  key: 'history-1',
  binding: 'history',
  member: 0,
  method: 'read',
  input: null,
  resources: {},
  continuations: [{ child: 'reader', method: 'reading', input: null }],
} satisfies Action;
void [nativeContinuation, capturedResource, removedCapture, stalePresentation, guardedQuery];

const sessionWindow = {
  revision: 'catalog',
  needsYouCount: 0,
  sessions: [],
  before: null,
  after: null,
} satisfies AgentSessionPage;
// @ts-expect-error A missing boundary is not an end-of-catalog receipt.
const missingSessionBoundary: AgentSessionPage = {
  revision: 'catalog',
  needsYouCount: 0,
  sessions: [],
  after: null,
};
// @ts-expect-error A bounded page length is not the complete pending-session count.
const missingSessionCount: AgentSessionPage = {
  revision: 'catalog',
  sessions: [],
  before: null,
  after: null,
};
const refreshSessionPage = {
  filter: 'all',
  position: { kind: 'at', cursor: 'session' },
} satisfies AgentList;
const nextSessionPage = {
  filter: 'needs_you',
  position: { kind: 'after', revision: 'catalog', cursor: 'session' },
} satisfies AgentList;
const unfencedSessionPage: AgentList = {
  filter: 'all',
  // @ts-expect-error Every continuation belongs to one catalog revision.
  position: { kind: 'before', cursor: 'session' },
};
void [
  sessionWindow,
  missingSessionBoundary,
  missingSessionCount,
  refreshSessionPage,
  nextSessionPage,
  unfencedSessionPage,
];

const resolveCoordinator: WorkHubResolve = {
  workspace: {
    rootId: 'root-1',
    segments: [],
    directoryIdentity: `sha256:${'0'.repeat(64)}`,
  },
};
function coordinatorSession(value: WorkHubCoordinator): string | null {
  return value.kind === 'ready' ? value.session.id : null;
}
void resolveCoordinator;
void coordinatorSession;

const messageWork: AgentWorkRead = {
  sessionId: 'worker-session',
  target: { kind: 'message', messageId: 'original-message' },
};
function taskExecution(task: WorkHubTask): string | null {
  const status = task.status;
  if (status.kind === 'started' && status.progress?.kind === 'delivered')
    return status.progress.execution.invocationId;
  if (status.kind === 'queued' && status.progress?.kind === 'delivered')
    return status.progress.shared ? null : status.progress.execution.invocationId;
  return null;
}
function coordinatorWorkers(configuration: WorkHubConfiguration): string[] {
  return configuration.workers;
}
const answerPage: AgentWorkRead = {
  sessionId: 'worker-session',
  target: { kind: 'invocation', invocationId: 'original-invocation' },
  cursor: { invocationId: 'answer-invocation', offset: 768 },
};
const originalControl: WorkHubControlRead = { controlId: `wc_${'0'.repeat(64)}` };
function controlStopped(control: WorkHubControl): boolean | null {
  const status = control.status;
  return status.kind === 'returned' && status.outcome.kind === 'stop'
    ? status.outcome.result.requested
    : null;
}
void [messageWork, answerPage, originalControl, controlStopped, taskExecution, coordinatorWorkers];

// Workspace file browsing uses Session authority and exact relative path bytes.
const workspaceRootFiles = {
  sessionId: 'session',
  path: null,
  after: null,
} satisfies AgentFileListRead;
const directoryWatch = { sessionId: 'session', path: null } satisfies AgentFileWatch;
// @ts-expect-error Root selection is explicit; omission is not another path spelling.
const missingFilePath = { sessionId: 'session' } satisfies AgentFileWatch;
// @ts-expect-error A workspace path is encoded bytes, never an absolute Host pathname.
const ambientFilePath: WorkspacePath = '/private/other';
function continueWorkspaceFiles(page: AgentFilePage): AgentFileListRead {
  for (const entry of page.entries) {
    const exactPath: WorkspacePath = entry.path;
    void exactPath;
  }
  return { sessionId: 'session', path: null, after: page.next };
}
function observeWorkspaceFiles(change: AgentFileChange) {
  if (change.kind === 'ready') {
    const originalDirectory: string = change.directoryIdentity;
    void originalDirectory;
  } else if (change.kind === 'unavailable') {
    const reason: 'directory_changed' = change.reason;
    void reason;
  }
  // @ts-expect-error File invalidation has no durable event replay cursor.
  const replay = change.after;
  void replay;
}
void [
  workspaceRootFiles,
  directoryWatch,
  missingFilePath,
  ambientFilePath,
  continueWorkspaceFiles,
  observeWorkspaceFiles,
];
