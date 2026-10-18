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

const ONCE_PROMPT: &str = "For this compression only, preserve the Unicode decision: 中文.";

fn context_foreground(output: &[u8]) -> Option<[u8; 3]> {
    let text = String::from_utf8_lossy(output);
    let before = &text[..text.rfind("Context ")?];
    let rgb = &before[before.rfind("\x1b[38;2;")? + "\x1b[38;2;".len()..];
    let mut parts = rgb.split(|c: char| !c.is_ascii_digit());
    Some([
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ])
}

#[test]
#[ignore = "real bundled product compaction dialog, streamed summary and durable divider after restart"]
fn default_product_compaction_edits_one_shot_prompt_and_keeps_durable_divider() {
    let global_prompt = maka_agent_facts::context::default_compaction_prompt(
        maka_agent_facts::language::Language::En,
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 2, 1);
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let skill = workspace.join(".agents/skills/pty-visible-skill");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "---\nname: pty-visible-skill\ndescription: PTY visible workspace skill\n---\nPreserve the task context.").unwrap();
    let ui = directory.path().join("ui");
    let host = ui.join("host");
    let host_guard = HostRootGuard::new(&host);
    let namespaces = RootNamespaces::for_current_account().unwrap();
    drop(RootOwner::create(&ui, &namespaces).unwrap());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        seed_connection(&host, &namespaces, &base, json!({})).await;
        let config = maka_config::ConfigurationStore::for_root(Arc::new(
            RootOwner::open(&host, &namespaces).unwrap(),
        ))
        .await
        .unwrap();
        let revision = config.runtime_policy().await.unwrap().revision;
        assert!(matches!(
            config
                .set_compaction_prompt(revision, Some(global_prompt.into()))
                .await
                .unwrap(),
            maka_agent_facts::configuration::policy::RuntimePolicyMutationResult::Committed { .. }
        ));
        config.shutdown().await.unwrap();
    });
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
    tui.wait_for("fixture-model");
    click(&mut tui, "Message…");
    tui.send(b"/pty-visible");
    tui.wait_for("PTY visible workspace skill");
    evidence(&tui, "visible-skill-candidate");
    tui.send(b"\t");
    tui.wait_for("/skill:pty-visible-skill");
    assert!(
        !tui.screen
            .snapshot()
            .unwrap()
            .screen
            .contains("PTY visible workspace skill")
    );
    tui.send(b"\x01");
    tui.send(b"\x7f");
    tui.wait_for("Message…");
    paste(&mut tui, "请用 /pty-visible 保留尾巴😀");
    tui.wait_for("请用 /pty-visible 保留尾巴😀");
    // Move back across the Unicode suffix to complete only the inline token.
    tui.send(b"\x1b[D\x1b[D\x1b[D\x1b[D\x1b[D\x1b[D");
    tui.wait_for("PTY visible workspace skill");
    tui.send(b"\t");
    tui.wait_for("请用 /skill:pty-visible-skill 保留尾巴😀");
    evidence(&tui, "inline-skill-completion");
    tui.send(b"\x7f");
    tui.wait_for("请用  保留尾巴😀");
    assert!(
        !tui.screen
            .snapshot()
            .unwrap()
            .screen
            .contains("/skill:pty-visible-skill")
    );
    tui.send(b"\x1a");
    tui.wait_for("请用 /skill:pty-visible-skill 保留尾巴😀");
    tui.send(b"\x01");
    tui.send(b"\x7f");
    tui.wait_for("Message…");
    paste(
        &mut tui,
        "Remember the Unicode decision and keep deployment pending.",
    );
    tui.wait_for("Remember the Unicode decision and keep deployment pending.");
    tui.send(b"\r");
    let (main, mut socket) = next_request(&mut tui, &received);
    delta(
        &mut socket,
        "Decision recorded: 中文. Deployment is still pending.",
    );
    complete(&mut socket);
    tui.wait_for("Deployment is still pending.");
    tui.wait_for("Workspace conversation");
    select_text(&mut tui, "Deployment is still pending.");
    copy_selection(&mut tui, "Deployment is still pending.");
    // A retained reading selection intentionally pauses automatic tail following.
    click(&mut tui, "Deployment is still pending.");
    click(&mut tui, "Message…");
    tui.send(b"/");
    tui.wait_for("/compact");
    tui.wait_for("/new");
    evidence(&tui, "slash-candidates");
    tui.send(b"\x1b[B");
    tui.wait_for("› /help");
    tui.send(b"co");
    tui.wait_until(|s| s.contains("› /compact") && !s.contains("/help"));
    evidence(&tui, "slash-filtered");
    tui.send(b"\t");
    tui.wait_until(|s| s.lines().filter(|line| line.contains("/compact")).count() >= 2);
    tui.send(b"\r");
    tui.wait_for("Compact context");
    tui.wait_for("## Critical Context");
    for (columns, rows) in [(80, 24), (64, 20), (120, 40)] {
        tui.resize(columns, rows);
        tui.wait_until(|screen| {
            screen.contains("This prompt applies to this compression only.")
                && screen
                    .lines()
                    .any(|line| line.contains("Compact") && line.contains("Cancel"))
        });
        evidence(&tui, &format!("compact-{columns}x{rows}"));
    }
    evidence(&tui, "compact-confirmation");
    let idle_context_color = context_foreground(&tui.output).unwrap();
    eprintln!("compact: native confirmation displays global prompt");
    assert!(
        received.try_recv().is_err(),
        "opening confirmation must not call the model"
    );
    click(&mut tui, "## Critical Context");
    tui.send(b"\x01");
    copy_selection(&mut tui, global_prompt);
    paste(&mut tui, ONCE_PROMPT);
    tui.wait_for("For this compression only");
    evidence(&tui, "compact-edited");
    // The native dialog title also contains Compact; the button includes its
    // neighboring Cancel control, so locate its row explicitly.
    let screen = tui.screen.snapshot().unwrap().screen;
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Compact") && line.contains("Cancel"))
        .unwrap();
    let column = line[..line.find("Compact").unwrap()].width() + 2;
    tui.send(
        format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            column,
            row + 1,
            column,
            row + 1
        )
        .as_bytes(),
    );
    let (summary, mut socket) = next_request(&mut tui, &received);
    assert_eq!(
        summary["messages"].as_array().unwrap().last().unwrap()["content"],
        ONCE_PROMPT
    );
    assert!(!summary["messages"].to_string().contains(global_prompt));
    assert!(
        summary["messages"]
            .to_string()
            .contains("Deployment is still pending")
    );
    assert_eq!(
        summary["tools"], main["tools"],
        "summary retains the captured Main tool surface"
    );
    // Hold the real HTTP summary stream open while inspecting the native footer.
    tui.wait_until(|s| !s.contains("Compact context") && s.contains("Context "));
    let deadline = Instant::now() + Duration::from_secs(10);
    while context_foreground(&tui.output) == Some(idle_context_color) {
        tui.read();
        assert!(
            Instant::now() < deadline,
            "Context foreground did not change while the summary stream was open"
        );
    }
    assert!(context_foreground(&tui.output).is_some());
    evidence(&tui, "compact-running");
    eprintln!("compact: exact one-shot instruction and active footer tint verified");
    assert!(
        !tui.screen
            .snapshot()
            .unwrap()
            .screen
            .contains("Context compressed (")
    );
    delta(
        &mut socket,
        "## Current task\nDeployment is pending.\n## Decisions\nPreserve 中文.\n## Next steps\nFinish deployment.",
    );
    complete(&mut socket);
    tui.wait_for("Context compressed (");
    tui.wait_until(|s| {
        s.lines()
            .any(|line| line.contains("Context compressed (") && line.contains("s)"))
    });
    evidence(&tui, "compact-completed");
    eprintln!("compact: successful durable divider displayed");
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();

    let mut restored = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    restored.wait_until_with_timeout(
        |s| s.contains("Workspace conversation"),
        Duration::from_secs(60),
    );
    restored.wait_for("Context compressed (");
    evidence(&restored, "compact-restarted");
    eprintln!("compact: divider retained after Host and UI restart");
    restored.send(b"\x11");
    restored.finish();
    host_guard.stop();
    runtime.block_on(async {
        let config = maka_config::ConfigurationStore::for_root(Arc::new(
            RootOwner::open(&host, &namespaces).unwrap(),
        ))
        .await
        .unwrap();
        assert_eq!(
            config
                .runtime_policy()
                .await
                .unwrap()
                .policy
                .compaction_prompt
                .as_deref(),
            Some(global_prompt),
            "the dialog override must not modify global preferences"
        );
        config.shutdown().await.unwrap();
    });
}
