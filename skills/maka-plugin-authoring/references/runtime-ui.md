<!--
  Licensed to the Apache Software Foundation (ASF) under one
  or more contributor license agreements.  See the NOTICE file
  distributed with this work for additional information
  regarding copyright ownership.  The ASF licenses this file
  to you under the Apache License, Version 2.0 (the
  "License"); you may not use this file except in compliance
  with the License.  You may obtain a copy of the License at

      http://www.apache.org/licenses/LICENSE-2.0

  Unless required by applicable law or agreed to in writing,
  software distributed under the License is distributed on an
  "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
  KIND, either express or implied.  See the License for the
  specific language governing permissions and limitations
  under the License.
-->

# Change the connected UI at runtime

The default product exposes `UIRequest` (`ui.submit`) and `UIResult` (`ui.result`) to the Agent. Discover current tool metadata: customized installations may use different names or omit these tools. The foreground TUI must be connected to process requests. This bridge edits that foreground's existing UI deployment through its deployment manager; it does not grant access to another Root or the business Host composition.

Submit `{ "method": "instances", "input": { "kind": "start" } }`, then pass the returned `{ "id": "…" }` to `UIResult`. A result waits up to five seconds for completion. `queued` means the foreground has not claimed it; `claimed` means execution or recovery is still unconfirmed. Observe the same ID again; submitting another mutation is a new operation. `completed.outcome` is the original UI `ActionOutcome`, including failures and unknown receipts. A recorded, returned manager value can itself be `conflict` or `rejected`. Neither request acceptance nor a committed desired source proves successful UI activation.

Use the source returned by `instances` with `readDocument` (`{source, offset: 0}`), following byte offsets in `next` until null and concatenating `text`. This returns the exact deployment document with instance UUIDs, package references, bindings and remote observations. For package bytes, call `readPackage` with `{source, package, path: null, offset: 0}` to list paths, then with each path and its returned continuation offset. The source is immutable. Copy contracts from that installed manifest or this build's SDK; do not reconstruct schemas from names.

Available commands are `instances`, `readDocument`, `readPackage`, `installSource`, `replaceSource`, `editComposition`, `configure`, `bindingCandidates` and `packageSettings`. Exact inputs are in the packaged SDK's `UiAuthoringCommand`. All command inputs go inside `input`. Queries also pass through `UIRequest` so they run against the connected UI's deployment.

## Install and bind a footer

The TUI AgentScreen has a required `footer` component import with contract `maka.agent.ComposerFooter`. The default `composer-footer` instance exports `Footer` and renders an empty line immediately below the composer. Its props are `sessionId`, `dataSource`, `running`, `theme` and `locale`. The container reserves one terminal row. Render a single line, do not open another surface, and use declared service imports for effects. Metrics and their calculations belong to your plugin.

1. Read the current document and the installed `composer-footer` package. Preserve the exact `Footer` contract and the entry's scene declaration. Author a replacement JavaScript entry, keeping its published contract equal to the manifest. Bundle any SDK imports before installation; the guest has no Node APIs.
2. Submit `installSource` with `{expectedRevision, files}`. `files` maps relative paths to UTF-8 text and must include `maka.extension.json` plus every referenced module. This runs the normal package loader and source validation. Save the returned immutable package reference and new source revision from the recorded manager result.
3. Read that new source document. Change `input.instances["composer-footer"].package` to `{source: "pinned", package: newPackageReference}`. Preserve its `key`, all other instance identities, the launch target and unrelated bindings. Keep the new reference in `document.packages`.
4. Submit `replaceSource` with `{expectedRevision: newSource.revision, document}`. The foreground observes the committed source and reloads the affected components. Check the original result and visible output. A conflict requires reading the latest source and reconsidering the edit; do not overwrite a newer document.

Alternatively insert a new component instance and update the AgentScreen's `footer` binding in one `editComposition` command. WorkHub contains worker and coordinator AgentScreen components; the default shared footer instance serves both, and each mounted component receives its own session props. Bind separately when the two views need different footers. Use the actual names and bindings from the current document.

A minimal module can embed the exact copied contract as `contract` and export:

```js
export function activate() {
  return { Footer: { kind: 'component', contract, component: ['footer'] } };
}
function render(revision, props) {
  return {
    frame: {
      revision, state: { revision }, surface: null,
      view: { kind: 'text', key: 'footer', text: props.running ? 'Working…' : 'Ready', color: props.theme.terminal?.muted ?? (props.theme.muted >>> 8) },
    },
    events: [], actions: [], subscriptions: [],
  };
}
export const footer = {
  mount: (props) => render(0, props),
  update: (state, props) => render(state.revision + 1, props),
  event: (state, _event, props) => render(state.revision + 1, props),
};
```

Validate the built artifact with the actual CLI as described in [Testing and shipping](testing-and-shipping.md). After a mutation interrupts the UI, its authoring worker recovers the original manager receipt through caller history, rather than dispatching the edit again. Do not remove or replace the active authoring worker or its recovery bindings while relying on this channel to report completion.

## Install a published bundle

For a direct HTTP(S) `.maka-plugin` download, submit `UIRequest` with `method: "installUrl"` and `input: { expectedRevision, url, sha256: null }`. Supply the publisher's SHA-256 instead of `null` when available. Observe the original request using `UIResult`, then use the returned package reference in a composition edit. An install stores validated bytes; enabling an instance and choosing its bindings is a separate explicit edit.

The download is bounded to 32 MiB and 60 seconds, follows at most five redirects without HTTPS downgrade, and uses the UI management process's proxy environment. A revision change during download rejects the stale install. A URL must point to an exported bundle, not a repository or web page.

If the foreground cannot open, close its UI and run `maka application restore-defaults --root UI_ROOT`. This restores the originally installed Host/management/UI compositions and exact package versions while preserving their instance identities and data. It does not choose a new product or upgrade the installation. Interrupted restoration resumes from its saved preparation.
