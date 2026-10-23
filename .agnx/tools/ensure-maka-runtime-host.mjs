#!/usr/bin/env node
/**
 * 场景：Maka G6.5 ensure — 探测/拉起 Runtime Host + 解析 rendererEntry / agnx-embed preload；失败可读 JSON。
 * 合同：docs/sub-app/桌面子应用嵌入需求 §4.3.1；禁止返回 G6_PLATFORM_PENDING；禁止回退官方 preload.cjs。
 * 用法：node .agnx/tools/ensure-maka-runtime-host.mjs [--supervised] [--work-dir <path>] [--force-restart]
 * 宿主 run-app-tool 会注入 `--work-dir <dataRoot>`；本脚本可忽略该值（state 仍走 resolveStateRoot）。
 */
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { access, constants as fsConstants, mkdir, readFile, writeFile } from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "../..");
const APP_ID = "maka";
const ENSURE_ID = "ensure-maka-runtime-host";
const STATE_FILE_NAME = "maka-runtime-host.json";
const DEV_PORT = Number(process.env.MAKA_DESKTOP_DEV_PORT) || 5179;

const { values } = parseArgs({
  options: {
    supervised: { type: "boolean", default: false },
    "force-restart": { type: "boolean", default: false },
    forceRestart: { type: "boolean", default: false },
    // 宿主默认注入；接受以免 ERR_PARSE_ARGS_UNKNOWN_OPTION
    "work-dir": { type: "string" },
    workDir: { type: "string" },
  },
  strict: true,
  allowPositionals: false,
});
void values["work-dir"];
void values.workDir;

function emit(payload, exitCode = 0) {
  console.log(JSON.stringify(payload));
  process.exit(exitCode);
}

function fail(error, extras = {}) {
  emit({ ok: false, appId: APP_ID, ensureId: ENSURE_ID, error, ...extras }, 1);
}

async function pathExists(target) {
  try {
    await access(target, fsConstants.F_OK);
    return true;
  } catch {
    return false;
  }
}

async function readJson(filePath) {
  if (!(await pathExists(filePath))) return null;
  try {
    return JSON.parse(await readFile(filePath, "utf8"));
  } catch {
    return null;
  }
}

async function writeJson(filePath, data) {
  await mkdir(path.dirname(filePath), { recursive: true });
  await writeFile(filePath, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function probeHttp(url, timeoutMs = 1200) {
  return new Promise((resolve) => {
    const req = http.get(url, { timeout: timeoutMs }, (res) => {
      res.resume();
      resolve(res.statusCode != null && res.statusCode >= 200 && res.statusCode < 500);
    });
    req.on("error", () => resolve(false));
    req.on("timeout", () => {
      req.destroy();
      resolve(false);
    });
  });
}

async function resolveRendererEntry() {
  const devUrl = `http://127.0.0.1:${DEV_PORT}/`;
  if (await probeHttp(devUrl)) {
    return { rendererEntry: devUrl, mode: "dev" };
  }
  const prodHtml = path.join(REPO_ROOT, "apps/desktop/dist-renderer/index.html");
  if (await pathExists(prodHtml)) {
    return { rendererEntry: pathToFileURL(prodHtml).href, mode: "prod" };
  }
  return null;
}

async function resolvePreloadPath() {
  const embed = path.join(REPO_ROOT, "apps/desktop/dist/preload/agnx-embed.cjs");
  if (await pathExists(embed)) {
    return embed;
  }
  return null;
}

function resolveStateRoot() {
  if (process.env.MAKA_STATE_ROOT?.trim()) {
    return path.resolve(process.env.MAKA_STATE_ROOT.trim());
  }
  const agnxHome = process.env.AGNX_HOME?.trim() || path.join(os.homedir(), ".agnx-dev");
  return path.join(agnxHome, "state", "maka-runtime-host");
}

function resolveStatePath() {
  return path.join(REPO_ROOT, ".agnx", "state", STATE_FILE_NAME);
}

function resolveSuperviseScript() {
  return path.join(__dirname, "maka-runtime-host-supervise.mjs");
}

function isPidAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function tryReuseHost(statePath, forceRestart) {
  if (forceRestart) return null;
  const existing = await readJson(statePath);
  if (!existing || existing.ok !== true) return null;
  if (!isPidAlive(existing.supervisePid)) return null;
  if (typeof existing.rootPath !== "string" || !existing.rootPath) return null;
  return existing;
}

async function startSupervise(rootPath, statePath) {
  const script = resolveSuperviseScript();
  if (!(await pathExists(script))) {
    fail("MAKA_SUPERVISE_MISSING", {
      message: `缺少 ${script}`,
    });
  }
  const child = spawn(process.execPath, [script, "--root", rootPath, "--state", statePath], {
    detached: true,
    stdio: "ignore",
    cwd: REPO_ROOT,
    env: {
      ...process.env,
      MAKA_STATE_ROOT: rootPath,
      MAKA_REPO_ROOT: REPO_ROOT,
    },
  });
  child.unref();
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 400));
    const state = await readJson(statePath);
    if (state?.ok === true && state.hostReady === true && isPidAlive(state.supervisePid)) {
      return state;
    }
    if (state?.ok === false && state.error) {
      fail(state.error, { message: state.message, warnings: state.warnings });
    }
  }
  fail("MAKA_HOST_UNAVAILABLE", {
    message: "Runtime Host supervise 超时未就绪（90s）；请检查 packages/runtime-host 是否已 build",
  });
}

async function main() {
  if (!values.supervised) {
    console.error(`${ENSURE_ID}: 请使用 --supervised（Agnx process-supervisor 合同）`);
    process.exit(2);
  }

  const warnings = [];
  const runtimeClient = path.join(REPO_ROOT, "packages/runtime-host/dist/client/index.js");
  const candidateMain = path.join(REPO_ROOT, "packages/runtime-host/dist/execution-candidate-main.js");
  if (!(await pathExists(runtimeClient)) || !(await pathExists(candidateMain))) {
    fail("MAKA_RUNTIME_DIST_MISSING", {
      message:
        "请先构建 @maka/runtime-host：cd apps/maka && npm run build --workspace=@maka/core && npm run build --workspace=@maka/storage && npm run build --workspace=@maka/runtime && npm run build --workspace=@maka/runtime-host",
    });
  }

  const renderer = await resolveRendererEntry();
  if (!renderer) {
    fail("MAKA_RENDERER_UNAVAILABLE", {
      message: `请先启动 desktop Vite（MAKA_DESKTOP_DEV_PORT=${DEV_PORT}）或执行：cd apps/desktop && npm run build:renderer（产物 apps/desktop/dist-renderer/index.html）`,
    });
  }

  const preloadPath = await resolvePreloadPath();
  if (!preloadPath) {
    fail("MAKA_PRELOAD_MISSING", {
      message:
        "缺少 apps/desktop/dist/preload/agnx-embed.cjs。请执行：cd apps/desktop && npm run build:agnx-embed。禁止使用官方 preload.cjs。",
    });
  }

  // Soft-check: official preload must not be selected
  if (preloadPath.endsWith("preload.cjs") && !preloadPath.includes("agnx-embed")) {
    fail("MAKA_PRELOAD_MISSING", {
      message: "检测到官方 preload.cjs；Agnx 嵌入必须使用 agnx-embed.cjs",
    });
  }

  const statePath = resolveStatePath();
  const rootPath = resolveStateRoot();
  await mkdir(rootPath, { recursive: true });

  let hostState = await tryReuseHost(statePath, values["force-restart"] === true);
  let reused = false;
  if (hostState) {
    reused = true;
  } else {
    hostState = await startSupervise(rootPath, statePath);
  }

  emit({
    ok: true,
    appId: APP_ID,
    ensureId: ENSURE_ID,
    reused,
    rendererEntry: renderer.rendererEntry,
    preloadPath,
    rootPath: hostState.rootPath ?? rootPath,
    socketPath: hostState.socketPath ?? null,
    hostEpoch: hostState.hostEpoch ?? null,
    supervisePid: hostState.supervisePid ?? null,
    mode: renderer.mode,
    warnings,
    sidecar: {
      supervised: true,
      stateFile: STATE_FILE_NAME,
    },
  });
}

void main().catch((err) => {
  fail("MAKA_ENSURE_CRASH", {
    message: err instanceof Error ? err.message : String(err),
  });
});
