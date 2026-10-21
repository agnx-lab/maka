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

use crate::{Error, client::Client};
use agent_client_protocol::schema::v2 as acp;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use maka_agent_api::contract::{
    DirectoryPage, DirectorySelection, Session, SessionActivity, SessionPage,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Component, Path, PathBuf};

pub(super) fn ensure_idle(session: &Session) -> Result<(), Error> {
    if session.archived || !matches!(session.activity, SessionActivity::Idle) {
        return Err("Host session is archived or has active work".into());
    }
    Ok(())
}
/// A cwd is a path on the Host. Never inspect the ACP client's local filesystem.
pub(super) fn directory(path: &Path) -> Result<PathBuf, Error> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("Workspace cwd must be an absolute path without parent traversal".into());
    }
    Ok(path.components().collect())
}
pub(super) async fn select(client: &Client, cwd: &Path) -> Result<DirectorySelection, Error> {
    let DirectoryPage::Roots { roots } = client
        .query("agent", "directories", json!({"kind":"roots"}))
        .await?
    else {
        return Err("Host omitted published workspace roots".into());
    };
    let mut current = roots
        .into_iter()
        .filter(|root| cwd.starts_with(&root.path))
        .max_by_key(|root| root.path.len())
        .ok_or("Workspace cwd is outside published Host roots")?;
    loop {
        if Path::new(&current.path) == cwd {
            return Ok(current.selection);
        }
        let mut after = None;
        let mut seen = std::collections::HashSet::new();
        current = loop {
            let DirectoryPage::Children {
                children,
                after: next,
                ..
            } = client
                .query(
                    "agent",
                    "directories",
                    json!({"kind":"children","selection":current.selection,"after":after}),
                )
                .await?
            else {
                return Err("Host omitted workspace children".into());
            };
            if let Some(child) = children.into_iter().find(|child| {
                let path = Path::new(&child.path);
                // Canonical aliases may point back to this directory or an
                // ancestor. Every step must consume part of the target path.
                path != Path::new(&current.path)
                    && path.starts_with(&current.path)
                    && cwd.starts_with(path)
            }) {
                break child;
            }
            let Some(next) = next else {
                return Err("Workspace cwd is unavailable in the published Host root".into());
            };
            if seen.len() >= 4096 || !seen.insert(next.clone()) {
                return Err("Directory pagination exceeded its limit".into());
            }
            after = Some(next);
        };
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    revision: String,
    cursor: String,
    cwd: Option<PathBuf>,
}
pub(super) async fn read(
    client: &Client,
    request: acp::ListSessionsRequest,
) -> Result<acp::ListSessionsResponse, Error> {
    let cwd = request
        .cwd
        .map(|path| directory(path.as_ref()))
        .transpose()?;
    let position = if let Some(cursor) = request.cursor {
        let token = cursor.to_string();
        if token.len() > 8192 {
            return Err("ACP session list cursor is too large".into());
        }
        let cursor: Cursor = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(token)?)?;
        if cursor.cwd != cwd {
            return Err("ACP session list cursor belongs to another cwd filter".into());
        }
        json!({"kind":"after","revision":cursor.revision,"cursor":cursor.cursor})
    } else {
        json!({"kind":"start"})
    };
    let page: SessionPage = client
        .query("agent", "list", json!({"filter":"all","position":position}))
        .await?;
    let mut results = Vec::new();
    for session in page.sessions {
        if session.archived {
            continue;
        }
        let path = directory(Path::new(&session.workspace.path))?;
        if cwd.as_ref().is_some_and(|filter| filter != &path) {
            continue;
        }
        results.push(
            acp::SessionInfo::new(session.id, acp::AbsolutePath::new(path)).title(session.title),
        );
    }
    let next = page
        .after
        .map(|cursor| {
            let token = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&Cursor {
                revision: page.revision,
                cursor,
                cwd,
            })?);
            if token.len() > 8192 {
                return Err("Host session cursor exceeds ACP limit".into());
            }
            Ok::<_, Error>(acp::SessionListCursor::new(token))
        })
        .transpose()?;
    Ok(acp::ListSessionsResponse::new(results).next_cursor(next))
}
