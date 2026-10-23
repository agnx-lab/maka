var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// src/index.ts
var index_exports = {};
__export(index_exports, {
  activate: () => activate
});
module.exports = __toCommonJS(index_exports);

// src/panel.tsx
var import_react = require("react");
var import_jsx_runtime = require("react/jsx-runtime");
var APP_ID = "maka";
var APP_TITLE = "Apache Maka";
var PANEL_ID = "maka.main";
var ENSURE_TOOL = ".agnx/tools/ensure-maka-desktop-window.mjs";
var ROOT_STYLE = {
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
  fontFamily: "ui-sans-serif, system-ui, sans-serif"
};
var TITLE_STYLE = {
  margin: 0,
  fontSize: 22,
  fontWeight: 600,
  letterSpacing: "-0.02em"
};
var BODY_STYLE = {
  margin: 0,
  maxWidth: 420,
  fontSize: 14,
  lineHeight: 1.5,
  color: "#a1a1aa",
  textAlign: "center"
};
var ROW_STYLE = {
  display: "flex",
  flexWrap: "wrap",
  gap: 10,
  justifyContent: "center"
};
var BTN_STYLE = {
  appearance: "none",
  border: "1px solid #3f3f46",
  background: "#18181b",
  color: "#fafafa",
  borderRadius: 8,
  padding: "8px 14px",
  fontSize: 13,
  cursor: "pointer"
};
var BTN_PRIMARY_STYLE = {
  ...BTN_STYLE,
  background: "#27272a",
  borderColor: "#52525b"
};
var boundHost = null;
function getHost() {
  if (!boundHost) {
    throw new Error(`${APP_TITLE} host \u672A\u7ED1\u5B9A`);
  }
  return boundHost;
}
function bindVendorHarnessHost(host) {
  boundHost = host;
}
function formatEnsureError(result) {
  if (result.timedOut) {
    return "\u62C9\u8D77\u8D85\u65F6\uFF1Aensure-maka-desktop-window \u672A\u5728\u65F6\u9650\u5185\u8FD4\u56DE";
  }
  const json = result.resultJson;
  if (json && typeof json === "object") {
    const row = json;
    const message = typeof row.message === "string" ? row.message : "";
    const error = typeof row.error === "string" ? row.error : "";
    if (message || error) {
      return [error, message].filter(Boolean).join(" \u2014 ");
    }
  }
  const stdout = result.stdout.trim();
  if (stdout) {
    try {
      const parsed = JSON.parse(stdout.split("\n").find((l) => l.startsWith("{")) || "");
      if (parsed && typeof parsed === "object") {
        const row = parsed;
        return [row.error, row.message].filter(Boolean).join(" \u2014 ") || stdout.slice(0, 400);
      }
    } catch {
    }
    return stdout.slice(0, 400);
  }
  return result.stderr.trim().slice(0, 400) || "\u672A\u77E5\u9519\u8BEF\uFF1Aensure \u5931\u8D25\u4E14\u65E0\u8F93\u51FA";
}
async function runDesktopEnsure(forceRestart) {
  const host = getHost();
  if (!host.tools?.run) {
    return {
      kind: "error",
      detail: "\u5BBF\u4E3B\u672A\u63D0\u4F9B tools.run\uFF08\u9700 Agnx \u684C\u9762\u7AEF\uFF09"
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
    timeoutMs: 12e4
  });
  if (!result.ok) {
    return { kind: "error", detail: formatEnsureError(result) };
  }
  const json = result.resultJson && typeof result.resultJson === "object" ? result.resultJson : null;
  if (json && json.ok === false) {
    return {
      kind: "error",
      detail: [json.error, json.message].filter(Boolean).join(" \u2014 ") || "ensure \u8FD4\u56DE ok:false"
    };
  }
  const desktopPid = typeof json?.desktopPid === "number" ? json.desktopPid : void 0;
  const reused = json?.reused === true;
  const detail = typeof json?.message === "string" && json.message.trim() ? json.message.trim() : reused ? "Maka Desktop \u5DF2\u5728\u8FD0\u884C\uFF08\u72EC\u7ACB\u7A97\uFF09" : "\u5DF2\u62C9\u8D77 Maka Desktop \u72EC\u7ACB\u7A97";
  return { kind: "ok", detail, desktopPid, reused };
}
function MakaIndependentWindowPanel() {
  const [state, setState] = (0, import_react.useState)({ kind: "idle" });
  const launch = (0, import_react.useCallback)(async (forceRestart) => {
    setState({
      kind: "running",
      detail: forceRestart ? "\u6B63\u5728\u91CD\u542F Maka Desktop\u2026" : "\u6B63\u5728\u6253\u5F00 Maka Desktop \u72EC\u7ACB\u7A97\u2026"
    });
    try {
      const next = await runDesktopEnsure(forceRestart);
      setState(next);
    } catch (error) {
      setState({
        kind: "error",
        detail: error instanceof Error ? error.message : String(error)
      });
    }
  }, []);
  (0, import_react.useEffect)(() => {
    void launch(false);
  }, [launch]);
  const statusText = state.kind === "idle" ? "\u51C6\u5907\u6253\u5F00\u2026" : state.kind === "running" ? state.detail : state.kind === "ok" ? state.detail + (typeof state.desktopPid === "number" ? `\uFF08pid ${state.desktopPid}\uFF09` : "") : state.detail;
  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(
    "div",
    {
      style: ROOT_STYLE,
      "data-testid": "workspace-mini-app-maka-root",
      "aria-label": `${APP_TITLE} panel`,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", { style: TITLE_STYLE, children: APP_TITLE }),
        /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", { style: BODY_STYLE, children: [
          "Maka \u4EE5",
          /* @__PURE__ */ (0, import_jsx_runtime.jsx)("strong", { children: "\u72EC\u7ACB\u7A97\u53E3" }),
          "\u8FD0\u884C\uFF08\u4E0D\u5D4C Agnx Tab\uFF09\u3002\u672C\u9762\u677F\u53EA\u8D1F\u8D23\u62C9\u8D77 / \u91CD\u542F\u684C\u9762\u8FDB\u7A0B\u3002"
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", { style: BODY_STYLE, "data-testid": "workspace-mini-app-maka-status", children: statusText }),
        /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { style: ROW_STYLE, children: [
          /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
            "button",
            {
              type: "button",
              style: BTN_PRIMARY_STYLE,
              disabled: state.kind === "running",
              onClick: () => void launch(false),
              children: "\u6253\u5F00 / \u805A\u7126"
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime.jsx)(
            "button",
            {
              type: "button",
              style: BTN_STYLE,
              disabled: state.kind === "running",
              onClick: () => void launch(true),
              children: "\u5F3A\u5236\u91CD\u542F"
            }
          )
        ] })
      ]
    }
  );
}
function activate(host) {
  bindVendorHarnessHost(host);
  const disposers = [];
  disposers.push(
    host.registerMiniApp({
      id: PANEL_ID,
      appId: APP_ID,
      title: APP_TITLE,
      component: MakaIndependentWindowPanel
    })
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
          currentPathname: context?.currentPathname
        });
      }
    })
  );
  return () => {
    for (const d of disposers.toReversed()) d();
  };
}
