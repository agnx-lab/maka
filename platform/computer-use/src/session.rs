/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

use crate::{driver::Driver, protocol::*};
use cua_driver_contract::{
    AppInfo, ListAppsOutput, ListWindowsOutput, WindowInfo, WindowStateOutput,
};
use maka_agent_facts::capability::CallResult;
use maka_agent_facts::capability::ContentBlock;
use maka_agent_facts::language::Language;
use maka_agent_facts::tools::ToolError;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

/// Native targets and observation identities have one Session owner. REPL
/// objects contain only random handles; the Host supplies current admission.
pub struct Session {
    native: std::sync::Arc<tokio::sync::Mutex<Driver>>,
    targets: HashMap<String, Target>,
    id: Option<String>,
    closed: bool,
    browsers: crate::browser::Browsers,
    cursor: crate::cursor::Spec,
}
struct Target {
    #[cfg(target_os = "macos")]
    macos: Option<std::sync::Arc<std::sync::Mutex<crate::macos::Target>>>,
    window: WindowInfo,
    process: String,
    snapshot: Option<WindowStateOutput>,
    capture: Option<String>,
    previous: Option<String>,
}
impl Session {
    pub fn new(native: std::sync::Arc<tokio::sync::Mutex<Driver>>) -> Self {
        Self {
            native,
            targets: HashMap::new(),
            id: None,
            closed: false,
            browsers: Default::default(),
            cursor: Default::default(),
        }
    }
    pub(crate) fn target_label(&self, handle: &Handle) -> Option<&str> {
        match handle {
            Handle::App(id) => self
                .targets
                .get(id)
                .map(|target| target.window.app_name.as_str()),
            Handle::Tab(id) => self.browsers.target_label(id),
        }
    }
    pub fn configure_browsers(
        &mut self,
        connections: Vec<maka_capabilities::computer::BrowserConnection>,
        language: Language,
    ) -> Result<(), ToolError> {
        self.browsers.configure(connections, language)
    }
    pub async fn invoke(
        &mut self,
        command: Command,
        session: &str,
        cancellation: &CancellationToken,
        language: Language,
    ) -> Result<Value, ToolError> {
        if cancellation.is_cancelled() {
            return Err(failed("Computer Use cancelled before dispatch"));
        }
        if self.closed {
            return Err(failed("Computer Use Session is closed"));
        }
        if self.id.as_deref().is_some_and(|id| id != session) {
            return Err(failed("Computer Use Session identity mismatch"));
        }
        self.id.get_or_insert_with(|| session.into());
        let mut result = match command {
            Command::Documentation => Ok(json!(crate::language::documentation(language))),
            Command::ConfigureCursor { options } => {
                self.cursor.configure(options)?;
                self.browsers.configure_cursor(&self.cursor, language).await;
                let native = self
                    .native
                    .lock()
                    .await
                    .cursors
                    .configure(&self.cursor)
                    .await;
                Ok(json!({"settings":self.cursor,"native":native}))
            }
            Command::CursorState { .. } => {
                let native = self.native.lock().await.cursors.state(&self.cursor).await;
                Ok(
                    json!({"settings":self.cursor,"native":native,"browsers":self.browsers.cursor_state(&self.cursor, language).await}),
                )
            }
            Command::ListApps { .. } => self.apps(session, cancellation, language).await,
            Command::ListWindows { .. } => Ok(json!(
                self.windows(session, cancellation, language)
                    .await?
                    .iter()
                    .map(window_info)
                    .collect::<Vec<_>>()
            )),
            Command::GetState { .. } => {
                let (browsers, mut errors) = self.browsers.state().await;
                #[cfg(target_os = "macos")]
                if let Err(error) = crate::macos::require_desktop() {
                    errors.push(error.to_string());
                }
                let apps = match self.apps(session, cancellation, language).await {
                    Ok(apps) => apps,
                    Err(error) => {
                        errors.push(error.to_string());
                        json!([])
                    }
                };
                Ok(json!({"apps": apps, "browsers": browsers, "errors":errors}))
            }
            Command::LaunchApp { app } => {
                if !cfg!(target_os = "windows") {
                    return Err(failed(
                        "unsupported: explicit native app launch is Windows-only",
                    ));
                }
                let apps: ListAppsOutput = decode(
                    self.native
                        .lock()
                        .await
                        .invoke("list_apps", json!({}), session, cancellation, language)
                        .await?,
                )?;
                let args = launch_args(&app, &apps.apps, language)?;
                let launched = self
                    .native
                    .lock()
                    .await
                    .invoke("launch_app", args, session, cancellation, language)
                    .await?;
                Ok(launched.structured_content.unwrap_or(Value::Null))
            }
            Command::GetApp { target } => {
                #[cfg(target_os = "macos")]
                crate::macos::require_desktop()?;
                if self.targets.len() >= 64 {
                    return Err(failed(match language {
                        Language::En => "target limit reached; reset Cua to release bindings",
                        Language::ZhCn => "目标数已达上限；请重置 Cua 以释放绑定",
                        Language::ZhTw => "目標數已達上限；請重設 Cua 以釋放綁定",
                    }));
                }
                let windows = self.windows(session, cancellation, language).await?;
                let candidates: Vec<_> = match target {
                    AppReference::Window { window_id } => windows
                        .into_iter()
                        .filter(|w| w.window_id == window_id)
                        .collect(),
                    AppReference::Name(name) => {
                        let apps: ListAppsOutput = decode(
                            self.native
                                .lock()
                                .await
                                .invoke("list_apps", json!({}), session, cancellation, language)
                                .await?,
                        )?;
                        let pids: Vec<_> = apps
                            .apps
                            .iter()
                            .filter(|a| {
                                a.name.eq_ignore_ascii_case(&name)
                                    || a.bundle_id.as_deref() == Some(&name)
                                    || a.launch_path.as_deref() == Some(&name)
                            })
                            .map(|a| a.pid)
                            .collect();
                        let selected: Vec<_> = windows
                            .into_iter()
                            .filter(|w| w.pid.is_some_and(|pid| pids.contains(&pid)))
                            .collect();
                        if selected.is_empty() && cfg!(target_os = "macos") {
                            let bundle = name.contains('.')
                                && !name.contains('/')
                                && !name.contains(' ')
                                && !name.ends_with(".app")
                                && name.chars().all(|c| {
                                    c.is_ascii_lowercase()
                                        || c.is_ascii_digit()
                                        || matches!(c, '.' | '-')
                                });
                            let args = if bundle {
                                json!({"bundle_id":name})
                            } else {
                                json!({"name":name})
                            };
                            let launched = self
                                .native
                                .lock()
                                .await
                                .invoke("launch_app", args, session, cancellation, language)
                                .await?;
                            let pid=launched.structured_content.as_ref().and_then(|value| value["pid"].as_u64()).ok_or_else(|| ToolError::OutcomeUnknown(match language { Language::En => "launch requested but process identity unavailable; inspect inventory before retrying", Language::ZhCn => "已请求启动但无法获取进程标识；重试前请检查清单", Language::ZhTw => "已請求啟動但無法取得程序識別；重試前請檢查清單" }.into()))?;
                            self.windows(session, cancellation, language)
                                .await?
                                .into_iter()
                                .filter(|w| w.pid.map(u64::from) == Some(pid))
                                .collect()
                        } else {
                            selected
                        }
                    }
                };
                if candidates.len() != 1 {
                    return Err(failed(match language {
                        Language::En => format!(
                            "App selection requires exactly one open window; found {}. Choose cua.getApp({{windowId}}) from {}",
                            candidates.len(),
                            json!(candidates.iter().map(window_info).collect::<Vec<_>>())
                        ),
                        Language::ZhCn => format!(
                            "应用选择需要恰好一个打开的窗口；找到 {} 个。请从 {} 中选择 cua.getApp({{windowId}})",
                            candidates.len(),
                            json!(candidates.iter().map(window_info).collect::<Vec<_>>())
                        ),
                        Language::ZhTw => format!(
                            "應用程式選擇需要恰好一個開啟的視窗；找到 {} 個。請從 {} 中選擇 cua.getApp({{windowId}})",
                            candidates.len(),
                            json!(candidates.iter().map(window_info).collect::<Vec<_>>())
                        ),
                    }));
                }
                let window = candidates.into_iter().next().unwrap();
                let pid = window
                    .pid
                    .ok_or_else(|| failed("window process identity unavailable"))?;
                let process = crate::identity::process(pid)?;
                #[cfg(target_os = "macos")]
                let macos = {
                    let id = window.window_id;
                    tokio::task::spawn_blocking(move || crate::macos::Target::bind(pid, id))
                        .await
                        .map_err(failed)?
                        .ok()
                        .map(|target| std::sync::Arc::new(std::sync::Mutex::new(target)))
                };
                let handle = uuid::Uuid::new_v4().to_string();
                self.targets.insert(
                    handle.clone(),
                    Target {
                        #[cfg(target_os = "macos")]
                        macos,
                        window,
                        process,
                        snapshot: None,
                        capture: None,
                        previous: None,
                    },
                );
                let state = match self
                    .observe(
                        &handle,
                        ObservationKind::Ax,
                        false,
                        session,
                        cancellation,
                        language,
                    )
                    .await
                {
                    Ok(value) => value,
                    Err(error) => {
                        self.targets.remove(&handle);
                        return Err(error);
                    }
                };
                let bound = self
                    .targets
                    .get(&handle)
                    .expect("observed target remains bound");
                Ok(
                    json!({"handle":Handle::App(handle.clone()),"kind":"app","state":state["state"],"binding":{"pid":bound.window.pid,"windowId":bound.window.window_id,"processGeneration":bound.process}}),
                )
            }
            Command::Observe {
                handle: Handle::App(handle),
                kind,
                options,
            } => {
                let result = self
                    .observe(
                        &handle,
                        kind,
                        !options.disable_diffing,
                        session,
                        cancellation,
                        language,
                    )
                    .await;
                if result.is_err()
                    && let Some(target) = self.targets.get_mut(&handle)
                {
                    target.invalidate_observation();
                }
                result
            }
            Command::Action {
                handle: Handle::App(handle),
                action,
            } => {
                self.action(&handle, action, session, cancellation, language)
                    .await
            }
            command => self
                .browsers
                .invoke(command, &self.cursor, cancellation, language)
                .await
                .map_err(|error| crate::browser::with_unknown_guidance(error, language)),
        }?;
        if result
            .get("effect")
            .and_then(Value::as_str)
            .is_some_and(|effect| {
                matches!(
                    effect,
                    "unverifiable" | "unchanged" | "partial" | "suspected_noop"
                )
            })
        {
            result["guidance"] = json!(crate::language::uncertain(language));
        }
        Ok(result)
    }
    async fn windows(
        &mut self,
        session: &str,
        cancellation: &CancellationToken,
        language: Language,
    ) -> Result<Vec<WindowInfo>, ToolError> {
        let output: ListWindowsOutput = decode(
            self.native
                .lock()
                .await
                .invoke("list_windows", json!({}), session, cancellation, language)
                .await?,
        )?;
        Ok(output.windows)
    }
    async fn apps(
        &mut self,
        session: &str,
        cancellation: &CancellationToken,
        language: Language,
    ) -> Result<Value, ToolError> {
        let apps: ListAppsOutput = decode(
            self.native
                .lock()
                .await
                .invoke("list_apps", json!({}), session, cancellation, language)
                .await?,
        )?;
        let windows = self.windows(session, cancellation, language).await?;
        Ok(json!(apps.apps.into_iter().map(|app| json!({
            "id":app_id(&app), "displayName":app.name,
            "isRunning":app.running, "windows":windows.iter().filter(|w| w.pid == Some(app.pid)).map(window_info).collect::<Vec<_>>()
        })).collect::<Vec<_>>()))
    }
    async fn verify(
        &mut self,
        handle: &str,
        session: &str,
        cancellation: &CancellationToken,
        language: Language,
    ) -> Result<WindowInfo, ToolError> {
        let target = self.targets.get(handle).ok_or_else(|| {
            failed(match language {
                Language::En => "stale_target: bind the app again",
                Language::ZhCn => "stale_target: 请重新绑定应用",
                Language::ZhTw => "stale_target: 請重新綁定應用程式",
            })
        })?;
        #[cfg(target_os = "macos")]
        crate::macos::require_desktop()?;
        let pid = target
            .window
            .pid
            .ok_or_else(|| failed("window has no process"))?;
        if crate::identity::process(pid).ok().as_ref() != Some(&target.process) {
            self.targets.remove(handle);
            return Err(failed(match language {
                Language::En => "stale_target: application process ended or changed; bind it again",
                Language::ZhCn => "stale_target: 应用进程已结束或改变；请重新绑定",
                Language::ZhTw => "stale_target: 應用程式程序已結束或變更；請重新綁定",
            }));
        }
        let id = target.window.window_id;
        #[cfg(target_os = "macos")]
        let macos = target.macos.clone();
        let windows = self.windows(session, cancellation, language).await?;
        let Some(window) = windows
            .into_iter()
            .find(|w| w.pid == Some(pid) && w.window_id == id)
        else {
            self.targets.remove(handle);
            return Err(failed(match language {
                Language::En => "stale_target: window closed; bind a current window",
                Language::ZhCn => "stale_target: 窗口已关闭；请绑定当前窗口",
                Language::ZhTw => "stale_target: 視窗已關閉；請綁定目前視窗",
            }));
        };
        #[cfg(target_os = "macos")]
        let window = if let Some(macos) = macos {
            let verified = tokio::task::spawn_blocking(move || {
                let target = macos.lock().unwrap();
                target.verify()?;
                target.geometry()
            })
            .await
            .map_err(failed)?;
            match verified {
                Ok(bounds) => WindowInfo { bounds, ..window },
                Err(error) => {
                    // AX timeouts do not prove that the window was replaced.
                    // Keep its retained identity, but require a fresh observation
                    // before any indexed or coordinate input can resume.
                    self.targets
                        .get_mut(handle)
                        .unwrap()
                        .invalidate_observation();
                    return Err(error);
                }
            }
        } else {
            window
        };
        Ok(window)
    }
    async fn observe(
        &mut self,
        handle: &str,
        kind: ObservationKind,
        diff: bool,
        session: &str,
        cancellation: &CancellationToken,
        language: Language,
    ) -> Result<Value, ToolError> {
        self.verify(handle, session, cancellation, language).await?;
        let ax = !matches!(kind, ObservationKind::Screenshot);
        let screenshot = !matches!(kind, ObservationKind::Ax);
        // Clear old authority before attempting observation; failure cannot leave
        // a misleading older capture active under a newer model observation.
        let target = self.targets.get_mut(handle).unwrap();
        target.snapshot = None;
        target.capture = None;
        #[cfg(target_os = "macos")]
        let own_ax = target.macos.is_some();
        #[cfg(not(target_os = "macos"))]
        let own_ax = false;
        #[cfg(target_os = "macos")]
        if let Some(macos) = target.macos.clone() {
            macos.lock().unwrap().clear();
        }
        crate::observation::settle(crate::observation::NATIVE_SETTLE, cancellation).await?;
        // The application may move or close during the settling window.
        let window = self.verify(handle, session, cancellation, language).await?;
        let result = if !own_ax || screenshot {
            self.native.lock().await.invoke("get_window_state", json!({"pid":window.pid,"window_id":window.window_id,"include_accessibility_tree":ax && !own_ax,"include_screenshot":screenshot,"max_elements":1000,"max_image_dimension":1600}), session, cancellation, language).await?
        } else {
            CallResult {
                content: vec![],
                structured_content: Some(json!({"pid":window.pid,"window_id":window.window_id})),
            }
        };
        let capture = result
            .structured_content
            .as_ref()
            .and_then(|v| v["capture_id"].as_str())
            .map(str::to_owned);
        let image = result
            .content
            .iter()
            .find(|block| matches!(block, ContentBlock::Image { .. }))
            .cloned();
        let snapshot: WindowStateOutput = decode(result)?;
        if snapshot.pid != window.pid.unwrap() || snapshot.window_id != window.window_id {
            return Err(failed("observation target changed"));
        }
        if screenshot && image.is_none() {
            return Err(failed("screenshot_unavailable: provider returned no image"));
        }
        if screenshot
            && self
                .verify(handle, session, cancellation, language)
                .await?
                .bounds
                != window.bounds
        {
            return Err(failed(match language {
                Language::En => "window geometry changed during capture; capture again",
                Language::ZhCn => "截图期间窗口几何信息已改变；请重新截图",
                Language::ZhTw => "擷取畫面期間視窗幾何資訊已變更；請重新擷取畫面",
            }));
        }
        let target = self.targets.get_mut(handle).unwrap();
        let mut output = serde_json::Map::new();
        if ax {
            let full = snapshot.tree_markdown.clone().unwrap_or_default();
            #[cfg(target_os = "macos")]
            let full = if let Some(macos) = target.macos.clone() {
                tokio::task::spawn_blocking(move || macos.lock().unwrap().observe())
                    .await
                    .map_err(failed)??
            } else {
                full
            };
            // Retain raw observation rows, never localized framing, as the
            // diff basis across evaluations with different captured languages.
            let body = if diff && cfg!(target_os = "macos") {
                display_diff(target.previous.as_deref(), &full, language)
            } else {
                full.clone()
            };
            let (window_label, app_label) = match language {
                Language::En => ("Window", "App"),
                Language::ZhCn => ("窗口", "应用"),
                Language::ZhTw => ("視窗", "應用程式"),
            };
            let mut state = format!(
                "{window_label}: {:?}, {app_label}: {:?}.\n{body}",
                window.title, window.app_name
            );
            if snapshot.truncated == Some(true) || snapshot.degraded == Some(true) {
                let (label, fallback) = match language {
                    Language::En => ("Observation incomplete", "partial accessibility tree"),
                    Language::ZhCn => ("观察不完整", "部分无障碍树"),
                    Language::ZhTw => ("觀察不完整", "部分輔助使用樹"),
                };
                let reason = snapshot
                    .truncation_reason
                    .as_deref()
                    .or(snapshot.degraded_reason.as_deref())
                    .unwrap_or(fallback);
                state.push_str(&format!("\n{label}: {reason}"));
            }
            target.previous = Some(full);
            output.insert("state".into(), json!(state));
        } else {
            target.previous = None;
        }
        if let Some(image) = image {
            output.insert("image".into(), json!(image));
        }
        target.window = window;
        target.snapshot = Some(snapshot);
        target.capture = capture;
        Ok(Value::Object(output))
    }
    async fn action(
        &mut self,
        handle: &str,
        action: Action,
        session: &str,
        cancellation: &CancellationToken,
        language: Language,
    ) -> Result<Value, ToolError> {
        let window = self.verify(handle, session, cancellation, language).await?;
        let cursor_requested = matches!(action, Action::MoveCursor { .. });
        let point = if cursor_requested
            || (self.cursor.enabled && self.native.lock().await.cursors.registered())
        {
            self.cursor_point(handle, action.cursor_target(), &window, language)
                .await
        } else {
            Err(failed("automatic native cursor disabled"))
        };
        if matches!(action, Action::MoveCursor { .. }) {
            return self
                .native
                .lock()
                .await
                .cursors
                .update(
                    &self.cursor,
                    Some(point?),
                    cursor_overlay::CursorAction::Idle,
                    Some(window.window_id),
                )
                .await
                .map(|state| json!(state));
        }
        if self.cursor.enabled
            && let Ok(point) = point
        {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(failed("Computer Use cancelled before input dispatch")),
                _ = async {
                    let _ = self.native.lock().await.cursors.update(
                        &self.cursor,
                        Some(point),
                        action.cursor_action(),
                        Some(window.window_id),
                    ).await;
                } => {}
            }
        }
        if cancellation.is_cancelled() {
            return Err(failed("Computer Use cancelled before input dispatch"));
        }
        let target = self.targets.get(handle).unwrap();
        #[cfg(target_os = "macos")]
        if crate::macos::Target::handles(&action)
            && let Some(macos) = target.macos.clone()
        {
            let pixel_scroll = matches!(
                &action,
                Action::Scroll {
                    distance: Some(Distance::Pixels { .. }),
                    ..
                }
            );
            let cancellation = cancellation.clone();
            let result = tokio::task::spawn_blocking(move || {
                macos.lock().unwrap().action(action, &cancellation)
            })
            .await
            .map_err(|error| ToolError::CleanupUnconfirmed(error.to_string()))?;
            self.targets.get_mut(handle).unwrap().capture = None;
            return result.map(|()| {
                if pixel_scroll {
                    json!({"effect":"unverifiable","route":"quartz"})
                } else {
                    Value::Null
                }
            });
        }
        let bound = json!({"kind":"window","pid":window.pid,"window_id":window.window_id});
        #[cfg(target_os = "macos")]
        if let Action::Paste {
            index: None,
            text,
            options,
        } = action
        {
            let html = matches!(options.format, Some(PasteFormat::Html));
            let macos = target
                .macos
                .clone()
                .ok_or_else(|| failed("paste requires an exact native accessibility target"))?;
            let result =
                tokio::task::spawn_blocking(move || macos.lock().unwrap().paste(&text, html))
                    .await
                    .map_err(|error| ToolError::CleanupUnconfirmed(error.to_string()))?;
            self.targets.get_mut(handle).unwrap().capture = None;
            let restored = result?;
            return Ok(if restored {
                Value::Null
            } else {
                json!({"warning":match language { Language::En => "Clipboard changed during paste; preserved the newer clipboard contents.", Language::ZhCn => "粘贴期间剪贴板已改变；已保留较新的剪贴板内容。", Language::ZhTw => "貼上期間剪貼簿已變更；已保留較新的剪貼簿內容。" }})
            });
        }
        let (name, args) = match action {
            Action::Click {
                target: position,
                options,
            } => {
                let count = options.click_count.unwrap_or(1);
                if !(1..=3).contains(&count) {
                    return Err(failed("clickCount must be between 1 and 3"));
                }
                let mut args = json!({"target":bound,"button":options.mouse_button.unwrap_or(MouseButton::Left).name(),"count":count,"delivery_mode":native_delivery_mode()});
                match position {
                    Position::Element(index) => {
                        args["element_token"] =
                            json!(target.element(index, language)?.element_token)
                    }
                    Position::Point(point) => {
                        target.point(point, &window, language)?;
                        args["x"] = json!(point[0]);
                        args["y"] = json!(point[1]);
                        args["capture_id"] = json!(target.capture);
                    }
                }
                ("click", args)
            }
            Action::SetValue { index, value } => (
                "set_value",
                json!({"pid":window.pid,"window_id":window.window_id,"element_token":target.element(index, language)?.element_token,"value":value}),
            ),
            Action::Secondary { index, action } if cfg!(target_os = "macos") => {
                let element = target.element(index, language)?;
                let (native, name) = match action.as_str() {
                    "AXShowMenu" | "Show Menu" => ("AXShowMenu", "show_menu"),
                    "AXPick" | "Pick" => ("AXPick", "pick"),
                    "AXConfirm" | "Confirm" => ("AXConfirm", "confirm"),
                    "AXCancel" | "Cancel" => ("AXCancel", "cancel"),
                    "AXOpen" | "Open" => ("AXOpen", "open"),
                    _ => {
                        return Err(failed(
                            "unsupported: Cua cannot deliver this accessibility action",
                        ));
                    }
                };
                if !element
                    .actions
                    .as_ref()
                    .is_some_and(|actions| actions.iter().any(|a| a == native))
                {
                    return Err(failed(
                        "action_not_exposed: select an action from the current accessibility observation",
                    ));
                }
                (
                    "click",
                    json!({"pid":window.pid,"window_id":window.window_id,"element_token":element.element_token,"action":name,"delivery_mode":"background"}),
                )
            }
            Action::TypeText { index: None, text } => (
                "type_text",
                json!({"target":bound,"text":text,"delivery_mode":native_delivery_mode()}),
            ),
            Action::Paste {
                index: None,
                text,
                options,
            } if matches!(options.format, None | Some(PasteFormat::Text)) => (
                "type_text",
                json!({"target":bound,"text":text,"delivery_mode":native_delivery_mode()}),
            ),
            Action::PressKey { index: None, key } => {
                let mut parts: Vec<_> = key.split('+').collect();
                let key = parts.pop().ok_or_else(|| failed("key must not be empty"))?;
                if key.is_empty() {
                    return Err(failed("key must not be empty"));
                }
                let modifiers: Result<Vec<_>, _> = parts
                    .into_iter()
                    .map(|part| match part.to_ascii_lowercase().as_str() {
                        "super" | "meta" | "cmd" | "command" => Ok(if cfg!(target_os = "macos") {
                            "cmd"
                        } else {
                            "super"
                        }),
                        "ctrl" | "control" => Ok("ctrl"),
                        "alt" | "option" => Ok("alt"),
                        "shift" => Ok("shift"),
                        _ => Err(failed("unsupported key modifier")),
                    })
                    .collect();
                (
                    "press_key",
                    json!({"target":bound,"key":key,"modifiers":modifiers?,"delivery_mode":native_delivery_mode()}),
                )
            }
            Action::Scroll {
                target: position,
                direction,
                distance,
            } => {
                let (by, amount, maximum) = match distance {
                    None => ("page", 1, 50),
                    Some(Distance::Pages(pages)) => ("page", pages, 50),
                    Some(Distance::Pixels { pixels }) if cfg!(target_os = "macos") => {
                        ("pixel", pixels, 20_000)
                    }
                    Some(Distance::Pixels { .. }) => {
                        return Err(failed(
                            "unsupported: pixel-unit input is unavailable on this native adapter",
                        ));
                    }
                };
                if amount == 0 || amount > maximum {
                    return Err(failed("scroll distance out of range"));
                }
                let mut args = json!({"direction":direction.name(),"by":by,"amount":amount,"delivery_mode":native_delivery_mode()});
                match position {
                    Position::Point(point) => {
                        target.point(point, &window, language)?;
                        args["target"] = bound;
                        args["x"] = json!(point[0]);
                        args["y"] = json!(point[1]);
                    }
                    Position::Element(index)
                        if cfg!(any(target_os = "macos", target_os = "windows")) =>
                    {
                        args["pid"] = json!(window.pid);
                        args["window_id"] = json!(window.window_id);
                        args["element_token"] =
                            json!(target.element(index, language)?.element_token);
                        args["delivery_mode"] = json!(native_delivery_mode());
                    }
                    _ => {
                        return Err(failed(
                            "unsupported: element scrolling is unavailable on this platform",
                        ));
                    }
                }
                ("scroll", args)
            }
            Action::Drag { from, to } => {
                target.point(from, &window, language)?;
                target.point(to, &window, language)?;
                (
                    "drag",
                    json!({"target":bound,"from_x":from[0],"from_y":from[1],"to_x":to[0],"to_y":to[1],"delivery_mode":native_delivery_mode()}),
                )
            }
            _ => {
                return Err(failed(
                    "unsupported: this native operation is not implemented by the current adapter",
                ));
            }
        };
        let result = self
            .native
            .lock()
            .await
            .invoke(name, args, session, cancellation, language)
            .await;
        // Every attempt invalidates coordinates. Element tokens stay bound to
        // the retained native nodes and are revalidated by Cua on each action.
        self.targets.get_mut(handle).unwrap().capture = None;
        result.map(|result| result.structured_content.unwrap_or(Value::Null))
    }
    pub async fn close(&mut self) -> Result<(), ToolError> {
        self.closed = true;
        self.targets.clear();
        self.browsers.remove_cursors(&self.cursor).await;
        self.browsers.close();
        self.native
            .lock()
            .await
            .cursors
            .remove(&self.cursor.id)
            .await;
        if let Some(session) = &self.id {
            self.native.lock().await.release_session(session).await?;
        }
        self.id = None;
        Ok(())
    }
    async fn cursor_point(
        &self,
        handle: &str,
        position: Option<Position>,
        window: &WindowInfo,
        language: Language,
    ) -> Result<[f64; 2], ToolError> {
        let target = self
            .targets
            .get(handle)
            .ok_or_else(|| failed("stale_target"))?;
        match position {
            None => Ok([
                window.bounds.x + window.bounds.width / 2.0,
                window.bounds.y + window.bounds.height / 2.0,
            ]),
            Some(Position::Point(point)) => {
                target.point(point, window, language)?;
                let snapshot = target.snapshot.as_ref().unwrap();
                let width = f64::from(snapshot.screenshot_width.unwrap_or(0));
                let height = f64::from(snapshot.screenshot_height.unwrap_or(0));
                if width <= 0.0 || height <= 0.0 {
                    return Err(failed("cursor capture dimensions unavailable"));
                }
                Ok([
                    window.bounds.x + point[0] * window.bounds.width / width,
                    window.bounds.y + point[1] * window.bounds.height / height,
                ])
            }
            Some(Position::Element(index)) => {
                #[cfg(target_os = "macos")]
                if let Some(macos) = target.macos.clone() {
                    return tokio::task::spawn_blocking(move || {
                        macos.lock().unwrap().cursor_point(index)
                    })
                    .await
                    .map_err(failed)?;
                }
                let _ = index;
                Err(failed(match language {
                    Language::En => {
                        "native cursor element geometry unavailable; use screenshot coordinates"
                    }
                    Language::ZhCn => "无法获取原生光标元素几何信息；请使用截图坐标",
                    Language::ZhTw => "無法取得原生游標元素幾何資訊；請使用擷取畫面座標",
                }))
            }
        }
    }
}
impl Target {
    fn invalidate_observation(&mut self) {
        self.snapshot = None;
        self.capture = None;
        self.previous = None;
        #[cfg(target_os = "macos")]
        if let Some(macos) = &self.macos {
            macos.lock().unwrap().clear();
        }
    }
    fn element(
        &self,
        index: u64,
        language: Language,
    ) -> Result<&cua_driver_contract::WindowElement, ToolError> {
        self.snapshot
            .as_ref()
            .and_then(|s| s.elements.as_ref())
            .and_then(|elements| {
                elements
                    .iter()
                    .find(|e| e.element_index == index && e.element_token.is_some())
            })
            .ok_or_else(|| {
                failed(match language {
                    Language::En => "stale_element: read getAXState before using this index",
                    Language::ZhCn => "stale_element: 使用此索引前请先读取 getAXState",
                    Language::ZhTw => "stale_element: 使用此索引前請先讀取 getAXState",
                })
            })
    }
    fn point(
        &self,
        point: [f64; 2],
        window: &WindowInfo,
        language: Language,
    ) -> Result<(), ToolError> {
        let snapshot = self.snapshot.as_ref().ok_or_else(|| {
            failed(match language {
                Language::En => "capture_required: read getScreenshot before coordinate input",
                Language::ZhCn => "capture_required: 按坐标输入前请先读取 getScreenshot",
                Language::ZhTw => "capture_required: 按座標輸入前請先讀取 getScreenshot",
            })
        })?;
        if self.capture.is_none()
            || self.window.bounds != window.bounds
            || snapshot.screenshot_frame_valid == Some(false)
        {
            return Err(failed(match language {
                Language::En => {
                    "stale_capture: window changed or capture was consumed; getScreenshot again"
                }
                Language::ZhCn => "stale_capture: 窗口已改变或截图已消耗；请重新调用 getScreenshot",
                Language::ZhTw => {
                    "stale_capture: 視窗已變更或擷取畫面已消耗；請重新呼叫 getScreenshot"
                }
            }));
        }
        if !point.iter().all(|v| v.is_finite() && *v >= 0.0)
            || point[0] >= f64::from(snapshot.screenshot_width.unwrap_or(0))
            || point[1] >= f64::from(snapshot.screenshot_height.unwrap_or(0))
        {
            return Err(failed("coordinate outside the captured image"));
        }
        Ok(())
    }
}
fn native_delivery_mode() -> &'static str {
    if cfg!(target_os = "linux") {
        "background"
    } else {
        "foreground"
    }
}
fn window_info(window: &WindowInfo) -> Value {
    json!({"id":window.window_id,"app":window.app_name,"title":window.title,"isOnScreen":window.is_on_screen,"minimized":window.minimized,"onCurrentSpace":window.on_current_space,"zIndex":window.z_index})
}
fn decode<T: DeserializeOwned>(result: CallResult) -> Result<T, ToolError> {
    serde_json::from_value(
        result
            .structured_content
            .ok_or_else(|| failed("native structured output unavailable"))?,
    )
    .map_err(|error| ToolError::OutcomeUnknown(format!("invalid native result: {error}")))
}
fn failed(error: impl std::fmt::Display) -> ToolError {
    ToolError::Failed(error.to_string())
}
pub(crate) fn display_diff(previous: Option<&str>, current: &str, language: Language) -> String {
    let Some(previous) = previous else {
        return current.into();
    };
    if previous == current {
        return match language {
            Language::En => "Accessibility state unchanged.",
            Language::ZhCn => "无障碍状态未改变。",
            Language::ZhTw => "輔助使用狀態未變更。",
        }
        .into();
    }
    let old: std::collections::HashSet<_> = previous.lines().collect();
    let new: std::collections::HashSet<_> = current.lines().collect();
    let changes: Vec<_> = previous
        .lines()
        .filter(|line| !new.contains(line))
        .map(|line| format!("- {line}"))
        .chain(
            current
                .lines()
                .filter(|line| !old.contains(line))
                .map(|line| format!("+ {line}")),
        )
        .collect();
    let label = match language {
        Language::En => "Accessibility changes",
        Language::ZhCn => "无障碍变化",
        Language::ZhTw => "輔助使用變更",
    };
    let diff = format!("{label}:\n{}", changes.join("\n"));
    if changes.is_empty() || diff.len() >= current.len() {
        current.into()
    } else {
        diff
    }
}

fn app_id(app: &AppInfo) -> String {
    use sha2::{Digest, Sha256};
    if cfg!(target_os = "windows")
        && app.kind.as_deref() != Some("uwp")
        && let Some(path) = &app.launch_path
    {
        return format!("launch:{}", hex::encode(Sha256::digest(path.as_bytes())));
    }
    if let Some(bundle) = &app.bundle_id {
        return bundle.clone();
    }
    if let Some(path) = &app.launch_path {
        return format!("launch:{}", hex::encode(Sha256::digest(path.as_bytes())));
    }
    format!("pid:{}", app.pid)
}

fn launch_args(app: &str, inventory: &[AppInfo], language: Language) -> Result<Value, ToolError> {
    let mut matches = inventory
        .iter()
        .filter(|candidate| app_id(candidate) == app);
    let target = matches.next().ok_or_else(|| {
        failed(match language {
            Language::En => "app identity missing; refresh cua.listApps before launch",
            Language::ZhCn => "应用标识已缺失；启动前请刷新 cua.listApps",
            Language::ZhTw => "應用程式識別已缺少；啟動前請重新整理 cua.listApps",
        })
    })?;
    if matches.next().is_some() {
        return Err(failed(match language {
            Language::En => "app identity ambiguous; refresh cua.listApps before launch",
            Language::ZhCn => "应用标识有歧义；启动前请刷新 cua.listApps",
            Language::ZhTw => "應用程式識別有歧義；啟動前請重新整理 cua.listApps",
        }));
    }
    if target.kind.as_deref() == Some("uwp")
        && let Some(bundle) = target
            .bundle_id
            .as_ref()
            .filter(|bundle| bundle.contains('!'))
    {
        Ok(json!({"aumid":bundle}))
    } else if let Some(path) = &target.launch_path {
        Ok(json!({"launch_path":path}))
    } else {
        Err(failed(match language {
            Language::En => "launch unavailable for this inventory entry; select its open window",
            Language::ZhCn => "此清单条目不支持启动；请选择其已打开的窗口",
            Language::ZhTw => "此清單項目不支援啟動；請選擇其已開啟的視窗",
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_shortcuts_keep_different_parameters_for_the_same_executable() {
        let first: AppInfo = serde_json::from_value(json!({"pid":0,"name":"First profile","running":false,"active":false,"bundle_id":"C:\\Apps\\Browser.exe","launch_path":"C:\\Apps\\Browser.exe --profile first"})).unwrap();
        let mut second = first.clone();
        second.launch_path = Some("C:\\Apps\\Browser.exe --profile second".into());
        assert_ne!(app_id(&first), app_id(&second));
        let id = app_id(&second);
        let expected = json!({"launch_path":second.launch_path});
        assert_eq!(
            launch_args(&id, &[first, second], Language::En).unwrap(),
            expected
        );
    }
    #[test]
    fn launch_uses_revalidated_inventory_identity_not_pid_zero_or_caller_path() {
        let app = |path: &str| AppInfo {
            pid: 0,
            name: "Installed app".into(),
            running: false,
            active: false,
            bundle_id: None,
            launch_path: Some(path.into()),
            kind: Some("desktop".into()),
            last_used: None,
        };
        let first = app(r#""C:\Apps\First.exe" --profile owned"#);
        let second = app(r#"C:\Apps\Second.exe"#);
        assert_ne!(app_id(&first), app_id(&second));
        let id = app_id(&first);
        let inventory = [first.clone(), second];
        assert_eq!(
            launch_args(&id, &inventory, Language::En).unwrap(),
            json!({"launch_path":first.launch_path})
        );
        assert!(
            matches!(launch_args("C:\\Apps\\First.exe", &inventory, Language::En), Err(ToolError::Failed(message)) if message.contains("app identity missing"))
        );
        assert!(
            matches!(launch_args(&id, &[inventory[1].clone()], Language::En), Err(ToolError::Failed(message)) if message.contains("app identity missing"))
        );
        assert!(
            matches!(launch_args(&id, &[first.clone(), first.clone()], Language::En), Err(ToolError::Failed(message)) if message.contains("app identity ambiguous"))
        );
        let mut family_only = first.clone();
        family_only.kind = Some("uwp".into());
        family_only.bundle_id = Some("Maka_fixture_family".into());
        family_only.launch_path = Some(r"shell:appsFolder\Maka_fixture_family!App".into());
        assert_eq!(
            launch_args("Maka_fixture_family", &[family_only], Language::En).unwrap(),
            json!({"launch_path":r"shell:appsFolder\Maka_fixture_family!App"})
        );
        let mut packaged = first;
        packaged.bundle_id = Some("Maka_fixture!App".into());
        packaged.kind = Some("uwp".into());
        assert_eq!(
            launch_args("Maka_fixture!App", &[packaged], Language::En).unwrap(),
            json!({"aumid":"Maka_fixture!App"})
        );
    }
}
