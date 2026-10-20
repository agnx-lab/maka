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
use base64::{Engine, engine::general_purpose::STANDARD};
use std::path::Path;

// Git authors fixtures and supplies an independent status oracle. Agent values
// come from the public ApplicationClient through the real Session scope.
fn git(workspace: &Path, arguments: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(arguments)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository(workspace: &Path) {
    git(workspace, &["init", "--quiet", "--initial-branch=main"]);
    git(
        workspace,
        &["config", "user.email", "git-fixture@example.invalid"],
    );
    git(workspace, &["config", "user.name", "Git fixture"]);
    git(workspace, &["config", "core.autocrlf", "false"]);
    // The workspace identity marker is fixture infrastructure, not a user edit.
    std::fs::write(
        workspace.join(".git/info/exclude"),
        format!("/{}\n", maka_fs_tools::workspace::MARKER_FILE),
    )
    .unwrap();
}

fn path(name: &str) -> Value {
    json!({"bytesBase64": STANDARD.encode(name.as_bytes())})
}

fn changed_lines(result: &Value) -> Vec<&str> {
    assert_eq!(result["kind"], "text", "{result}");
    result["patch"]
        .as_str()
        .unwrap()
        .lines()
        .filter(|line| {
            (line.starts_with('+') && !line.starts_with("+++"))
                || (line.starts_with('-') && !line.starts_with("---"))
        })
        .collect()
}

#[tokio::test]
async fn selected_directory_create_excludes_workspace_marker_from_agent_and_git_status() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let f = Fixture::new("http://127.0.0.1:1/v1", None).await;
        // Unlike repository(), this leaves marker exclusion to actual create.
        git(&f.workspace, &["init", "--quiet", "--initial-branch=main"]);
        let user_file = "literal-user-untracked.txt";
        std::fs::write(f.workspace.join(user_file), "user file\n").unwrap();
        let input = f.create_input("Selected Git directory").await;
        let created = f.mutation(2, "create", input, Uuid::new_v4()).await;
        let status = match created
            .as_ref()
            .ok()
            .and_then(|value| value["session"]["id"].as_str())
        {
            Some(session) => Some(f.query("gitStatus", json!({"sessionId":session})).await),
            None => None,
        };
        let marker = maka_fs_tools::workspace::MARKER_FILE;
        let marker_exists = f.workspace.join(marker).is_file();
        let oracle = std::process::Command::new("git")
            .arg("-C")
            .arg(&f.workspace)
            .args(["status", "--porcelain", "--untracked-files=all"])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output();
        // Complete real Host cleanup before the expected marker-leak RED.
        let app_shutdown = f.app.shutdown().await;
        let config_shutdown = f.config.shutdown().await;
        app_shutdown.unwrap();
        config_shutdown.unwrap();
        let created = created.expect("selected-directory create must succeed");
        assert_eq!(created["kind"], "created", "{created}");
        assert!(
            marker_exists,
            "create must preserve intrinsic workspace identity"
        );
        let status = status
            .expect("created Session ID")
            .expect("Agent gitStatus");
        let entries = status["entries"].as_array().unwrap();
        let oracle = oracle.expect("real Git status oracle");
        assert!(
            oracle.status.success(),
            "{}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        let porcelain = String::from_utf8(oracle.stdout).unwrap();
        assert!(
            entries
                .iter()
                .any(|entry| entry["area"] == "untracked" && entry["path"] == path(user_file)),
            "{status}"
        );
        assert!(
            porcelain
                .lines()
                .any(|line| line == format!("?? {user_file}")),
            "{porcelain}"
        );
        let agent_has_marker = entries.iter().any(|entry| entry["path"] == path(marker));
        let git_has_marker = porcelain.lines().any(|line| line == format!("?? {marker}"));
        assert!(
            !agent_has_marker && !git_has_marker,
            "workspace marker leaked: Agent={status}, Git={porcelain}"
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn git_status_and_diff_read_current_workspace_without_mutating_index() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let f = Fixture::new("http://127.0.0.1:1/v1", None).await;
        let session = f.create("Git comparison").await;
        repository(&f.workspace);
        std::fs::write(f.workspace.join("tracked.txt"), "base\n").unwrap();
        git(&f.workspace, &["add", "tracked.txt"]);
        git(&f.workspace, &["commit", "--quiet", "-m", "base"]);
        std::fs::write(f.workspace.join("tracked.txt"), "staged\n").unwrap();
        git(&f.workspace, &["add", "tracked.txt"]);
        std::fs::write(f.workspace.join("tracked.txt"), "worktree\n").unwrap();
        std::fs::write(f.workspace.join("new.txt"), "new file\n").unwrap();
        let index = f.workspace.join(".git/index");
        let index_bytes = std::fs::read(&index).unwrap();
        let index_modified = std::fs::metadata(&index).unwrap().modified().unwrap();
        let observed = f.query("gitStatus", json!({"sessionId":session})).await;
        // A preimplementation missing-method RED still closes the real runtime.
        if observed.is_err() {
            f.app.shutdown().await.unwrap();
            f.config.shutdown().await.unwrap();
        }
        let status = observed.expect("Agent gitStatus must read the selected Session repository");
        let mut entries = status["entries"].as_array().unwrap().clone();
        entries.sort_by_key(|entry| {
            (
                entry["area"].as_str().unwrap().to_owned(),
                entry["path"].to_string(),
            )
        });
        let mut status = status;
        status["entries"] = json!(entries);
        let head = std::fs::read_to_string(f.workspace.join(".git/refs/heads/main")).unwrap();
        assert_eq!(
            status,
            json!({"head":head.trim(),"entries":[
                {"area":"staged","path":{"bytesBase64":"dHJhY2tlZC50eHQ="},"kind":"modified"},
                {"area":"untracked","path":{"bytesBase64":"bmV3LnR4dA=="},"kind":"added"},
                {"area":"worktree","path":{"bytesBase64":"dHJhY2tlZC50eHQ="},"kind":"modified"},
            ]})
        );
        for (area, name, expected) in [
            ("staged", "tracked.txt", vec!["-base", "+staged"]),
            ("worktree", "tracked.txt", vec!["-staged", "+worktree"]),
            ("untracked", "new.txt", vec!["+new file"]),
        ] {
            let diff = f
                .query(
                    "gitDiff",
                    json!({"sessionId":session,"area":area,"path":path(name)}),
                )
                .await
                .unwrap();
            assert_eq!(changed_lines(&diff), expected, "{area}");
        }
        std::fs::write(f.workspace.join("tracked.txt"), "later edit\n").unwrap();
        let fresh = f
            .query(
                "gitDiff",
                json!({"sessionId":session,"area":"worktree","path":path("tracked.txt")}),
            )
            .await
            .unwrap();
        assert_eq!(changed_lines(&fresh), vec!["-staged", "+later edit"]);
        std::fs::remove_file(f.workspace.join("new.txt")).unwrap();
        let fresh_status = f
            .query("gitStatus", json!({"sessionId":session}))
            .await
            .unwrap();
        let mut areas: Vec<_> = fresh_status["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["area"].as_str().unwrap())
            .collect();
        areas.sort();
        assert_eq!(areas, ["staged", "worktree"]);
        assert_eq!(std::fs::read(&index).unwrap(), index_bytes);
        assert_eq!(
            std::fs::metadata(&index).unwrap().modified().unwrap(),
            index_modified
        );
        assert!(!f.workspace.join(".git/index.lock").exists());
        assert_eq!(
            std::fs::read_to_string(f.workspace.join("tracked.txt")).unwrap(),
            "later edit\n"
        );
        f.app.shutdown().await.unwrap();
        f.config.shutdown().await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn git_status_returns_every_literal_entry_through_public_host_without_mutating_repository() {
    let f = Fixture::new("http://127.0.0.1:1/v1", None).await;
    let session = f.create("Complete Git status").await;
    repository(&f.workspace);
    let staged = "staged-change.txt";
    let worktree = "worktree-change.txt";
    std::fs::write(f.workspace.join(staged), "staged base\n").unwrap();
    std::fs::write(f.workspace.join(worktree), "worktree base\n").unwrap();
    git(&f.workspace, &["add", staged, worktree]);
    git(&f.workspace, &["commit", "--quiet", "-m", "status base"]);
    std::fs::write(f.workspace.join(staged), "staged current\n").unwrap();
    git(&f.workspace, &["add", staged]);
    std::fs::write(f.workspace.join(worktree), "worktree current\n").unwrap();
    let names = (0..1_800)
        .map(|number| format!("literal-{number:04}-{}-中文.txt", "x".repeat(190)))
        .collect::<Vec<_>>();
    for name in &names {
        std::fs::write(f.workspace.join(name), "literal untracked contents\n").unwrap();
    }
    let head = std::fs::read_to_string(f.workspace.join(".git/refs/heads/main")).unwrap();
    let mut expected_entries = vec![
        json!({"area":"staged","path":path(staged),"kind":"modified"}),
        json!({"area":"worktree","path":path(worktree),"kind":"modified"}),
    ];
    expected_entries.extend(
        names
            .iter()
            .map(|name| json!({"area":"untracked","path":path(name),"kind":"added"})),
    );
    let expected = json!({"head":head.trim(),"entries":expected_entries});
    let preserved = [
        ".git/HEAD",
        ".git/refs/heads/main",
        ".git/index",
        staged,
        worktree,
    ]
    .into_iter()
    .chain(names.iter().map(String::as_str))
    .map(|name| {
        let file = f.workspace.join(name);
        let bytes = std::fs::read(&file).unwrap();
        let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
        (file, bytes, modified)
    })
    .collect::<Vec<_>>();
    let observed = f.query("gitStatus", json!({"sessionId":session})).await;
    // Retain the query outcome and close both real Host owners before any RED assertion.
    let app_shutdown = f.app.shutdown().await;
    let config_shutdown = f.config.shutdown().await;
    app_shutdown.unwrap();
    config_shutdown.unwrap();
    for (file, bytes, modified) in preserved {
        assert_eq!(
            std::fs::read(&file).unwrap(),
            bytes,
            "query changed {}",
            file.display()
        );
        assert_eq!(
            std::fs::metadata(&file).unwrap().modified().unwrap(),
            modified,
            "query touched {}",
            file.display()
        );
    }
    assert!(!f.workspace.join(".git/index.lock").exists());
    assert!(serde_json::to_vec(&expected).unwrap().len() > 512 * 1024);
    let status = observed.expect("public Host gitStatus must deliver every repository entry");
    assert_eq!(status["head"], head.trim());
    assert_eq!(
        status["entries"].as_array().unwrap().len(),
        1_802,
        "public Host gitStatus must not return a bounded prefix"
    );
    for (number, (actual, expected)) in status["entries"]
        .as_array()
        .unwrap()
        .iter()
        .zip(expected["entries"].as_array().unwrap())
        .enumerate()
    {
        assert_eq!(
            actual, expected,
            "complete public Host status entry {number}"
        );
    }
    assert!(
        status == expected,
        "complete public Host gitStatus must have exactly the expected head/entries shape"
    );
}

#[tokio::test]
async fn git_diff_returns_complete_large_utf8_comparisons_without_mutating_repository() {
    let f = Fixture::new("http://127.0.0.1:1/v1", None).await;
    let session = f.create("Large Git comparisons").await;
    repository(&f.workspace);
    let tracked = "large-tracked.txt";
    let untracked = "large-new.txt";
    let base: String = (0..65_537)
        .map(|line| format!("row-{line:08} 中文😀 payload\n"))
        .collect();
    let staged = base.replace("row-00032768 中文😀 payload", "staged 中文😀 change");
    let working = staged.replace("row-00049152 中文😀 payload", "worktree 中文😀 change");
    let added: String = (0..25_000)
        .map(|line| format!("new-row-{line:08} 中文😀\n"))
        .collect();
    std::fs::write(f.workspace.join(tracked), &base).unwrap();
    git(&f.workspace, &["add", tracked]);
    git(&f.workspace, &["commit", "--quiet", "-m", "large base"]);
    std::fs::write(f.workspace.join(tracked), &staged).unwrap();
    git(&f.workspace, &["add", tracked]);
    std::fs::write(f.workspace.join(tracked), &working).unwrap();
    std::fs::write(f.workspace.join(untracked), &added).unwrap();
    let preserved = [
        ".git/HEAD",
        ".git/refs/heads/main",
        ".git/index",
        tracked,
        untracked,
    ]
    .into_iter()
    .map(|name| {
        let file = f.workspace.join(name);
        let bytes = std::fs::read(&file).unwrap();
        let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
        (file, bytes, modified)
    })
    .collect::<Vec<_>>();
    let mut results = Vec::new();
    for (area, name) in [
        ("staged", tracked),
        ("worktree", tracked),
        ("untracked", untracked),
    ] {
        results.push(
            f.query(
                "gitDiff",
                json!({"sessionId":session,"area":area,"path":path(name)}),
            )
            .await,
        );
    }
    // Await both real runtime owners before asserting the expected baseline refusals.
    let app_shutdown = f.app.shutdown().await;
    let config_shutdown = f.config.shutdown().await;
    app_shutdown.unwrap();
    config_shutdown.unwrap();
    for (file, bytes, modified) in preserved {
        assert_eq!(
            std::fs::read(&file).unwrap(),
            bytes,
            "query changed {}",
            file.display()
        );
        assert_eq!(
            std::fs::metadata(&file).unwrap().modified().unwrap(),
            modified,
            "query touched {}",
            file.display()
        );
    }
    assert!(!f.workspace.join(".git/index.lock").exists());
    assert!(base.len() > 1024 * 1024 && staged.len() > 1024 * 1024 && working.len() > 1024 * 1024);
    assert!(added.len() > 512 * 1024 && added.len() < 1024 * 1024);
    let results = results
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("public Host gitDiff queries must return full values");
    assert_eq!(
        results
            .iter()
            .map(|result| result["kind"].clone())
            .collect::<Vec<_>>(),
        vec![json!("text"); 3],
        "large staged, worktree and untracked comparisons must all be text"
    );
    assert_eq!(
        changed_lines(&results[0]),
        ["-row-00032768 中文😀 payload", "+staged 中文😀 change"]
    );
    assert_eq!(
        changed_lines(&results[1]),
        ["-row-00049152 中文😀 payload", "+worktree 中文😀 change"]
    );
    assert!(results[2]["patch"].as_str().unwrap().len() > 512 * 1024);
    assert!(serde_json::to_vec(&results[2]).unwrap().len() > 512 * 1024);
    for (result, name, before, expected) in [
        (&results[0], tracked, Some(base.as_str()), staged.as_str()),
        (
            &results[1],
            tracked,
            Some(staged.as_str()),
            working.as_str(),
        ),
        (&results[2], untracked, None, added.as_str()),
    ] {
        // Real Git independently checks every hunk, header path and original line;
        // exact destination bytes detect truncation anywhere in the returned patch.
        let oracle = tempfile::tempdir().unwrap();
        if let Some(before) = before {
            std::fs::write(oracle.path().join(name), before).unwrap();
        }
        let patch = oracle.path().join("input.patch");
        std::fs::write(&patch, result["patch"].as_str().unwrap()).unwrap();
        git(oracle.path(), &["apply", "--", patch.to_str().unwrap()]);
        assert_eq!(
            std::fs::read(oracle.path().join(name)).unwrap(),
            expected.as_bytes(),
            "complete patch for {name}"
        );
    }
}

#[tokio::test]
async fn git_queries_keep_unborn_binary_results_within_session_and_method_grants() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let f = Fixture::new("http://127.0.0.1:1/v1", None).await;
        let session = f.create("Unborn Git").await;
        assert_eq!(f.query("gitStatus", json!({"sessionId":session})).await.unwrap(), Value::Null);
        assert_eq!(f.query("gitDiff", json!({"sessionId":session,"area":"untracked","path":path("binary.dat")})).await.unwrap(), Value::Null);
        repository(&f.workspace);
        std::fs::write(f.workspace.join("binary.dat"), b"before\0after").unwrap();
        git(&f.workspace, &["add", "binary.dat"]);
        let expected = json!({"head":null,"entries":[{"area":"staged","path":{"bytesBase64":"YmluYXJ5LmRhdA=="},"kind":"added"}]});
        assert_eq!(f.query("gitStatus", json!({"sessionId":session})).await.unwrap(), expected);
        let input = json!({"sessionId":session,"area":"staged","path":path("binary.dat")});
        assert_eq!(f.query("gitDiff", input.clone()).await.unwrap(), json!({"kind":"binary"}));
        for (method, input) in [("gitStatus", json!({"sessionId":session})), ("gitDiff", input)] {
            let denied = f.app.client().start(f.binding(2).await, method.into(), input.clone(),
                scope(&f.issuer, false), Attachments::new().into()).await;
            assert_eq!(denied.err().unwrap(), "denied", "{method} must retain its actual Host method grant");
            let mut foreign = input;
            foreign["sessionId"] = json!("foreign-session");
            assert_eq!(f.query(method, foreign).await.unwrap_err(), "agent_session_unavailable", "{method}");
        }
        assert_eq!(f.query("gitStatus", json!({"sessionId":session})).await.unwrap(), expected);
        f.app.shutdown().await.unwrap();
        f.config.shutdown().await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn git_queries_reject_selected_subdirectories_and_replaced_workspace_identity() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let f = Fixture::new("http://127.0.0.1:1/v1", None).await;
        let root_session = f.create("Repository root").await;
        repository(&f.workspace);
        std::fs::create_dir(f.workspace.join("selected")).unwrap();
        std::fs::write(f.workspace.join("selected/inside.txt"), "inside base\n").unwrap();
        std::fs::write(f.workspace.join("sibling.txt"), "sibling base\n").unwrap();
        git(&f.workspace, &["add", "selected/inside.txt", "sibling.txt"]);
        git(&f.workspace, &["commit", "--quiet", "-m", "base"]);
        std::fs::write(f.workspace.join("selected/inside.txt"), "inside current\n").unwrap();
        std::fs::write(f.workspace.join("sibling.txt"), "private sibling current\n").unwrap();
        let root_diff = f
            .query(
                "gitDiff",
                json!({"sessionId":root_session,"area":"worktree","path":path("sibling.txt")}),
            )
            .await
            .unwrap();
        assert_eq!(
            changed_lines(&root_diff),
            ["-sibling base", "+private sibling current"]
        );
        let roots = f
            .query("directories", json!({"kind":"roots"}))
            .await
            .unwrap();
        let children = f
            .query(
                "directories",
                json!({"kind":"children","selection":roots["roots"][0]["selection"],"after":null}),
            )
            .await
            .unwrap();
        let selected = children["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| {
                Path::new(entry["path"].as_str().unwrap()).file_name()
                    == Some(std::ffi::OsStr::new("selected"))
            })
            .unwrap();
        let child = f
            .mutation(
                2,
                "create",
                json!({"title":"Selected child","labels":[],
            "workspace":{"kind":"directory","selection":selected["selection"]}}),
                Uuid::new_v4(),
            )
            .await
            .unwrap();
        assert_eq!(child["kind"], "created");
        let child_session = &child["session"]["id"];
        for (method, input) in [
            ("gitStatus", json!({"sessionId":child_session})),
            (
                "gitDiff",
                json!({"sessionId":child_session,"area":"worktree","path":path("inside.txt")}),
            ),
        ] {
            let error = f.query(method, input).await.unwrap_err();
            assert_eq!(error, "agent_git_unsupported", "{method}");
            assert!(!error.contains("private sibling current"));
        }
        let original_marker =
            std::fs::read(f.workspace.join(maka_fs_tools::workspace::MARKER_FILE)).unwrap();
        let moved = f.workspace.with_file_name("original-workspace");
        std::fs::rename(&f.workspace, &moved).unwrap();
        std::fs::create_dir(&f.workspace).unwrap();
        std::fs::write(
            f.workspace.join(maka_fs_tools::workspace::MARKER_FILE),
            original_marker,
        )
        .unwrap();
        repository(&f.workspace);
        std::fs::write(f.workspace.join("replacement.txt"), "must not retarget\n").unwrap();
        for (method, input) in [
            ("gitStatus", json!({"sessionId":root_session})),
            (
                "gitDiff",
                json!({"sessionId":root_session,"area":"untracked","path":path("replacement.txt")}),
            ),
        ] {
            assert_eq!(
                f.query(method, input).await.unwrap_err(),
                "agent_workspace_unavailable",
                "{method} must validate the original workspace identity"
            );
        }
        assert_eq!(
            std::fs::read_to_string(moved.join("sibling.txt")).unwrap(),
            "private sibling current\n"
        );
        f.app.shutdown().await.unwrap();
        f.config.shutdown().await.unwrap();
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn git_diff_reads_link_text_without_following_the_target_outside_its_workspace() {
    tokio::time::timeout(Duration::from_secs(45), async {
        use std::os::unix::fs::symlink;
        let f = Fixture::new("http://127.0.0.1:1/v1", None).await;
        let session = f.create("Link text").await;
        repository(&f.workspace);
        std::fs::write(f.root.join("private-secret"), "private target payload\n").unwrap();
        symlink("../old-target", f.workspace.join("link.txt")).unwrap();
        git(&f.workspace, &["add", "link.txt"]);
        git(&f.workspace, &["commit", "--quiet", "-m", "base link"]);
        std::fs::remove_file(f.workspace.join("link.txt")).unwrap();
        symlink("../root/private-secret", f.workspace.join("link.txt")).unwrap();
        let diff = f
            .query(
                "gitDiff",
                json!({"sessionId":session,"area":"worktree","path":path("link.txt")}),
            )
            .await
            .unwrap();
        assert_eq!(
            changed_lines(&diff),
            ["-../old-target", "+../root/private-secret"]
        );
        assert!(
            !diff["patch"]
                .as_str()
                .unwrap()
                .contains("private target payload")
        );
        assert_eq!(
            std::fs::read_to_string(f.root.join("private-secret")).unwrap(),
            "private target payload\n"
        );
        f.app.shutdown().await.unwrap();
        f.config.shutdown().await.unwrap();
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn git_path_bytes_roundtrip_through_public_status_and_diff() {
    tokio::time::timeout(Duration::from_secs(45), async {
        use std::os::unix::ffi::OsStringExt;
        let f = Fixture::new("http://127.0.0.1:1/v1", None).await;
        let session = f.create("Opaque Git paths").await;
        repository(&f.workspace);
        // APFS rejects invalid UTF-8 filenames; still exercise exact legal
        // bytes that cannot safely be treated as shell syntax or quoted text.
        #[cfg(target_os = "macos")]
        let (bytes, expected) = (
            "odd-中文\n\t\\.txt".as_bytes().to_vec(),
            "b2RkLeS4reaWhwoJXC50eHQ=",
        );
        #[cfg(not(target_os = "macos"))]
        let (bytes, expected) = (b"odd-\xff\n\t.txt".to_vec(), "b2RkLf8KCS50eHQ=");
        let name = std::ffi::OsString::from_vec(bytes);
        std::fs::write(f.workspace.join(name), "exact file contents\n").unwrap();
        let status = f
            .query("gitStatus", json!({"sessionId":session}))
            .await
            .unwrap();
        assert_eq!(
            status["entries"],
            json!([{"area":"untracked","path":{"bytesBase64":expected},"kind":"added"}])
        );
        let diff = f
            .query(
                "gitDiff",
                json!({"sessionId":session,"area":"untracked","path":status["entries"][0]["path"]}),
            )
            .await
            .unwrap();
        assert_eq!(changed_lines(&diff), ["+exact file contents"]);
        f.app.shutdown().await.unwrap();
        f.config.shutdown().await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn git_queries_use_the_selected_managed_linked_worktree_not_its_source_repository() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let f = Fixture::configured("http://127.0.0.1:1/v1", None, None, None, |settings| {
            settings.isolated_workspaces = true
        })
        .await;
        let source_session = f.create("Source repository").await;
        repository(&f.workspace);
        std::fs::write(f.workspace.join("tracked.txt"), "source base\n").unwrap();
        git(&f.workspace, &["add", "tracked.txt"]);
        git(&f.workspace, &["commit", "--quiet", "-m", "source base"]);
        let mut input = f.create_input("Managed checkout").await;
        input["workspace"]["kind"] = json!("isolated_git");
        let created = f
            .mutation(2, "create", input, Uuid::new_v4())
            .await
            .unwrap();
        assert_eq!(created["kind"], "created", "{created}");
        let managed =
            std::path::PathBuf::from(created["session"]["workspace"]["path"].as_str().unwrap());
        assert!(
            managed.join(".git").is_file(),
            "exercise an actual linked worktree"
        );
        std::fs::write(managed.join("tracked.txt"), "managed current\n").unwrap();
        let session = &created["session"]["id"];
        let diff = f
            .query(
                "gitDiff",
                json!({"sessionId":session,"area":"worktree","path":path("tracked.txt")}),
            )
            .await
            .unwrap();
        assert_eq!(changed_lines(&diff), ["-source base", "+managed current"]);
        // Public browsing shares this exact managed-worktree exception, while
        // the private parent and administration directories remain unreachable.
        let files = f
            .query(
                "listFiles",
                json!({
                    "sessionId":session,"path":null,"after":null,
                }),
            )
            .await
            .unwrap();
        assert_eq!(
            files["entries"],
            json!([
                {"path":{"bytesBase64":"dHJhY2tlZC50eHQ="},"kind":"file"},
            ])
        );
        assert_eq!(files["next"], Value::Null);
        assert!(
            f.query(
                "listFiles",
                json!({
                    "sessionId":session,"path":path("../"),"after":null,
                })
            )
            .await
            .is_err()
        );
        let status = f
            .query("gitStatus", json!({"sessionId":session}))
            .await
            .unwrap();
        assert_eq!(
            status["entries"],
            json!([{"area":"worktree","path":{"bytesBase64":"dHJhY2tlZC50eHQ="},"kind":"modified"}])
        );
        assert_eq!(
            f.query("gitStatus", json!({"sessionId":source_session}))
                .await
                .unwrap()["entries"],
            json!([])
        );
        assert_eq!(
            std::fs::read_to_string(f.workspace.join("tracked.txt")).unwrap(),
            "source base\n"
        );
        f.app.shutdown().await.unwrap();
        f.config.shutdown().await.unwrap();
    })
    .await
    .unwrap();
}
