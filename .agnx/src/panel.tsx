/**
 * 场景：Maka 薄面板 — ensure 拉起独立 Desktop 窗；不挂 WebContentsView / mountElectronModule。
 */
import { useCallback, useEffect, useState, type CSSProperties, type ReactElement } from "react";
import type { WorkspaceAppHostApi } from "@agnx-lab/app-sdk";

const APP_ID = "maka";
const APP_TITLE = "Apache Maka";
const PANEL_ID = "maka.main";
const ENSURE_TOOL = ".agnx/tools/ensure-maka-desktop-window.mjs";

const ROOT_STYLE: CSSProperties = {
  flex: "1 1 0%",
  alignSelf: "stretch",
  minHeight: 0,
  minWidth: 0,
  width: "100%",
  height: "100%",
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "center",
  gap: 16,
  padding: 24,
  background: "rgb(9, 9, 11)",
  color: "#e4e4e7",
  fontFamily: "ui-sans-serif, system-ui, sans-serif",
};

const TITLE_STYLE: CSSProperties = {
  margin: 0,
  fontSize: 22,
  fontWeight: 600,
  letterSpacing: "-0.02em",
};

const BODY_STYLE: CSSProperties = {
  margin: 0,
  maxWidth: 420,
  fontSize: 14,
  lineHeight: 1.5,
  color: "#a1a1aa",
  textAlign: "center",
};

const ROW_STYLE: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: 10,
  justifyContent: "center",
};

const BTN_STYLE: CSSProperties = {
  appearance: "none",
  border: "1px solid #3f3f46",
  background: "#18181b",
  color: "#fafafa",
  borderRadius: 8,
  padding: "8px 14px",
  fontSize: 13,
  cursor: "pointer",
};

const BTN_PRIMARY_STYLE: CSSProperties = {
  ...BTN_STYLE,
  background: "#27272a",
  borderColor: "#52525b",
};

let boundHost: WorkspaceAppHostApi | null = null;

function getHost(): WorkspaceAppHostApi {
  if (!boundHost) {
    throw new Error(`${APP_TITLE} host 未绑定`);
  }
  return boundHost;
}

export function bindVendorHarnessHost(host: WorkspaceAppHostApi): void {
  boundHost = host;
}

type LaunchState =
  | { kind: "idle" }
  | { kind: "running"; detail: string }
  | { kind: "ok"; detail: string; desktopPid?: number; reused?: boolean }
  | { kind: "error"; detail: string };

function formatEnsureError(result: {
  ok: boolean;
  stdout: string;
  stderr: string;
  resultJson: unknown | null;
  timedOut: boolean;
}): string {
  if (result.timedOut) {
    return "拉起超时：ensure-maka-desktop-window 未在时限内返回";
  }
  const json = result.resultJson;
  if (json && typeof json === "object") {
    const row = json as { message?: unknown; error?: unknown };
    const message = typeof row.message === "string" ? row.message : "";
    const error = typeof row.error === "string" ? row.error : "";
    if (message || error) {
      return [error, message].filter(Boolean).join(" — ");
    }
  }
  const stdout = result.stdout.trim();
  if (stdout) {
    try {
      const parsed = JSON.parse(stdout.split("\n").find((l) => l.startsWith("{")) || "");
      if (parsed && typeof parsed === "object") {
        const row = parsed as { message?: string; error?: string };
        return [row.error, row.message].filter(Boolean).join(" — ") || stdout.slice(0, 400);
      }
    } catch {
      /* fall through */
    }
    return stdout.slice(0, 400);
  }
  return result.stderr.trim().slice(0, 400) || "未知错误：ensure 失败且无输出";
}

async function runDesktopEnsure(forceRestart: boolean): Promise<LaunchState> {
  const host = getHost();
  if (!host.tools?.run) {
    return {
      kind: "error",
      detail: "宿主未提供 tools.run（需 Agnx 桌面端）",
    };
  }
  const args = ["--supervised"];
  if (forceRestart) {
    args.push("--force-restart");
  }
  const result = await host.tools.run({
    appId: host.appId ?? APP_ID,
    tool: ENSURE_TOOL,
    args,
    timeoutMs: 120_000,
  });
  if (!result.ok) {
    return { kind: "error", detail: formatEnsureError(result) };
  }
  const json =
    result.resultJson && typeof result.resultJson === "object"
      ? (result.resultJson as {
          ok?: boolean;
          message?: string;
          desktopPid?: number;
          reused?: boolean;
          error?: string;
        })
      : null;
  if (json && json.ok === false) {
    return {
      kind: "error",
      detail: [json.error, json.message].filter(Boolean).join(" — ") || "ensure 返回 ok:false",
    };
  }
  const desktopPid = typeof json?.desktopPid === "number" ? json.desktopPid : undefined;
  const reused = json?.reused === true;
  const detail =
    typeof json?.message === "string" && json.message.trim()
      ? json.message.trim()
      : reused
        ? "Maka Desktop 已在运行（独立窗）"
        : "已拉起 Maka Desktop 独立窗";
  return { kind: "ok", detail, desktopPid, reused };
}

export function MakaIndependentWindowPanel(): ReactElement {
  const [state, setState] = useState<LaunchState>({ kind: "idle" });

  const launch = useCallback(async (forceRestart: boolean) => {
    setState({
      kind: "running",
      detail: forceRestart ? "正在重启 Maka Desktop…" : "正在打开 Maka Desktop 独立窗…",
    });
    try {
      const next = await runDesktopEnsure(forceRestart);
      setState(next);
    } catch (error) {
      setState({
        kind: "error",
        detail: error instanceof Error ? error.message : String(error),
      });
    }
  }, []);

  useEffect(() => {
    void launch(false);
  }, [launch]);

  const statusText =
    state.kind === "idle"
      ? "准备打开…"
      : state.kind === "running"
        ? state.detail
        : state.kind === "ok"
          ? state.detail +
            (typeof state.desktopPid === "number" ? `（pid ${state.desktopPid}）` : "")
          : state.detail;

  return (
    <div
      style={ROOT_STYLE}
      data-testid="workspace-mini-app-maka-root"
      aria-label={`${APP_TITLE} panel`}
    >
      <h1 style={TITLE_STYLE}>{APP_TITLE}</h1>
      <p style={BODY_STYLE}>
        Maka 以<strong>独立窗口</strong>运行（不嵌 Agnx Tab）。本面板只负责拉起 / 重启桌面进程。
      </p>
      <p style={BODY_STYLE} data-testid="workspace-mini-app-maka-status">
        {statusText}
      </p>
      <div style={ROW_STYLE}>
        <button
          type="button"
          style={BTN_PRIMARY_STYLE}
          disabled={state.kind === "running"}
          onClick={() => void launch(false)}
        >
          打开 / 聚焦
        </button>
        <button
          type="button"
          style={BTN_STYLE}
          disabled={state.kind === "running"}
          onClick={() => void launch(true)}
        >
          强制重启
        </button>
      </div>
    </div>
  );
}

/** @deprecated 旧名保留，避免历史 import 断裂 */
export const GuestElectronModulePanel = MakaIndependentWindowPanel;

export function activate(host: WorkspaceAppHostApi): () => void {
  bindVendorHarnessHost(host);
  const disposers: Array<() => void> = [];

  disposers.push(
    host.registerMiniApp({
      id: PANEL_ID,
      appId: APP_ID,
      title: APP_TITLE,
      component: MakaIndependentWindowPanel,
    }),
  );

  disposers.push(
    host.registerCommand({
      id: "maka.open",
      appId: APP_ID,
      title: APP_TITLE,
      run: (context) => {
        host.openSubAppTab({
          appId: host.appId ?? APP_ID,
          panelId: PANEL_ID,
          serverId: context?.serverId,
          workspaceId: context?.workspaceId,
          currentPathname: context?.currentPathname,
        });
      },
    }),
  );

  return () => {
    for (const d of disposers.toReversed()) d();
  };
}
