#!/usr/bin/env node
/**
 * 场景：Maka 独立窗 ensure — 复用或拉起官方 Desktop Electron（非 Agnx Tab WCV）。
 * 合同：docs/sub-app/桌面子应用嵌入需求 §4.3.1（独立窗主路径）。
 * 用法：node .agnx/tools/ensure-maka-desktop-window.mjs [--supervised] [--force-restart] [--work-dir <path>]
 * stdout：一行 JSON + 一行 RESULT_JSON:{...}；失败 exit 1。
 */
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { access, constants as fsConstants, mkdir, readFile, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "../..");
const DESKTOP_ROOT = path.join(REPO_ROOT, "apps/desktop");
const APP_ID = "maka";
const ENSURE_ID = "ensure-maka-desktop-window";
const STATE_FILE_NAME = "maka-desktop-window.json";

const { values } = parseArgs({
  options: {
    supervised: { type: "boolean", default: false },
    "force-restart": { type: "boolean", default: false },
    forceRestart: { type: "boolean", default: false },
    "work-dir": { type: "string" },
    workDir: { type: "string" },
  },
  strict: true,
  allowPositionals: false,
});
void values["work-dir"];
void values.workDir;

const forceRestart = values["force-restart"] === true || values.forceRestart === true;

function emit(payload, exitCode = 0) {
  const line = JSON.stringify(payload);
  console.log(line);
  console.log(`RESULT_JSON:${line}`);
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

function resolveStatePath() {
  return path.join(REPO_ROOT, ".agnx", "state", STATE_FILE_NAME);
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

function resolveElectronBinary() {
  const requireFromDesktop = createRequire(path.join(DESKTOP_ROOT, "package.json"));
  try {
    return requireFromDesktop("electron");
  } catch {
    const requireFromRoot = createRequire(path.join(REPO_ROOT, "package.json"));
    try {
      return requireFromRoot("electron");
    } catch {
      return null;
    }
  }
}

async function killPidTree(pid) {
  if (!isPidAlive(pid)) return;
  try {
    process.kill(pid, "SIGTERM");
  } catch {
    /* ignore */
  }
  await new Promise((r) => setTimeout(r, 400));
  if (isPidAlive(pid)) {
    try {
      process.kill(pid, "SIGKILL");
    } catch {
      /* ignore */
    }
  }
}

async function main() {
  if (!values.supervised) {
    console.error(`${ENSURE_ID}: 请使用 --supervised（Agnx process-supervisor 合同）`);
    process.exit(2);
  }

  const mainJs = path.join(DESKTOP_ROOT, "dist/main/main.js");
  if (!(await pathExists(mainJs))) {
    fail("MAKA_DESKTOP_MAIN_MISSING", {
      message:
        "缺少 apps/desktop/dist/main/main.js。请先：cd apps/desktop && npm run build:main（或 npm run build）",
    });
  }

  const electronBin = resolveElectronBinary();
  if (!electronBin || typeof electronBin !== "string") {
    fail("MAKA_ELECTRON_MISSING", {
      message:
        "找不到 electron 二进制。请在 maka 仓根或 apps/desktop 执行 npm install（含 electron）。",
    });
  }

  const statePath = resolveStatePath();
  const existing = await readJson(statePath);

  if (forceRestart && existing?.desktopPid) {
    await killPidTree(existing.desktopPid);
  }

  if (!forceRestart && existing?.ok === true && isPidAlive(existing.desktopPid)) {
    emit({
      ok: true,
      appId: APP_ID,
      ensureId: ENSURE_ID,
      reused: true,
      desktopPid: existing.desktopPid,
      launchKind: existing.launchKind ?? "electron-dist-main",
      stateFile: STATE_FILE_NAME,
      message: "Maka Desktop 独立窗已在运行",
      sidecar: { supervised: true, stateFile: STATE_FILE_NAME },
    });
  }

  const child = spawn(electronBin, ["."], {
    cwd: DESKTOP_ROOT,
    detached: true,
    stdio: "ignore",
    env: {
      ...process.env,
      ELECTRON_RUN_AS_NODE: "",
      MAKA_REPO_ROOT: REPO_ROOT,
      MAKA_AGNX_LAUNCH: "independent-window",
    },
  });
  child.unref();

  const desktopPid = child.pid;
  if (!Number.isInteger(desktopPid) || desktopPid <= 0) {
    fail("MAKA_DESKTOP_SPAWN_FAILED", {
      message: "spawn electron 未返回 pid",
    });
  }

  // 给主进程一点启动时间；若立刻退出则失败
  await new Promise((r) => setTimeout(r, 1500));
  if (!isPidAlive(desktopPid)) {
    fail("MAKA_DESKTOP_EXITED", {
      message:
        "Electron 进程启动后立刻退出。请在 apps/desktop 检查 dist/main 与 preload 是否齐全（npm run build）。",
      desktopPid,
    });
  }

  const payload = {
    ok: true,
    appId: APP_ID,
    ensureId: ENSURE_ID,
    reused: false,
    desktopPid,
    launchKind: "electron-dist-main",
    desktopRoot: DESKTOP_ROOT,
    host: os.hostname(),
    stateFile: STATE_FILE_NAME,
    message: "已拉起 Maka Desktop 独立窗",
    sidecar: { supervised: true, stateFile: STATE_FILE_NAME },
  };
  await writeJson(statePath, payload);
  emit(payload);
}

void main().catch((err) => {
  fail("MAKA_ENSURE_CRASH", {
    message: err instanceof Error ? err.message : String(err),
  });
});
