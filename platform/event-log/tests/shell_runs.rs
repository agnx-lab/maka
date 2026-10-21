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

use maka_agent_facts::shell_run::ShellOutcome;
use maka_agent_facts::shell_run::ShellOutput;
use maka_agent_facts::shell_run::ShellPatch;
use maka_agent_facts::shell_run::ShellRun;
use maka_agent_facts::shell_run::ShellState;
use maka_agent_facts::shell_run::ShellVisibility;
use maka_event_log::{EventLog, StoreError};
use serde_json::json;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

fn starting(id: &str) -> ShellRun {
    ShellRun {
        id: id.into(),
        session_id: "session".into(),
        source_run_id: Some("run".into()),
        source_turn_id: "turn".into(),
        source_tool_call_id: "call".into(),
        visibility: ShellVisibility::Model,
        permissions: maka_agent_facts::shell_run::ShellPermissions {
            boundary_revision: 0,
            sandbox: maka_agent_facts::shell_run::Sandbox::Disabled,
        },
        cwd: "/captured/workspace".into(),
        command: "command".into(),
        started_at: 10,
        updated_at: 10,
        timeout_ms: None,
        revision: 1,
        state: ShellState::Starting,
        output: output(""),
    }
}

fn output(text: &str) -> ShellOutput {
    ShellOutput::Pipes {
        stdout: text.into(),
        stderr: String::new(),
        latest_stream: None,
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

fn state(state: ShellState) -> ShellPatch {
    ShellPatch {
        state: Some(state),
        ..Default::default()
    }
}

fn terminal(outcome: ShellOutcome) -> ShellState {
    ShellState::Terminal {
        completed_at: 20,
        outcome,
        observed_at: None,
    }
}

#[tokio::test]
async fn durable_lifecycle_keeps_terminal_output_mutable_and_never_reattaches_on_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("runtime.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    log.create_session(
        "session",
        "create",
        &json!({"workspace":{"hostCwd":"/captured/workspace"}}),
        1,
    )
    .await
    .unwrap();
    let record = starting("finished");
    let mut changes = log.subscribe_shell_changes();
    assert_eq!(log.create_shell_run(record.clone()).await.unwrap(), record);
    let change = changes.try_recv().unwrap();
    assert_eq!((&*change.session_id, &*change.id), ("session", "finished"));
    assert_eq!(
        log.read_shell_run(&change.session_id, &change.id)
            .await
            .unwrap(),
        Some(record.clone())
    );
    assert!(matches!(
        log.create_shell_run(record.clone()).await,
        Err(StoreError::ShellConflict)
    ));
    assert!(
        log.read_shell_run("other", "finished")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        log.patch_shell_run(
            "session",
            "finished",
            state(terminal(ShellOutcome::Completed))
        )
        .await
        .is_err(),
        "starting cannot claim a completed process"
    );
    let running = log
        .patch_shell_run("session", "finished", state(ShellState::Running))
        .await
        .unwrap();
    assert_eq!(running.revision, 2);
    assert_eq!(changes.try_recv().unwrap(), change);
    assert!(
        matches!(
            changes.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ),
        "conflicts and rejected transitions publish no invalidations"
    );
    assert!(
        log.patch_shell_run(
            "session",
            "finished",
            ShellPatch {
                observed_at: Some(19),
                ..Default::default()
            }
        )
        .await
        .is_err()
    );

    let finished = log
        .patch_shell_run(
            "session",
            "finished",
            ShellPatch {
                state: Some(terminal(ShellOutcome::Completed)),
                output: Some(output("first frame")),
                updated_at: Some(20),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(finished.revision, 3);
    let flushed = log
        .patch_shell_run(
            "session",
            "finished",
            ShellPatch {
                output: Some(output("first frame\nfinal frame 中文")),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(flushed.revision, 4);
    assert_eq!(flushed.state, finished.state);
    assert!(
        log.patch_shell_run(
            "session",
            "finished",
            state(terminal(ShellOutcome::Cancelled { message: None }))
        )
        .await
        .is_err()
    );
    assert!(
        log.patch_shell_run("session", "finished", state(ShellState::Running))
            .await
            .is_err()
    );
    let observed = log
        .patch_shell_run(
            "session",
            "finished",
            ShellPatch {
                observed_at: Some(30),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(observed.revision, 5);
    while changes.try_recv().is_ok() {}
    assert_eq!(
        log.patch_shell_run(
            "session",
            "finished",
            ShellPatch {
                observed_at: Some(40),
                ..Default::default()
            }
        )
        .await
        .unwrap(),
        observed
    );
    assert!(
        matches!(
            changes.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ),
        "an unchanged observation publishes no new revision"
    );

    for id in ["unspawned", "lost"] {
        log.create_shell_run(starting(id)).await.unwrap();
    }
    log.patch_shell_run("session", "lost", state(ShellState::Running))
        .await
        .unwrap();
    assert_eq!(
        log.prefix(1, 1024).await.unwrap().high_water,
        0,
        "resource control is not fabricated invocation history"
    );
    log.close().await.unwrap();

    let log = EventLog::open(&path).await.unwrap();
    let before = log.workspace_fence("/captured/workspace").await.unwrap();
    assert_eq!(before.0.len(), 1);
    assert!(!before.0[0].orphaned_shells);
    assert_eq!(log.recover_shell_runs(50).await.unwrap(), 2);
    let after = log.workspace_fence("/captured/workspace").await.unwrap();
    assert!(
        after.0[0].orphaned_shells,
        "unknown cleanup is not a transient busy workspace"
    );
    assert_ne!(before, after);
    assert_eq!(log.recover_shell_runs(60).await.unwrap(), 0);
    assert_eq!(
        log.read_shell_run("session", "finished").await.unwrap(),
        Some(observed)
    );
    for (id, revision) in [("unspawned", 2), ("lost", 3)] {
        let orphaned = log.read_shell_run("session", id).await.unwrap().unwrap();
        assert_eq!(orphaned.revision, revision);
        assert_eq!(orphaned.updated_at, 50);
        assert!(matches!(
            orphaned.state,
            ShellState::Terminal {
                completed_at: 50,
                outcome: ShellOutcome::Orphaned { .. },
                observed_at: None
            }
        ));
        assert_eq!(orphaned.output, output(""));
        assert!(
            log.patch_shell_run("session", id, state(ShellState::Running))
                .await
                .is_err()
        );
    }
    log.close().await.unwrap();
}

#[tokio::test]
async fn recovery_commit_is_atomic_under_real_sqlite_faults() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("runtime.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    log.create_session("session", "create", &json!({}), 1)
        .await
        .unwrap();
    for id in ["a", "b"] {
        log.create_shell_run(starting(id)).await.unwrap();
    }
    let mut observer = SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TRIGGER reject_second_recovery BEFORE UPDATE ON shell_runs
        WHEN EXISTS (SELECT 1 FROM shell_runs WHERE active = 0)
        BEGIN SELECT RAISE(ABORT, 'injected recovery fault'); END;",
    )
    .execute(&mut observer)
    .await
    .unwrap();
    assert!(log.recover_shell_runs(50).await.is_err());
    for id in ["a", "b"] {
        assert_eq!(
            log.read_shell_run("session", id).await.unwrap(),
            Some(starting(id)),
            "all recovery writes roll back together"
        );
    }
    sqlx::raw_sql(
        "DROP TRIGGER reject_second_recovery;
        CREATE TRIGGER reject_outcome BEFORE UPDATE ON shell_runs
        BEGIN SELECT RAISE(ABORT, 'injected outcome fault'); END;",
    )
    .execute(&mut observer)
    .await
    .unwrap();
    let mut changes = log.subscribe_shell_changes();
    assert!(
        log.patch_shell_run("session", "a", state(ShellState::Running))
            .await
            .is_err()
    );
    assert_eq!(
        log.read_shell_run("session", "a").await.unwrap(),
        Some(starting("a"))
    );
    assert!(
        matches!(
            changes.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ),
        "rolled-back writes publish no invalidations"
    );
    sqlx::raw_sql("DROP TRIGGER reject_outcome")
        .execute(&mut observer)
        .await
        .unwrap();
    // A real SQLite writer lock holds the accepted update after its caller
    // disappears. Publication belongs to the SQL owner, not the abandoned await.
    let blocker = observer.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let mut update = Box::pin(log.patch_shell_run("session", "a", state(ShellState::Running)));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut update)
            .await
            .is_err()
    );
    assert!(matches!(
        changes.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    drop(update);
    blocker.commit().await.unwrap();
    let change = tokio::time::timeout(std::time::Duration::from_secs(5), changes.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!((&*change.session_id, &*change.id), ("session", "a"));
    assert_eq!(
        log.read_shell_run("session", "a")
            .await
            .unwrap()
            .unwrap()
            .state,
        ShellState::Running
    );
    observer.close().await.unwrap();
    log.close().await.unwrap();
    let log = EventLog::open(&path).await.unwrap();
    assert_eq!(log.recover_shell_runs(60).await.unwrap(), 2);
    log.close().await.unwrap();
}

#[tokio::test]
async fn read_prefixes_exclude_user_only_and_foreign_tasks_and_prefer_exact_ids() {
    use maka_agent_facts::read::ResourceAddress::Task;
    let temp = tempfile::tempdir().unwrap();
    let log = EventLog::open(&temp.path().join("prefix.sqlite"))
        .await
        .unwrap();
    for session in ["session", "other"] {
        log.create_session(session, session, &json!({}), 1)
            .await
            .unwrap();
    }
    for id in ["task_a", "task_ab"] {
        log.create_shell_run(starting(id)).await.unwrap();
    }
    let mut hidden = starting("task_hidden");
    hidden.visibility = ShellVisibility::User;
    log.create_shell_run(hidden).await.unwrap();
    let mut foreign = starting("task_foreign");
    foreign.session_id = "other".into();
    log.create_shell_run(foreign).await.unwrap();
    assert_eq!(
        log.resolve_read_resource("session", Task("task_a".into()))
            .await
            .unwrap(),
        Some(Task("task_a".into()))
    );
    assert!(matches!(
        log.resolve_read_resource("session", Task("task_".into()))
            .await,
        Err(StoreError::AmbiguousReadResource)
    ));
    for prefix in ["task_h", "task_f", "TASK_A"] {
        assert!(
            log.resolve_read_resource("session", Task(prefix.into()))
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        log.resolve_read_resource("other", Task("task_".into()))
            .await
            .unwrap(),
        Some(Task("task_foreign".into()))
    );
    log.close().await.unwrap();
}

#[tokio::test]
async fn provider_catalog_filters_before_count_pagination_and_digest() {
    let dir = tempfile::tempdir().unwrap();
    let log = EventLog::open(&dir.path().join("catalog.sqlite"))
        .await
        .unwrap();
    log.create_session("session", "create", &json!({}), 1)
        .await
        .unwrap();
    for prefix in ["a_other_", "z_owner_"] {
        for index in 0..70 {
            let id = format!("{prefix}{index:03}");
            let record = starting(&id);
            log.create_shell_run(record).await.unwrap();
            log.patch_shell_run(
                "session",
                &id,
                state(terminal(ShellOutcome::Failed {
                    message: "fixture".into(),
                })),
            )
            .await
            .unwrap();
        }
    }
    log.create_shell_run(starting("zXownerX000")).await.unwrap();
    let page = log
        .query_shell_catalog("session", "z_owner_", 0)
        .await
        .unwrap();
    assert_eq!(page.total, 70);
    assert_eq!(page.resources.len(), 64);
    assert_eq!(page.next_offset, Some(64));
    assert!(
        page.resources
            .iter()
            .all(|r| r.result.output.is_none() && r.result.resource_ref.contains("z_owner_"))
    );
    let last = log
        .query_shell_catalog("session", "z_owner_", 64)
        .await
        .unwrap();
    assert_eq!(last.resources.len(), 6);
    assert_eq!(last.total, 70);
    assert_eq!(last.revision, page.revision);
    assert_eq!(last.next_offset, None);
    log.patch_shell_run(
        "session",
        "a_other_000",
        ShellPatch {
            output: Some(output("unrelated")),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        log.query_shell_catalog("session", "z_owner_", 0)
            .await
            .unwrap()
            .revision,
        page.revision
    );
    assert!(
        log.query_shell_catalog("session", "z_owner_%", 0)
            .await
            .is_err()
    );
    assert!(log.query_shell_catalog("session", "", 0).await.is_err());
}

#[tokio::test]
async fn soft_first_shell_resource_keeps_complete_identity_and_offset() {
    let temp = tempfile::tempdir().unwrap();
    let log = EventLog::open(&temp.path().join("singleton.sqlite"))
        .await
        .unwrap();
    log.create_session("session", "create", &json!({}), 1)
        .await
        .unwrap();
    let identity = "original-turn-".repeat(6000);
    let mut first = starting("a_first");
    first.source_turn_id = identity.clone();
    log.create_shell_run(first).await.unwrap();
    log.create_shell_run(starting("b_second")).await.unwrap();
    let page = log.query_shell_catalog("session", "a_", 0).await;
    let all = log.query_shell_resources("session", None, 0).await;
    let tail = log.query_shell_resources("session", None, 1).await;
    let foreign = log.query_shell_resources("missing", None, 0).await;
    log.close().await.unwrap();
    let page = page.expect("first complete catalog resource must progress");
    assert_eq!(page.resources.len(), 1);
    assert!(page.resources[0].source_turn_id == identity);
    assert_eq!(page.total, 1);
    assert_eq!(page.next_offset, None);
    let all = all.expect("first complete resource must progress");
    assert_eq!(all.resources.len(), 1);
    assert!(all.resources[0].source_turn_id == identity);
    assert_eq!(all.total, 2);
    assert_eq!(all.next_offset, Some(1));
    let tail = tail.unwrap();
    assert_eq!(tail.resources.len(), 1);
    assert_eq!(tail.resources[0].source_turn_id, "turn");
    assert_eq!(tail.revision, all.revision);
    assert_eq!(tail.next_offset, None);
    assert!(matches!(foreign, Err(StoreError::SessionNotFound)));
}
