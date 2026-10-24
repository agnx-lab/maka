#!/usr/bin/env node
/**
 * 场景：Agnx「开始调试」专用 — 只 watch `.agnx` 胶水并 bump 仓根 `.agnx-debug-rev`。
 * 禁止拉起 Maka 官方 Desktop Electron；禁止再造 apps/dev/maka 空壳（会把 ensure 指歪）。
 * 仓根 `npm run dev` → 本脚本；官方桌面 HMR → `npm run dev:desktop`。
 */
import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const MAKA_ROOT = path.resolve(__dirname, "../..");
const GLUE_DIR = path.join(MAKA_ROOT, ".agnx");
const REV_PATH = path.join(MAKA_ROOT, ".agnx-debug-rev");

let rev = 0;

async function bumpRev(reason) {
  rev += 1;
  await mkdir(MAKA_ROOT, { recursive: true });
  await writeFile(REV_PATH, `${String(rev)}\n# ${reason}\n# ${new Date().toISOString()}\n`, "utf8");
  process.stdout.write(`[agnx-debug-watch] rev=${rev} (${reason})\n`);
}

async function main() {
  await bumpRev("start");

  const child = spawn("npm", ["run", "dev", "--silent"], {
    cwd: GLUE_DIR,
    stdio: ["ignore", "pipe", "pipe"],
    env: process.env,
    shell: true,
  });

  let debounce = null;
  const onBuildOutput = (buf) => {
    const text = String(buf);
    process.stdout.write(text);
    if (
      !/build (succeeded|finished)|error:/i.test(text) &&
      !/\b(watching|rebuild|changed)\b/i.test(text)
    ) {
      return;
    }
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(() => {
      void bumpRev("glue-rebuild");
    }, 200);
  };
  child.stdout?.on("data", onBuildOutput);
  child.stderr?.on("data", onBuildOutput);

  const shutdown = () => {
    try {
      child.kill("SIGTERM");
    } catch {
      /* ignore */
    }
    process.exit(0);
  };
  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);

  child.on("exit", (code, signal) => {
    process.stderr.write(
      `[agnx-debug-watch] glue watch exited code=${code} signal=${signal ?? ""}\n`,
    );
    process.exit(code ?? 1);
  });
}

void main();
