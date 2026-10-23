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

//! The window the workbench runs in, and the end of the client.
//!
//! Closing the window, the quit key and the quit menu item all close the
//! window; once no window is left the client detaches from the Host (its
//! tasks stop and its connection closes, within [`DETACH_LIMIT`]) and then the
//! app quits. On macOS quitting ends the process (`terminate:`), so the
//! detach runs before it, not after the event loop returns.

use maka_workbench::bindings::Bindings;
use nsh_engine::{EngineConfig, Limits};
use nsh_platform_gpui::{
    AppOptions, AppView,
    gpui::{self, App, AppContext as _, Focusable as _},
    titlebar_options,
};
use std::{cell::RefCell, rc::Rc, time::Duration};
use tokio::sync::watch;

const APP_ID: &str = "org.apache.maka.desktop";
const TITLE: &str = "Maka";
const SIZE: (i32, i32) = (1280, 820);
/// The narrowest layout the workbench folds its sidebar for.
const MIN_SIZE: (f32, f32) = (640., 480.);
/// How long quitting waits for the Host connection to close.
const DETACH_LIMIT: Duration = Duration::from_secs(5);

/// Runs the workbench in its window until the app quits.
pub fn run(
    config: EngineConfig,
    bindings: Bindings,
    runtime: tokio::runtime::Runtime,
) -> Result<(), String> {
    let detach = Detach::new(bindings, runtime);
    let mut config = Some(config);
    // Quitting waits for the detach: GPUI's default quits Linux and Windows
    // apps as their last window closes, giving the detach no more than its
    // quit handlers' 200 ms.
    let application = gpui_platform::application().with_quit_mode(gpui::QuitMode::Explicit);
    application.run(move |cx| {
        let closed = detach.clone();
        cx.on_window_closed(move |cx, _| {
            if cx.windows().is_empty() {
                closed.then_quit(cx);
            }
        })
        .detach();
        // Quitting from elsewhere (the Dock, logging out) gets the time GPUI
        // gives quit handlers.
        let quitting = detach.clone();
        cx.on_app_quit(move |cx| quitting.start(cx)).detach();
        let config = config.take().expect("the app starts once");
        if let Err(error) = open(config, cx) {
            eprintln!("maka-desktop-shell: cannot open a window: {error}");
            std::process::exit(1);
        }
        cx.activate(true);
    });
    Ok(())
}

fn open(config: EngineConfig, cx: &mut App) -> Result<(), String> {
    let bounds = gpui::Bounds::centered(
        None,
        gpui::size(gpui::px(SIZE.0 as f32), gpui::px(SIZE.1 as f32)),
        cx,
    );
    let options = gpui::WindowOptions {
        window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
        titlebar: Some(titlebar_options(TITLE)),
        app_id: Some(APP_ID.into()),
        window_min_size: Some(gpui::size(gpui::px(MIN_SIZE.0), gpui::px(MIN_SIZE.1))),
        // Linux: the window manager's decorations until the app hides the
        // title bar.
        window_decorations: Some(gpui::WindowDecorations::Server),
        // Animations keep the display's rate while another window has the keyboard.
        inactive_frame_interval: None,
        ..Default::default()
    };
    let data_dir = config.data_dir.clone();
    cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| {
            AppView::new(
                AppOptions {
                    app_id: APP_ID.into(),
                    title: TITLE.into(),
                    size: SIZE,
                    // The workbench is compiled into this trusted executable.
                    limits: Limits::standalone(),
                    data_dir,
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let app = view.read(cx);
        app.own_window(window, cx);
        // Quit and close both close the window; the last one closed detaches.
        app.on_close_request(|_, window, _| window.remove_window());
        app.load_with_config(config);
        let focus = view.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        view
    })
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// Detaches the client once: its tasks stop and its Host connection closes.
#[derive(Clone)]
struct Detach(Rc<RefCell<DetachState>>);

struct DetachState {
    /// Taken by the first [`Detach::start`].
    bindings: Option<Bindings>,
    /// Runs the bindings until the process ends.
    runtime: tokio::runtime::Runtime,
    done: watch::Sender<bool>,
}

impl Detach {
    fn new(bindings: Bindings, runtime: tokio::runtime::Runtime) -> Self {
        Self(Rc::new(RefCell::new(DetachState {
            bindings: Some(bindings),
            runtime,
            done: watch::Sender::new(false),
        })))
    }

    /// Starts detaching (once) and returns a future that ends when it has.
    fn start(&self, _: &mut App) -> impl Future<Output = ()> + use<> {
        let mut state = self.0.borrow_mut();
        if let Some(bindings) = state.bindings.take() {
            let done = state.done.clone();
            state.runtime.spawn(async move {
                match tokio::time::timeout(DETACH_LIMIT, bindings.close()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => eprintln!("maka-desktop-shell: Host cleanup: {error}"),
                    Err(_) => eprintln!("maka-desktop-shell: Host cleanup did not finish in time"),
                }
                done.send_replace(true);
            });
        }
        let mut done = state.done.subscribe();
        async move {
            let _ = done.wait_for(|done| *done).await;
        }
    }

    /// Detaches, then quits the app.
    fn then_quit(&self, cx: &mut App) {
        let detached = self.start(cx);
        cx.spawn(async move |cx| {
            detached.await;
            cx.update(|cx| cx.quit());
        })
        .detach();
    }
}
