Computer Use 是独立且持久的 JavaScript REPL，直接调用 cua_repl；无需 Code Mode。变量与绑定对象在当前 Host 进程内跨调用和 Turn 保留。重启后 REPL 没有历史变量；请遵循模型请求提供的当前 Computer Use 状态。
`cua.computer.target` 标识选定的桌面平台（`mac`、`linux`、`windows`）。WSL 默认选择 Windows 主机；Shell 命令仍可能在 Linux 中运行。

全新 REPL 首次只执行一个入口调用，先阅读返回的文档和初始状态，再选择后续调用。浏览器 ID 只来自已配置的提供方；未配置时，通过 getApp 操作原生浏览器窗口。API 不提供 start_session 等驱动管理操作。

入口（选择目标时自动输出初始状态）：
- `await cua.getState({emit?})`：应用和已配置浏览器清单，包括清单错误。
- `await cua.listApps({emit?})`、`await cua.listWindows({emit?})`。
- `const app = await cua.getApp("Name / bundle ID / app path")` 或 `getApp({windowId})`。
  macOS 会启动缺失的应用。Windows getApp 选择已打开的窗口；先使用来自 cua.listApps() 的 ID 调用 `await cua.computer.launch_app({app: appId})`，再刷新清单并选择确切窗口。启动前会重新核对原生清单中的 ID，不接受任意可执行文件路径。名称有歧义时选择确切窗口。Linux 要求应用已打开。
- `await cua.listBrowsers({emit?})`、`await cua.listTabs({browser?, emit?})`。
- `const browser = await cua.getBrowser({id? , url?})`：只选择，不打开标签页。
- `const tab = await cua.getTab("tab ID", {browser?})` 或 `getTab({url}, {browser?})`。
- `await cua.createBrowserTab(browserId, url?)`：在已配置的配置文件中打开。不可用的提供方、标签页提及和不支持的创建选项会抛出错误。

App 和 Tab 观察：
- `getAXState({emit?, disableDiffing?})` 返回文本。标签页和 macOS 应用默认返回差异；Windows 和 Linux 应用返回完整状态。
- `getScreenshot({emit?})` 返回 Uint8Array 并输出图像。
- `getAXStateAndScreenshot(options?)` 返回 `{state, screenshot}` 并输出两者。
发现、选择和观察会自动输出结果，直接调用即可。用 `nodeRepl.emitImage()` 包装 `getScreenshot()` 或用 `nodeRepl.write()` 包装 `getAXState()` 会重复输出。自行处理结果时使用 `{emit:false}`；清单错误仍会显示。同一次调用内相同图像只交付一次；之后的调用可以再次输出相同图像。
观察包含短暂 UI 更新的有限稳定等待（标签页 200 ms，应用 400 ms），不会等待任意异步任务，持续动画不会延长窗口。之后的更新需重新观察。仅截图的观察会清除无障碍索引。
若应用忙碌导致原生 AX 观察失败，待应用恢复后刷新同一绑定对象。失败的观察会使旧索引和坐标失效。

App 和 Tab 操作：
- `moveCursor(index | [x,y])` 用合成光标指示已观察的位置。坐标与 `click` 使用同一张当前截图，指示不会消耗截图。返回的可见性是显示反馈，不证明输入效果。
- `click(index | [x,y], {mouseButton?, clickCount?})`、`drag([x,y], [x,y])`。
- `scroll(index | [x,y], "up" | "down" | "left" | "right", pages?)`。
- `setValue(index, string)`。
- `selectText(index, text, {prefix?, suffix?, selectionType?})`，selectionType 为 `text`、`cursor_before` 或 `cursor_after`。原生选择仅支持 macOS。`text_not_found` 表示文本或给定上下文不存在，请重新读取当前状态。`ambiguous_text` 需要更具体的 prefix/suffix。
- `performSecondaryAction(index, action)`：使用原生 AX 观察中列出的确切操作。CDP 不提供次要操作，会拒绝此方法。

App 输入：`typeText(text)`、`paste(text, {format?})`、`pressKey(key)`。
Tab 输入：`typeText(index|null, text)`、`paste(index|null, text, {format?})`、`pressKey(index|null, key)`。给定索引会先聚焦已观察的字段，`null` 表示当前焦点。按键示例：`Return`、`Tab`、`super+a`、`shift+Left`。
粘贴格式：`text`（默认）、`md`（将 Markdown 源码作为纯文本）和 `html`。原生 HTML 粘贴仅支持 macOS，需要可读的可编辑值和选区；它会临时激活确切窗口、保留选区，并恢复原剪贴板，除非用户已复制更新的内容。值未改变会报告为不确定。浏览器 HTML 粘贴需要已观察的 contenteditable 元素。

Tab 导航：`goto(url)`、`back()`、`forward()`、`reload()`、`close()`。导航支持 HTTP(S) 和 about:blank，并等待可观察的文档。覆盖主要 CDP 目标；跨进程 iframe 无障碍支持尚不完整。
原生滚动接受页数。macOS 和 CDP 也接受 `{pixels:500}` 输入增量；应用可能限制、动画化或转换增量。Windows/Linux 原生适配器目前拒绝像素单位输入。macOS/Windows 索引滚动保留选定的无障碍目标。原生索引点击支持一次语义点击；需要多次点击时使用截图坐标。

确定性操作后，在同一次调用内获取 `getAXState()` 再决定下一步。观察前不要猜测等待时长。输入派发成功不证明用户目标已达成，请验证结果状态。
坐标属于最新截图。移动/缩放、导航、滚动或任何操作都会消耗或使映射失效；再次按坐标操作前重新截图。原生语义操作使用保留的无障碍元素。应用菜单仅限所属进程和确切绑定窗口，关闭菜单的内容不会显示。菜单交互可能激活绑定窗口，并在菜单打开时保持激活。
macOS/Windows 坐标和键盘输入通过确切窗口的前台路径交付；macOS 在支持时恢复先前应用。Linux 保持后台交付。坐标输入可能移动系统指针。未验证的输入效果会在调用输出中报告，请先观察再决定下一步。没有公开的 `delivery_mode` 选项，也不会自动重放。

`nodeRepl.write(value)` 输出文本/JSON。`await nodeRepl.emitImage(bytes | dataUrl | {bytes, mimeType})` 输出图像。不提供文件/HTTP 图像 URL、Node、任意文件系统和网络 API。`cua.rewriteDocumentation()` 重新输出此参考文档。

每次操作都使用当前 Host 权限；JS 对象不保留授权。Ask 模式下，Computer Use 批准在 REPL 执行超时计时开始前完成。macOS 原生操作在屏幕锁定时报告 `desktop_locked`；已连接 CDP 提供方和纯 REPL 计算仍可独立使用。
每个 Session 有独立的 Maka 光标。输入操作自动显示光标；仅指示而不操作时才调用 `moveCursor`。`cua.cursor.configure({label?, color?, enabled?, reducedMotion?})` 修改此 Session 的显示。颜色为 `blue`、`mint`、`violet`、`amber`、`rose`、`cyan`；标签最多 48 个字符。`cua.cursor.getState()` 报告设置及原生/浏览器可见性。处于 `idle` 的原生渲染器在首次原生指示时启动；`unavailable` 表示显示主机缺失或失败。重新配置可重试失败的渲染器。空闲光标保留至禁用或清理。光标不共享输入权限，也不移动系统指针。多个光标不能隔离物理键盘、应用焦点或输入法。
REPL 被取消、超时或耗尽堆内存后，需要 `cua_reset` 并重新绑定目标。已接纳的原生工作会在返回前结算。绝不自动重放不确定的操作。重置仅清除此 Session 的 REPL 和绑定，用户应用和标签页保持打开。
