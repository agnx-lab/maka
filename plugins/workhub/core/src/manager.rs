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

use crate::{Access, Coordinator, assignment::Assignments};
use maka_agent_facts::language::Language;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::Notify;

pub struct Manager {
    pub(super) coordinator: Arc<Coordinator>,
    pub(super) assignments: Arc<Assignments>,
    pub(super) access: Arc<Access>,
    pub(super) queries: Arc<dyn maka_agent_ports::session::catalog::Queries>,
    wake: Notify,
    recovering: AtomicBool,
    report: Mutex<crate::recovery::Report>,
}
impl Manager {
    pub fn new(
        coordinator: Arc<Coordinator>,
        assignments: Arc<Assignments>,
        access: Arc<Access>,
        queries: Arc<dyn maka_agent_ports::session::catalog::Queries>,
    ) -> Self {
        Self {
            coordinator,
            assignments,
            access,
            queries,
            wake: Notify::new(),
            recovering: AtomicBool::new(true),
            report: Mutex::default(),
        }
    }

    pub async fn recover(&self, language: impl Fn() -> Language) {
        let mut delay = Duration::from_millis(250);
        loop {
            // Subscribe before reading facts: changes are invalidations, never
            // a second source of truth. A timer also covers restored consent.
            let mut changes = if self
                .assignments
                .repository
                .pending()
                .await
                .is_ok_and(|pending| !pending.is_empty())
            {
                self.coordinator
                    .resolve()
                    .await
                    .ok()
                    .and_then(|(_, commands)| commands.changes().ok())
            } else {
                None
            };
            let retry = match self.recover_decisions(language()).await {
                Ok(report) => {
                    let retry = report.retrying() || report.pending_results;
                    *self.report.lock().unwrap() = report;
                    retry
                }
                Err(error) => {
                    self.report.lock().unwrap().unavailable =
                        Some(error.to_string().chars().take(512).collect());
                    true
                }
            };
            self.recovering.store(retry, Ordering::Release);
            if retry {
                tokio::select! {
                    _ = self.wake.notified() => { delay = Duration::from_millis(250); }
                    _ = async { match &mut changes {
                        Some(changes) => {
                            if changes.changed().await.is_err() { std::future::pending::<()>().await; }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        None => std::future::pending().await,
                    }} => { delay = Duration::from_millis(250); }
                    _ = tokio::time::sleep(delay) => { delay = (delay * 2).min(Duration::from_secs(30)); }
                }
            } else {
                self.wake.notified().await;
                delay = Duration::from_millis(250);
            }
        }
    }
}
impl maka_capabilities::background::BackgroundWork for Manager {
    fn is_pending(&self) -> bool {
        self.recovering.load(Ordering::Acquire)
    }
    fn wake(&self) {
        self.wake.notify_one();
    }
}
