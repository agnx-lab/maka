# Maka × Agnx 嵌入纪律（外置仓）

## 主路径（2026-10 起）：独立窗

| 模式 | 实际干什么 |
|------|------------|
| **侧栏打开** | 薄 Panel → `ensure-maka-desktop-window.mjs --supervised` → spawn 官方 Desktop Electron |
| **产品面** | Maka **自己的**主进程 / preload / Runtime Host（Agnx **不**桥 `window.maka`） |
| **禁止** | Tab 内 `mountElectronModule` / WebContentsView 硬嵌整壳；官方 `preload.cjs` 塞进 Agnx WCV |

```text
侧栏 maka → openSubAppTab(maka.main)
  → panel：host.tools.run(ensure-maka-desktop-window)
  → Electron 独立窗（apps/desktop）
  → 用户在独立窗内建 session / 发消息
```

## 调试 vs 安装

| 模式 | Agnx `launchMode` | 实际干什么 |
|------|-------------------|------------|
| **调试（开发默认）** | `debug` | `agnx-debug-watch`：只 rebuild `.agnx` + bump `.agnx-debug-rev` |
| **安装** | `packaged` | 用已构建 `.agnx/dist` |
| 官方桌面 HMR | （手动） | 仓根 `npm run dev:desktop`（与 Agnx 侧栏 ensure 二选一，勿叠床架屋） |

`apps/dev/maka` **必须**是指向 `apps/maka` 的软链（或根本不存在）。
