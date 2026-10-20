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

use super::*;

fn capture(tui: &Pty, name: &str) {
    if let Some(directory) = std::env::var_os("MAKA_TRACE_CAPTURE") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(format!("{name}.ansi")), &tui.output).unwrap();
        std::fs::write(
            directory.join(format!("{name}.txt")),
            tui.screen.preview(usize::MAX).unwrap().screen,
        )
        .unwrap();
    }
}

#[test]
#[ignore = "real product, model HTTP, native Shell and terminal trace interaction"]
fn default_product_trace_preserves_parents_titles_and_inline_evidence() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 2, 1);
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let ui = directory.path().join("ui");
    let host = ui.join("host");
    let host_guard = HostRootGuard::new(&host);
    let namespaces = RootNamespaces::for_current_account().unwrap();
    drop(RootOwner::create(&ui, &namespaces).unwrap());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(seed_connection(
        &host,
        &namespaces,
        &base,
        json!({"fixture-model":{"codeMode":true}}),
    ));
    let mut tui = Pty::spawn(&[
        "application",
        "launch",
        "--root",
        ui.to_str().unwrap(),
        "--workspace",
        workspace.to_str().unwrap(),
        "--trust-workspace",
        "--locale",
        "en",
    ]);
    tui.wait_until_with_timeout(|s| s.contains("Settings"), Duration::from_secs(60));
    tui.send(b"\x0b");
    tui.wait_for("Close commands");
    tui.send(b"New conversation");
    tui.wait_for("› New conversation");
    tui.send(b"\r");
    tui.wait_for("Message…");
    click(&mut tui, "Message…");
    paste(&mut tui, "检查工作区，并展示验证结果。");
    tui.send(b"\r");
    let (_, mut socket) = next_request(&mut tui, &received);
    code_mode::tool_response(
        &mut socket,
        "exec",
        json!({"code":
            "text(await tools.Shell({title:'检查工作区',command:'printf trace-visible > trace-visible.txt; cat trace-visible.txt',login:false,timeout_ms:5000})); text(await tools.Shell({title:'验证取消流程',command:'printf known-failure >&2; exit 7',login:false,timeout_ms:5000}));"
        }),
    );
    let (next, mut socket) = next_request(&mut tui, &received);
    assert!(next["messages"].to_string().contains("trace-visible"));
    assert_eq!(
        std::fs::read_to_string(workspace.join("trace-visible.txt")).unwrap(),
        "trace-visible"
    );
    tui.wait_for("Exit code: 7");
    tui.wait_until(|screen| {
        let rows: Vec<_> = screen.lines().collect();
        let failure = rows.iter().position(|line| line.contains("Exit code: 7"));
        let clock = rows.iter().position(|line| {
            let label = line.rsplit('│').next().unwrap().trim();
            label.contains('s')
                && label
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, ' ' | 's' | 'm' | 'h'))
        });
        matches!((failure, clock), (Some(failure), Some(clock)) if clock > failure)
    });
    capture(&tui, "running");
    delta(
        &mut socket,
        "命令运行完毕。第二条命令返回退出码 7，仍需处理。",
    );
    complete(&mut socket);
    tui.wait_for("命令运行完毕");
    tui.wait_until(|screen| {
        screen.contains("检查工作区")
            && screen.contains("验证取消流程")
            && screen.contains("Run tools")
    });
    tui.wait_for("Exit code: 7");
    tui.wait_until(|screen| !screen.contains("Finished"));
    let screen = tui.screen.preview(usize::MAX).unwrap().screen;
    let rows: Vec<_> = screen.lines().collect();
    let first = rows
        .iter()
        .position(|line| line.contains("› 检查工作区"))
        .unwrap();
    let parent = rows
        .iter()
        .find(|line| line.contains("› Run tools"))
        .unwrap();
    assert!(
        parent.trim_end().ends_with('⋯'),
        "the tool menu must align with its heading, below the response gap: {screen}"
    );
    assert_eq!(
        rows[first][..rows[first].find('·').unwrap()].width(),
        parent[..parent.find('·').unwrap()].width() + 2,
        "child indentation must come from its canonical ancestry: {screen}"
    );
    assert!(
        rows[first + 1].contains("验证取消流程"),
        "successful titled steps use one row: {screen}"
    );
    assert!(
        !screen.contains("printf trace-visible"),
        "collapsed input must remain behind its disclosure: {screen}"
    );
    capture(&tui, "collapsed");
    click(&mut tui, "› Run tools");
    tui.wait_for("Tool arguments");
    tui.wait_for("text(await tools.Shell");
    tui.wait_for("Tool result");
    tui.wait_until(|screen| !screen.contains("Loading conversation…"));
    capture(&tui, "orchestration");
    tui.resize(80, 20);
    tui.wait_until(|screen| screen.lines().count() == 20);
    for _ in 0..12 {
        tui.send(b"\x1b[<65;70;10M");
    }
    tui.wait_for("↑ Hide details");
    assert!(
        !tui.screen
            .preview(usize::MAX)
            .unwrap()
            .screen
            .contains("⌄ Run tools"),
        "the bottom control must work with the heading outside the viewport"
    );
    capture(&tui, "bottom");
    click(&mut tui, "↑ Hide details");
    tui.wait_for("› Run tools");
    tui.wait_until(|screen| !screen.contains("text(await tools.Shell"));
    capture(&tui, "recollapsed");
    tui.resize(120, 40);
    tui.wait_for("› 检查工作区");
    click(&mut tui, "› 检查工作区");
    tui.wait_for("trace-visible.txt");
    tui.wait_for("Exit code: 0");
    tui.wait_until(|screen| {
        screen
            .lines()
            .any(|line| line.trim_end().ends_with("trace-visible"))
    });
    tui.wait_until(|screen| !screen.contains("Loading conversation…"));
    assert!(
        !tui.screen
            .preview(usize::MAX)
            .unwrap()
            .screen
            .contains("Recorded process status:"),
        "the process outcome needs no redundant prefix"
    );
    capture(&tui, "expanded");
    click(&mut tui, "⌄ 检查工作区");
    tui.wait_until(|screen| !screen.contains("printf trace-visible"));
    tui.send(b"\r");
    tui.wait_for("⌄ 检查工作区");
    tui.wait_for("trace-visible.txt");
    tui.send(b"\r");
    tui.wait_for("› 检查工作区");
    tui.wait_until(|screen| !screen.contains("printf trace-visible"));
    tui.resize(80, 30);
    tui.wait_until(|screen| screen.contains("› 检查工作区") && screen.contains("验证取消流程"));
    capture(&tui, "narrow");
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();
}
