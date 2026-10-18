Computer Use 是獨立且持久的 JavaScript REPL，直接呼叫 cua_repl；無需 Code Mode。變數與綁定物件在目前 Host 程序內跨呼叫和 Turn 保留。重新啟動後 REPL 沒有歷史變數；請遵循模型請求提供的目前 Computer Use 狀態。
`cua.computer.target` 識別選定的桌面平台（`mac`、`linux`、`windows`）。WSL 預設選擇 Windows 主機；Shell 命令仍可能在 Linux 中執行。

全新 REPL 首次只執行一個入口呼叫，先閱讀傳回的文件和初始狀態，再選擇後續呼叫。瀏覽器 ID 只來自已設定的提供者；未設定時，透過 getApp 操作原生瀏覽器視窗。API 不提供 start_session 等驅動程式管理操作。

入口（選擇目標時自動輸出初始狀態）：
- `await cua.getState({emit?})`：應用程式和已設定瀏覽器清單，包括清單錯誤。
- `await cua.listApps({emit?})`、`await cua.listWindows({emit?})`。
- `const app = await cua.getApp("Name / bundle ID / app path")` 或 `getApp({windowId})`。
  macOS 會啟動缺少的應用程式。Windows getApp 選擇已開啟的視窗；先使用來自 cua.listApps() 的 ID 呼叫 `await cua.computer.launch_app({app: appId})`，再重新整理清單並選擇確切視窗。啟動前會重新核對原生清單中的 ID，不接受任意可執行檔路徑。名稱有歧義時選擇確切視窗。Linux 要求應用程式已開啟。
- `await cua.listBrowsers({emit?})`、`await cua.listTabs({browser?, emit?})`。
- `const browser = await cua.getBrowser({id? , url?})`：只選擇，不開啟分頁。
- `const tab = await cua.getTab("tab ID", {browser?})` 或 `getTab({url}, {browser?})`。
- `await cua.createBrowserTab(browserId, url?)`：在已設定的設定檔中開啟。無法使用的提供者、分頁提及和不支援的建立選項會擲出錯誤。

App 和 Tab 觀察：
- `getAXState({emit?, disableDiffing?})` 傳回文字。分頁和 macOS 應用程式預設傳回差異；Windows 和 Linux 應用程式傳回完整狀態。
- `getScreenshot({emit?})` 傳回 Uint8Array 並輸出影像。
- `getAXStateAndScreenshot(options?)` 傳回 `{state, screenshot}` 並輸出兩者。
探索、選擇和觀察會自動輸出結果，直接呼叫即可。用 `nodeRepl.emitImage()` 包裝 `getScreenshot()` 或用 `nodeRepl.write()` 包裝 `getAXState()` 會重複輸出。自行處理結果時使用 `{emit:false}`；清單錯誤仍會顯示。同一次呼叫內相同影像只交付一次；之後的呼叫可以再次輸出相同影像。
觀察包含短暫 UI 更新的有限穩定等待（分頁 200 ms，應用程式 400 ms），不會等待任意非同步工作，持續動畫不會延長視窗。之後的更新需重新觀察。僅擷取畫面的觀察會清除輔助使用索引。
若應用程式忙碌導致原生 AX 觀察失敗，待應用程式恢復後重新整理同一綁定物件。失敗的觀察會使舊索引和座標失效。

App 和 Tab 操作：
- `moveCursor(index | [x,y])` 用合成游標指示已觀察的位置。座標與 `click` 使用同一張目前擷取畫面，指示不會消耗畫面。傳回的可見性是顯示回饋，不證明輸入效果。
- `click(index | [x,y], {mouseButton?, clickCount?})`、`drag([x,y], [x,y])`。
- `scroll(index | [x,y], "up" | "down" | "left" | "right", pages?)`。
- `setValue(index, string)`。
- `selectText(index, text, {prefix?, suffix?, selectionType?})`，selectionType 為 `text`、`cursor_before` 或 `cursor_after`。原生選取僅支援 macOS。`text_not_found` 表示文字或指定上下文不存在，請重新讀取目前狀態。`ambiguous_text` 需要更具體的 prefix/suffix。
- `performSecondaryAction(index, action)`：使用原生 AX 觀察中列出的確切操作。CDP 不提供次要操作，會拒絕此方法。

App 輸入：`typeText(text)`、`paste(text, {format?})`、`pressKey(key)`。
Tab 輸入：`typeText(index|null, text)`、`paste(index|null, text, {format?})`、`pressKey(index|null, key)`。指定索引會先聚焦已觀察的欄位，`null` 表示目前焦點。按鍵範例：`Return`、`Tab`、`super+a`、`shift+Left`。
貼上格式：`text`（預設）、`md`（將 Markdown 原始碼作為純文字）和 `html`。原生 HTML 貼上僅支援 macOS，需要可讀的可編輯值和選取範圍；它會暫時啟用確切視窗、保留選取範圍，並還原原剪貼簿，除非使用者已複製更新的內容。值未改變會回報為不確定。瀏覽器 HTML 貼上需要已觀察的 contenteditable 元素。

Tab 導覽：`goto(url)`、`back()`、`forward()`、`reload()`、`close()`。導覽支援 HTTP(S) 和 about:blank，並等待可觀察的文件。涵蓋主要 CDP 目標；跨程序 iframe 輔助使用支援尚不完整。
原生捲動接受頁數。macOS 和 CDP 也接受 `{pixels:500}` 輸入增量；應用程式可能限制、動畫化或轉換增量。Windows/Linux 原生配接器目前拒絕像素單位輸入。macOS/Windows 索引捲動保留選定的輔助使用目標。原生索引點擊支援一次語義點擊；需要多次點擊時使用擷取畫面座標。

確定性操作後，在同一次呼叫內取得 `getAXState()` 再決定下一步。觀察前不要猜測等待時間。輸入派發成功不證明使用者目標已達成，請驗證結果狀態。
座標屬於最新擷取畫面。移動/縮放、導覽、捲動或任何操作都會消耗或使映射失效；再次按座標操作前重新擷取畫面。原生語義操作使用保留的輔助使用元素。應用程式選單僅限所屬程序和確切綁定視窗，關閉選單的內容不會顯示。選單互動可能啟用綁定視窗，並在選單開啟時保持啟用。
macOS/Windows 座標和鍵盤輸入透過確切視窗的前景路徑交付；macOS 在支援時還原先前應用程式。Linux 保持背景交付。座標輸入可能移動系統指標。未驗證的輸入效果會在呼叫輸出中回報，請先觀察再決定下一步。沒有公開的 `delivery_mode` 選項，也不會自動重播。

`nodeRepl.write(value)` 輸出文字/JSON。`await nodeRepl.emitImage(bytes | dataUrl | {bytes, mimeType})` 輸出影像。不提供檔案/HTTP 影像 URL、Node、任意檔案系統和網路 API。`cua.rewriteDocumentation()` 重新輸出此參考文件。

每次操作都使用目前 Host 權限；JS 物件不保留授權。Ask 模式下，Computer Use 核准在 REPL 執行逾時計時開始前完成。macOS 原生操作在螢幕鎖定時回報 `desktop_locked`；已連線 CDP 提供者和純 REPL 計算仍可獨立使用。
每個 Session 有獨立的 Maka 游標。輸入操作自動顯示游標；僅指示而不操作時才呼叫 `moveCursor`。`cua.cursor.configure({label?, color?, enabled?, reducedMotion?})` 修改此 Session 的顯示。顏色為 `blue`、`mint`、`violet`、`amber`、`rose`、`cyan`；標籤最多 48 個字元。`cua.cursor.getState()` 回報設定及原生/瀏覽器可見性。處於 `idle` 的原生繪製器在首次原生指示時啟動；`unavailable` 表示顯示主機缺少或失敗。重新設定可重試失敗的繪製器。閒置游標保留至停用或清理。游標不共享輸入權限，也不移動系統指標。多個游標不能隔離實體鍵盤、應用程式焦點或輸入法。
REPL 被取消、逾時或耗盡堆積記憶體後，需要 `cua_reset` 並重新綁定目標。已接納的原生工作會在傳回前結算。絕不自動重播不確定的操作。重設僅清除此 Session 的 REPL 和綁定，使用者應用程式和分頁保持開啟。
