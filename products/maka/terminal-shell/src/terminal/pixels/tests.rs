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

use gpui::{
    AppContext, Application, Bounds, PlatformInput, WindowBounds, WindowOptions, point, px, size,
};
use nsh_engine::{App, Bundle, EngineConfig};
use nsh_gpui_terminal::{SubmittedFrame, TerminalPlatform};
use nsh_platform_gpui::{AppOptions, AppView};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn native_editor_focus_and_text_do_not_wait_for_another_pixel_frame() {
    const APP: &str = r#"
        import { mount, signal } from 'nsh';
        import { element, prop, bindProp, append } from 'nsh:rt';
        const value = signal('');
        mount(() => {
            const root = element('view');
            prop(root, 'style', {flex: 1, padding: 16, backgroundColor: '#111111'});
            const input = element('input');
            prop(input, 'style', {width: 240, height: 48, backgroundColor: '#204060', color: '#ffffff'});
            bindProp(input, 'value', value);
            prop(input, 'onChange', text => value.set(text));
            append(root, input);
            const body = element('view');
            prop(body, 'style', {width: 240, height: 64, backgroundColor: '#406020'});
            append(root, body);
            return root;
        });
    "#;
    let frames = Rc::new(RefCell::new(Vec::<SubmittedFrame>::new()));
    let received = frames.clone();
    let count = Rc::new(Cell::new(0usize));
    let submitted = count.clone();
    let platform = TerminalPlatform::new(
        Box::new(gpui_wgpu::WgpuHeadlessRenderer::new().unwrap()),
        Box::new(move |frame| {
            submitted.set(submitted.get() + 1);
            received.borrow_mut().push(frame);
        }),
        Arc::new(|| {}),
    )
    .unwrap();
    let errors = Rc::new(RefCell::new(None));
    let failed = errors.clone();
    let window = Rc::new(RefCell::new(None));
    let opened = window.clone();
    let application = Application::with_platform(platform.clone()).run_embedded(move |cx| {
        *opened.borrow_mut() = Some(
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.0), px(0.0)),
                        size(px(320.0), px(160.0)),
                    ))),
                    inactive_frame_interval: None,
                    ..Default::default()
                },
                move |window, cx| {
                    cx.new(|cx| {
                        let view = AppView::new(AppOptions::default(), window, cx);
                        view.on_error(move |message| {
                            *failed.borrow_mut() = Some(message.to_owned())
                        });
                        view.load_with_config(EngineConfig {
                            ui_sets: vec![nsh_base::registry()],
                            ..EngineConfig::new(App::Js(Bundle::source(APP.into())))
                        });
                        view
                    })
                },
            )
            .unwrap(),
        );
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        platform.pump();
        assert!(
            errors.borrow().is_none(),
            "real Input failed to load: {:?}",
            errors.borrow()
        );
        let ready = frames.borrow_mut().drain(..).any(|frame| {
            let frame = frame.complete().unwrap();
            // The fixture's actual input fill proves that native mount and its
            // initial paint completed; a blank startup frame is insufficient.
            &frame.pixels()[(24 * 320 + 24) * 4..][..4] == [32, 64, 96, 255]
        });
        if ready {
            break;
        }
        assert!(Instant::now() < deadline, "real Input did not load");
    }
    let initial_count = count.get();
    let click = |y| {
        let position = point(px(32.0), px(y));
        platform
            .dispatch_input(PlatformInput::MouseDown(gpui::MouseDownEvent {
                position,
                button: gpui::MouseButton::Left,
                click_count: 1,
                ..Default::default()
            }))
            .unwrap();
        platform
            .dispatch_input(PlatformInput::MouseUp(gpui::MouseUpEvent {
                position,
                button: gpui::MouseButton::Left,
                click_count: 1,
                ..Default::default()
            }))
            .unwrap();
    };
    let stroke = |key: &str, text: Option<&str>, control| {
        let keystroke = gpui::Keystroke {
            key: key.into(),
            key_char: text.map(str::to_owned),
            modifiers: gpui::Modifiers {
                control,
                ..Default::default()
            },
        };
        platform
            .dispatch_input(PlatformInput::KeyDown(gpui::KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            }))
            .unwrap();
        platform
            .dispatch_input(PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke }))
            .unwrap();
    };
    let copied = || {
        stroke("a", None, true);
        stroke("c", None, true);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            platform.pump_with_draw(false);
            assert_eq!(
                count.get(),
                initial_count,
                "text/focus must not force pixel output"
            );
            if let Some(text) = platform.take_clipboard_write().and_then(|item| item.text()) {
                return text;
            }
            assert!(
                errors.borrow().is_none(),
                "native input error: {:?}",
                errors.borrow()
            );
            assert!(Instant::now() < deadline, "native copy did not complete");
        }
    };
    // Initial paint had no focused editor. Registration must already exist so
    // the physical click can focus it and accept text before another paint.
    click(32.0);
    stroke("a", Some("a"), false);
    platform.paste("你好😀").unwrap();
    assert_eq!(copied(), "a你好😀");

    // The same registered handler must consult live focus: an ordinary body
    // click must stop paste immediately, without a paint to unregister it.
    click(96.0);
    platform.paste("must not enter the old editor").unwrap();
    click(32.0);
    assert_eq!(copied(), "a你好😀");
    assert_eq!(count.get(), initial_count);
    application.update(|cx| {
        window
            .borrow()
            .as_ref()
            .unwrap()
            .update(cx, |view, _, _| view.stop())
            .unwrap();
    });
    drop(application);
    platform.shutdown();
}
