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

use super::support::SqlStore;
use futures_util::{FutureExt, future::BoxFuture};
use maka_capabilities::storage::Mutation;
use maka_capabilities::storage::Record;
use maka_capabilities::storage::Store;
use maka_capabilities::storage::StoreError;
use maka_event_log::EventLog;
use maka_scheduler::{
    plan::Plan,
    repository::Repository,
    schedule::Schedule,
    task::{Create, Creator, Effect, Notification, Outcome, Run},
};
use std::sync::Arc;
use tokio::sync::Notify;

struct PausedRead {
    inner: SqlStore,
    entered: Arc<Notify>,
    proceed: Arc<Notify>,
}
impl Store for PausedRead {
    fn scan(
        &self,
        query: maka_capabilities::storage::Scan,
    ) -> BoxFuture<'_, Result<maka_capabilities::storage::Page, StoreError>> {
        self.inner.scan(query)
    }
    fn read(&self, key: String) -> BoxFuture<'_, Result<Option<Record>, StoreError>> {
        Box::pin(async move {
            if key == "scheduled-tasks:4:main:task:task-one" {
                self.entered.notify_one();
                self.proceed.notified().await;
            }
            self.inner.read(key).await
        })
    }
    fn batch(&self, mutations: Vec<Mutation>) -> BoxFuture<'_, Result<Vec<Record>, StoreError>> {
        self.inner.batch(mutations)
    }
}

#[tokio::test]
async fn pending_trigger_survives_lost_reply_and_restart_while_stale_writers_and_id_reuse_fail() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("scheduler.sqlite");
    let log = Arc::new(EventLog::open(&path).await.unwrap());
    let repository = Repository::new(Arc::new(SqlStore(log.clone())), "main").unwrap();
    let mut catalog = repository.load().await.unwrap();
    let plan = Plan::create(
        "task-one".into(),
        Create {
            title: "Reminder".into(),
            intent_body: "Read the result".into(),
            schedule: Schedule::Once { run_at: 10_000 },
            effect: Effect::Notify(Notification::Local),
            max_fires: None,
            expires_at: None,
        },
        Creator::User,
        "Asia/Singapore".into(),
        1_000,
    )
    .unwrap();
    repository.save(&mut catalog, plan.clone()).await.unwrap();
    let isolated = Repository::new(Arc::new(SqlStore(log.clone())), "other").unwrap();
    assert!(isolated.load().await.unwrap().plans.is_empty());
    let mut stale = repository.load().await.unwrap();
    let mut triggered = plan.clone();
    triggered.claim(10_000).unwrap();
    // Mark attempted delivery before the external effect; this outcome cannot
    // be cancelled like an explicitly deferred notification.
    triggered.pending.as_mut().unwrap().delivery_started = true;
    let fire = triggered.pending.clone().unwrap();
    // The SQL owner finishes an accepted batch after its response waiter is lost.
    assert!(
        repository
            .save(&mut catalog, triggered.clone())
            .now_or_never()
            .is_none()
    );
    let committed = repository.load().await.unwrap();
    assert_eq!(committed.plans["task-one"].plan.pending, Some(fire.clone()));
    assert!(matches!(
        repository.save(&mut stale, plan.clone()).await,
        Err(maka_scheduler::Error::Storage(StoreError::Conflict { .. }))
    ));
    log.shutdown().await.unwrap();
    drop(repository);
    drop(log);

    let log = Arc::new(EventLog::open(&path).await.unwrap());
    let repository = Repository::new(Arc::new(SqlStore(log.clone())), "main").unwrap();
    let mut catalog = repository.load().await.unwrap();
    let mut recovered = catalog.plans["task-one"].plan.clone();
    recovered.recover(30_000).unwrap();
    assert_eq!(recovered.claim(30_000).unwrap(), Some(&fire));
    assert!(repository.remove(&mut catalog, "task-one").await.is_err());
    recovered
        .settle(Run {
            id: fire.id,
            at: 30_000,
            outcome: Outcome::Blocked,
            message: "Notification outcome unknown; not replayed".into(),
            session_id: None,
            run_id: None,
        })
        .unwrap();
    repository.save(&mut catalog, recovered).await.unwrap();
    let entered = Arc::new(Notify::new());
    let proceed = Arc::new(Notify::new());
    let concurrent = Repository::new(
        Arc::new(PausedRead {
            inner: SqlStore(log.clone()),
            entered: entered.clone(),
            proceed: proceed.clone(),
        }),
        "main",
    )
    .unwrap();
    let reader = tokio::spawn(async move { concurrent.load().await });
    entered.notified().await;
    repository.remove(&mut catalog, "task-one").await.unwrap();
    proceed.notify_one();
    assert!(matches!(
        reader.await.unwrap(),
        Err(maka_scheduler::Error::Storage(StoreError::Conflict { .. }))
    ));
    assert!(repository.load().await.unwrap().plans.is_empty());
    assert!(repository.save(&mut catalog, plan).await.is_err());
    log.shutdown().await.unwrap();
}

#[tokio::test]
async fn complete_run_history_survives_original_store_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("complete-history.sqlite");
    let log = Arc::new(EventLog::open(&path).await.unwrap());
    let repository = Repository::new(Arc::new(SqlStore(log.clone())), "main").unwrap();
    let mut catalog = repository.load().await.unwrap();
    let mut plan = Plan::create(
        "complete-history".into(),
        Create {
            title: "Every original result".into(),
            intent_body: "Keep the full history".into(),
            schedule: Schedule::Interval {
                every_seconds: 10,
                start_at: 10_000,
            },
            effect: Effect::Notify(Notification::Local),
            max_fires: None,
            expires_at: None,
        },
        Creator::User,
        "UTC".into(),
        1_000,
    )
    .unwrap();
    let mut originals = Vec::new();
    for ordinal in 0..25 {
        let at = 10_000 + ordinal * 10_000;
        let fire = plan.claim(at).unwrap().unwrap().clone();
        let run = Run {
            id: fire.id,
            at,
            outcome: Outcome::Ok,
            message: format!("original result {ordinal}"),
            session_id: None,
            run_id: None,
        };
        originals.insert(0, run.clone());
        plan.settle(run).unwrap();
        repository.save(&mut catalog, plan.clone()).await.unwrap();
    }
    drop(repository);
    log.shutdown().await.unwrap();
    drop(log);
    let reopened = Arc::new(EventLog::open(&path).await.unwrap());
    let repository = Repository::new(Arc::new(SqlStore(reopened.clone())), "main").unwrap();
    let loaded = repository.load().await;
    drop(repository);
    reopened.shutdown().await.unwrap();
    let loaded = loaded.unwrap();
    let saved = &loaded.plans["complete-history"].plan.task;
    assert_eq!(saved.fire_count, 25);
    assert_eq!(saved.runs, originals, "reopen lost original settled runs");
}

#[tokio::test]
async fn complete_catalog_and_large_task_survive_original_store_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("complete-catalog.sqlite");
    let log = Arc::new(EventLog::open(&path).await.unwrap());
    let repository = Repository::new(Arc::new(SqlStore(log.clone())), "main").unwrap();
    let mut catalog = repository.load().await.unwrap();
    let body = "original evidence 🦀\n".repeat(6000);
    for ordinal in 0..257 {
        let mut plan = Plan::create(
            format!("task-{ordinal:03}"),
            Create {
                title: if ordinal == 0 {
                    "Complete title ".repeat(20)
                } else {
                    format!("Task {ordinal}")
                },
                intent_body: if ordinal == 0 {
                    body.clone()
                } else {
                    "Work".into()
                },
                schedule: Schedule::Interval {
                    every_seconds: 1,
                    start_at: 1000,
                },
                effect: Effect::Notify(Notification::Local),
                max_fires: Some(10001),
                expires_at: None,
            },
            Creator::User,
            "UTC".into(),
            0,
        )
        .unwrap();
        if ordinal == 0 {
            let fire = plan.claim(1000).unwrap().unwrap().clone();
            plan.settle(Run {
                id: fire.id,
                at: 1000,
                outcome: Outcome::Failed,
                message: body.clone(),
                session_id: None,
                run_id: None,
            })
            .unwrap();
        }
        repository.save(&mut catalog, plan).await.unwrap();
    }
    drop(repository);
    log.shutdown().await.unwrap();
    drop(log);
    let reopened = Arc::new(EventLog::open(&path).await.unwrap());
    let repository = Repository::new(Arc::new(SqlStore(reopened.clone())), "main").unwrap();
    let loaded = repository.load().await;
    drop(repository);
    reopened.shutdown().await.unwrap();
    let loaded = loaded.unwrap();
    assert_eq!(loaded.plans.len(), 257);
    assert_eq!(
        loaded.plans["task-000"].plan.task.intent.body(),
        body.trim()
    );
    assert_eq!(loaded.plans["task-000"].plan.task.runs[0].message, body);
    assert_eq!(
        loaded.plans["task-000"].plan.task.last_error.as_deref(),
        Some(body.as_str())
    );
    assert_eq!(loaded.plans["task-256"].plan.task.title, "Task 256");
}
