# Reverse Runtime Host connections

A Host behind NAT can dial an outbound WebSocket to a reachable Maka gateway. The
controlling Desktop connects to the gateway using its existing TLS/manual Host
profile. The executing Host still owns authentication, operation permissions,
Sessions, tools, tasks and receipts. No inbound port or separate tunnel client is
needed on the executing computer.

```text
Executing Maka Host --outbound WSS--> reachable Maka gateway <--WSS-- Desktop/Bot
```

The gateway can run beside the controlling Desktop, behind any WebSocket reverse
proxy, or on a public server with its own TLS certificate. Cloudflare Tunnel is
one possible deployment tool, not a dependency or a setting in Maka. A domain
alone does not run the gateway or provide a reachable network path.

## Trust and scope

This is a **trusted gateway** transport, not the encrypted Peer transport. TLS
protects each connection, but the gateway and a TLS-terminating proxy can see
Host credentials and protocol contents. Deploy them only under your control and
trust. This version does not add application-level end-to-end encryption.

There are two distinct credentials:

- A route token lets the executing Host register at a specific gateway route.
  It does **not** grant permission to execute operations on that Host.
- A normal Maka Host credential authenticates the controlling Client. The
  executing Host checks it before admitting each connection and retains the
  existing revocation, operation-grant and expected-root checks.

Tokens are sent in headers/control frames, never query strings. Browser Origin
requests are rejected. The gateway defaults to loopback. Remote plaintext binds
require explicit `behindProxy: true`; use that only on an interface reachable
solely by your trusted reverse proxy. Prefer direct TLS otherwise. Browser-based
login redirects are not supported by the connector.

The initial setup uses built-in CLI commands and the existing manual Host UI;
there is no new Desktop setup wizard. The gateway executable and executing Host must contain this feature. The
controlling Desktop uses its existing TLS Host support and must satisfy the normal
Host protocol compatibility requirement. Setup never restarts an existing Host
or modifies its Sessions.

## 1. Create a route on the gateway machine

Choose a new directory and a reachable **origin URL** (no path):

```sh
maka runtime-host reverse init \
  --directory /absolute/path/reverse-laptop \
  --url wss://remote.example.com \
  --route laptop \
  --port 7444
```

This generates `gateway.json` and `connector.json` with a cryptographically random
route token. Files have mode `0600`; the new directory has mode `0700`. Existing
directories are refused to avoid silently replacing a pairing.

Run the gateway:

```sh
maka runtime-host reverse gateway --config /absolute/path/reverse-laptop/gateway.json
```

Forward WebSocket upgrades for `/reverse/*` and `/runtime-host/*` from your public
WSS endpoint to `http://127.0.0.1:7444`. The proxy must preserve Authorization and
`X-Maka-Reverse-Ticket` headers. It must not cache, redirect, or rewrite these
paths. A proxy is optional: the gateway accepts `--tls-certificate <pem>` and
`--tls-private-key <pem>` for direct TLS. Set its configured bind address/port to
match the intended listener. No public listener is enabled by default.

Multiple computers use different `route` values/tokens in `gateway.json.routes`.
Never share a route between independent computers. Restart the gateway to apply
route changes. Removing a route or replacing its token revokes that route after
restart; the executing connector will then remain offline.

## 2. Configure the executing computer

Securely transfer **only** its `connector.json`, retaining mode `0600`. Select the
same canonical root used by that computer's Maka Desktop/Host:

```sh
maka runtime-host reverse configure \
  --root /absolute/path/to/maka-root \
  --config /absolute/path/connector.json
maka runtime-host reverse status --root /absolute/path/to/maka-root
```

Configuration is persisted in the root's control directory, as
`reverse-connection.json`. It is automatically loaded by both Desktop-launched
Hosts and `runtime-host serve` on their **next start**. The status command reports
configuration only, not live connectivity, and never prints the token.

Restart the intended Host when convenient. Do not start a second Host over an
already-owned root. To use a fresh isolated root for testing:

```sh
maka runtime-host serve --root /absolute/path/to/isolated-root
```

The connector runs inside Maka. It makes outbound connections only, so no
Cloudflare/cloudflared installation is required on this computer. Connection
state is logged as `connecting`, `connected`, `offline`, or `stopped` without
credentials. `connected` refers to gateway transport, not completion of a task.

## 3. Connect the controlling Desktop

Issue a normal Host credential on the executing computer using the existing flow:

```sh
maka runtime-host access issue \
  --root /absolute/path/to/maka-root \
  --principal assistant-desktop \
  --preset desktop-client
```

In Desktop **Settings → Workspace → Add computer → Manual configuration**, use:

- TLS URL: `wss://remote.example.com/runtime-host/laptop`
- Expected root: the executing Host's root ID (also printed by reverse status).
- Credential: the **Host credential**, not the route token.

If the Desktop and gateway share a machine, an explicitly configured loopback
plaintext profile can use `ws://127.0.0.1:7444/runtime-host/laptop`; the executing
Host still connects to the public WSS endpoint. The gateway itself is not a Host
and does not start a second assistant.

The normal connected-Host tools and background feedback bridge then work through
this connection. Remote tools still need their existing `remote.tools` grant.
See [multi-Host assistant tools](remote-agent-tools.md) for supported tools and
limitations, including Browser/Computer Use exports.

## Recovery and shutdown

- Gateway unavailable at startup does not prevent local Host startup/work.
- Both sides use WebSocket ping/pong to detect half-open links. The connector
  retries with jittered exponential backoff up to approximately 30 seconds.
- A new authenticated connector for the same route replaces the old one and
  closes its client channels. The replaced connector stops rather than fighting
  the new connection in a reconnect loop; restart it only if takeover is intended.
- Gateway loss closes pending/client connections. In-progress Host operations
  are not automatically replayed. Reconnect and query/retry using the existing
  call/context identity and durable receipt semantics.
- Existing continuous tasks stay under the executing Host's scheduler. Deferred
  feedback remains in its outbox for the existing Desktop bridge to recover.
- Reverse links do not migrate or replicate databases, model credentials or files.
- `reverse disable --root <path>` removes only reverse configuration. It takes
  effect at the next Host start. To revoke execution immediately, use the normal
  Host credential revocation flow; to cut the route, remove it and restart the
  gateway. Neither operation should be mistaken for cancelling an existing task.

## Verification

The targeted tests use actual loopback WebSocket/TCP connections and a real Maka
Host with temporary data. They exercise Host tool calls, gateway/Host restart,
lost responses after a shell side effect, receipt recovery without replay,
credential rejection/revocation, expected-root and operation-grant enforcement,
duplicate registration, offline startup, and configuration persistence. No paid
model or production data is needed. Public-proxy and physical two-machine checks
remain deployment verification, not claims made by the local tests.
