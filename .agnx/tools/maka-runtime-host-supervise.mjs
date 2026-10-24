#!/usr/bin/env node
/**
 * 场景：Maka Runtime Host 长驻 supervise — ensure 以 detached 拉起；保持 connectOrSpawn 后进程存活。
 */
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = process.env.MAKA_REPO_ROOT?.trim() || path.resolve(__dirname, "../..");

const { values } = parseArgs({
  options: {
    root: { type: "string" },
    state: { type: "string" },
  },
});

async function writeState(statePath, data) {
  await mkdir(path.dirname(statePath), { recursive: true });
  await writeFile(statePath, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

async function main() {
  const rootPath = values.root?.trim() || process.env.MAKA_STATE_ROOT;
  const statePath = values.state?.trim();
  if (!rootPath || !statePath) {
    process.stderr.write("maka-runtime-host-supervise: need --root and --state\n");
    process.exit(2);
  }

  const clientUrl = pathToFileURL(
    path.join(REPO_ROOT, "packages/runtime-host/dist/client/index.js"),
  ).href;
  const protocolUrl = pathToFileURL(
    path.join(REPO_ROOT, "packages/runtime-host/dist/protocol/index.js"),
  ).href;
  const storageUrl = pathToFileURL(
    path.join(REPO_ROOT, "packages/storage/dist/root-authority.js"),
  ).href;
  const candidateEntrypoint = pathToFileURL(
    path.join(REPO_ROOT, "packages/runtime-host/dist/execution-candidate-main.js"),
  );

  try {
    const client = await import(clientUrl);
    const protocol = await import(protocolUrl);
    const storage = await import(storageUrl);
    const {
      connectOrSpawnRuntimeHost,
      connectExistingRuntimeHost,
    } = client;
    const {
      RUNTIME_HOST_PROTOCOL_VERSION,
      INTERACTIVE_RUNTIME_HOST_COMPOSITION_ID,
    } = protocol;
    const { resolveStorageRoot } = storage;

    // connectOrSpawn 要求已标记 storage root；resolveStorageRoot 会创建 .maka-storage-root.json
    await resolveStorageRoot({ path: rootPath, kind: "interactive" });

    const protocolRange = {
      min: RUNTIME_HOST_PROTOCOL_VERSION,
      max: RUNTIME_HOST_PROTOCOL_VERSION,
    };
    const compositionId = INTERACTIVE_RUNTIME_HOST_COMPOSITION_ID;

    let result;
    try {
      result = await connectExistingRuntimeHost({
        rootPath,
        protocol: protocolRange,
        compositionId,
      });
    } catch (err) {
      result = {
        kind: "unavailable",
        reason: err instanceof Error ? err.message : String(err),
      };
    }

    if (result.kind !== "connected") {
      result = await connectOrSpawnRuntimeHost({
        rootPath,
        protocol: protocolRange,
        compositionId,
        candidateEntrypoint,
        closeOnLauncherExit: false,
      });
    }

    if (result.kind !== "connected") {
      await writeState(statePath, {
        ok: false,
        error: "MAKA_HOST_UNAVAILABLE",
        message: `connectOrSpawn 失败：${result.kind}${result.reason ? ` (${result.reason})` : ""}`,
        rootPath,
        supervisePid: process.pid,
      });
      process.exit(1);
    }

    const registration = result.registration;
    await writeState(statePath, {
      ok: true,
      hostReady: true,
      rootPath,
      hostEpoch: registration?.hostEpoch ?? null,
      rootId: registration?.rootId ?? null,
      socketPath: registration?.endpoint ?? null,
      supervisePid: process.pid,
      reused: result.spawnedProcess == null,
      updatedAt: new Date().toISOString(),
    });

    // Keep connection + process alive for Host lifetime.
    const connection = result.connection;
    const shutdown = async () => {
      try {
        await connection.close();
      } catch {
        /* ignore */
      }
      process.exit(0);
    };
    process.on("SIGTERM", () => void shutdown());
    process.on("SIGINT", () => void shutdown());

    // Heartbeat touch state periodically
    setInterval(() => {
      void writeState(statePath, {
        ok: true,
        hostReady: true,
        rootPath,
        hostEpoch: registration?.hostEpoch ?? null,
        rootId: registration?.rootId ?? null,
        socketPath: registration?.endpoint ?? null,
        supervisePid: process.pid,
        heartbeatAt: new Date().toISOString(),
      }).catch(() => undefined);
    }, 30_000);
  } catch (err) {
    await writeState(statePath, {
      ok: false,
      error: "MAKA_HOST_UNAVAILABLE",
      message: err instanceof Error ? err.message : String(err),
      rootPath,
      supervisePid: process.pid,
    });
    process.exit(1);
  }
}

void main();
