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

//! Client-owned correlation and original receipts, independent of UI nodes.
use crate::{CONTROL_KIND, CONTROL_METHODS, Result};
use maka_plugin_core::{
    composition::InstanceId,
    deployment::DeploymentRef,
    operation::{OperationCleanup, OperationOutcome, Receipt, RecoveryRef},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{
    Row, SqlitePool,
    sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow, SqliteSynchronous,
    },
};
use std::path::Path;
use uuid::Uuid;

/// One user attempt. Neither this observation nor its original reference grants access.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Operation {
    pub id: Uuid,
    pub host: DeploymentRef,
    pub endpoint: String,
    pub method: String,
    pub session_id: Option<String>,
    pub correlation: Value,
    #[serde(default)]
    pub draft: Option<DraftReference>,
    /// Durable fence for an attempt abandoned before preparation committed.
    #[serde(default)]
    pub abandoned: bool,
    /// Present only after original preparation has been durably recorded.
    pub reference: Option<RecoveryRef>,
    /// A known returned business outcome; absent is never proof that it did not run.
    pub receipt: Option<Receipt>,
}

/// Opened by the trusted launcher in the client's own data directory, never from guest input.
#[derive(Clone)]
pub struct Journal(
    pub(crate) SqlitePool,
    #[cfg(test)] pub(crate) std::sync::Arc<tokio::sync::Notify>,
);

/// Stable product owner, independent of endpoint aliases, VM instances and UI nodes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftKey {
    pub state_domain: String,
    pub deployment: Uuid,
    pub namespace: InstanceId,
    pub export: String,
    pub session_id: String,
    pub editor_id: String,
}
impl DraftKey {
    pub(crate) fn new(
        host: &DeploymentRef,
        namespace: &InstanceId,
        export: &str,
        session: &str,
        editor: &str,
    ) -> Result<Self> {
        if [session, editor]
            .iter()
            .any(|name| name.is_empty() || name.len() > 256 || name.chars().any(char::is_control))
        {
            return Err("invalid draft session or editor identity".into());
        }
        Ok(Self {
            state_domain: host.state_domain.clone(),
            deployment: host.deployment,
            namespace: namespace.clone(),
            export: export.into(),
            session_id: session.into(),
            editor_id: editor.into(),
        })
    }
    pub(crate) fn matches(&self, original: &RecoveryRef) -> bool {
        self.state_domain == original.source.state_domain
            && self.deployment == original.source.deployment
            && self.namespace == original.namespace
            && self.export == original.export
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Draft {
    pub revision: u64,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftReference {
    pub key: DraftKey,
    pub revision: u64,
}

impl Journal {
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(error)?;
        sqlx::query("CREATE TABLE IF NOT EXISTS operations (id TEXT PRIMARY KEY, domain TEXT NOT NULL, deployment TEXT NOT NULL, context TEXT NOT NULL, draft_owner TEXT, reference TEXT, receipt TEXT, abandoned INTEGER NOT NULL DEFAULT 0)")
            .execute(&pool).await.map_err(error)?;
        sqlx::query("CREATE INDEX IF NOT EXISTS operations_pending ON operations(domain,deployment,draft_owner) WHERE receipt IS NULL AND abandoned=0")
            .execute(&pool).await.map_err(error)?;
        sqlx::query("CREATE TABLE IF NOT EXISTS drafts (owner TEXT PRIMARY KEY, revision INTEGER NOT NULL, text TEXT NOT NULL, consumed INTEGER NOT NULL DEFAULT 0)")
            .execute(&pool).await.map_err(error)?;
        Ok(Self(
            pool,
            #[cfg(test)]
            std::sync::Arc::new(tokio::sync::Notify::new()),
        ))
    }
    pub(crate) async fn load_draft(&self, key: &DraftKey) -> Result<Option<Draft>> {
        let row = sqlx::query("SELECT revision,CASE WHEN consumed=1 THEN '' ELSE text END AS text FROM drafts WHERE owner=?")
            .bind(serde_json::to_string(key).map_err(error)?)
            .fetch_optional(&self.0)
            .await
            .map_err(error)?;
        row.map(|row| {
            Ok(Draft {
                revision: row.try_get::<i64, _>("revision").map_err(error)? as u64,
                text: row.try_get("text").map_err(error)?,
            })
        })
        .transpose()
    }
    pub(crate) async fn save_draft(
        &self,
        key: &DraftKey,
        expected: Option<u64>,
        text: String,
    ) -> Result<Draft> {
        if text.len() > 1024 * 1024
            || expected.is_some_and(|revision| revision == 0 || revision >= 9_007_199_254_740_991)
        {
            return Err("draft text or revision exceeds its limit".into());
        }
        let key = serde_json::to_string(key).map_err(error)?;
        let row = if let Some(revision) = expected {
            sqlx::query("UPDATE drafts SET revision=revision+1,text=?,consumed=0 WHERE owner=? AND revision=? RETURNING revision,text")
                .bind(text).bind(key).bind(revision as i64).fetch_optional(&self.0).await.map_err(error)?
        } else {
            sqlx::query("INSERT INTO drafts (owner,revision,text) VALUES (?,1,?) ON CONFLICT DO NOTHING RETURNING revision,text")
                .bind(key).bind(text).fetch_optional(&self.0).await.map_err(error)?
        }.ok_or("draft revision conflicts with a newer saved value")?;
        Ok(Draft {
            revision: row.try_get::<i64, _>("revision").map_err(error)? as u64,
            text: row.try_get("text").map_err(error)?,
        })
    }
    pub(crate) async fn reserve(&self, operation: &Operation) -> Result<()> {
        if operation.id.is_nil()
            || operation
                .session_id
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 256)
        {
            return Err("invalid client operation identity".into());
        }
        maka_plugin_core::json::check_delivery(&operation.correlation, 32 * 1024)?;
        let context = serde_json::to_string(operation).map_err(error)?;
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO operations (id,domain,deployment,context,draft_owner) VALUES (?,?,?,?,?)",
        )
        .bind(operation.id.to_string())
        .bind(&operation.host.state_domain)
        .bind(operation.host.deployment.to_string())
        .bind(context)
        .bind(operation.draft.as_ref().map(|draft| serde_json::to_string(&draft.key)).transpose().map_err(error)?)
        .execute(&self.0)
        .await
        .map_err(error)?;
        if inserted.rows_affected() != 1 {
            return Err("client operation already exists; recover the original attempt".into());
        }
        Ok(())
    }
    pub(crate) async fn prepared(&self, id: Uuid, reference: &RecoveryRef) -> Result<()> {
        let encoded = serde_json::to_string(reference).map_err(error)?;
        #[cfg(test)]
        self.1.notify_one();
        let updated = sqlx::query(
            "UPDATE operations SET reference=? WHERE id=? AND reference IS NULL AND abandoned=0",
        )
        .bind(encoded)
        .bind(id.to_string())
        .execute(&self.0)
        .await
        .map_err(error)?;
        if updated.rows_affected() != 1 {
            return Err(
                "original client preparation was already recorded or is unavailable".into(),
            );
        }
        Ok(())
    }
    pub(crate) async fn load(&self, host: &DeploymentRef, id: Uuid) -> Result<Operation> {
        let row = sqlx::query("SELECT context,reference,receipt,abandoned FROM operations WHERE id=? AND domain=? AND deployment=?")
            .bind(id.to_string()).bind(&host.state_domain).bind(host.deployment.to_string())
            .fetch_optional(&self.0).await.map_err(error)?.ok_or("original client operation not found for this Host")?;
        decode_operation(row)
    }
    pub(crate) async fn abandon(&self, host: &DeploymentRef, id: Uuid) -> Result<Operation> {
        sqlx::query("UPDATE operations SET abandoned=1 WHERE id=? AND domain=? AND deployment=? AND reference IS NULL AND receipt IS NULL")
            .bind(id.to_string()).bind(&host.state_domain).bind(host.deployment.to_string())
            .execute(&self.0).await.map_err(error)?;
        self.load(host, id).await
    }
    pub(crate) async fn pending(
        &self,
        host: &DeploymentRef,
        key: &DraftKey,
    ) -> Result<Vec<Operation>> {
        let rows = sqlx::query("SELECT context,reference,receipt,abandoned FROM operations WHERE domain=? AND deployment=? AND draft_owner=? AND receipt IS NULL AND abandoned=0 ORDER BY rowid DESC LIMIT 128")
            .bind(&host.state_domain).bind(host.deployment.to_string())
            .bind(serde_json::to_string(key).map_err(error)?).fetch_all(&self.0).await.map_err(error)?;
        rows.into_iter().map(decode_operation).collect()
    }
    pub(crate) async fn pending_controls(&self, host: &DeploymentRef) -> Result<Vec<Operation>> {
        let correlation = serde_json::json!({"kind": CONTROL_KIND}).to_string();
        let rows = sqlx::query("SELECT context,reference,receipt,abandoned FROM operations WHERE domain=? AND deployment=? AND draft_owner IS NULL AND json_extract(context,'$.endpoint')='agent' AND json_extract(context,'$.method') IN (SELECT value FROM json_each(?)) AND json_type(context,'$.correlation')='object' AND CASE json_extract(context,'$.method') WHEN 'stop' THEN json_type(context,'$.correlation.invocationId')='text' AND length(CAST(json_extract(context,'$.correlation.invocationId') AS BLOB)) BETWEEN 1 AND 128 AND json_extract(json_remove(context,'$.correlation.invocationId'),'$.correlation')=? WHEN 'respond' THEN json_type(context,'$.correlation.requestId')='text' AND length(CAST(json_extract(context,'$.correlation.requestId') AS BLOB)) BETWEEN 1 AND 128 AND json_extract(json_remove(context,'$.correlation.requestId'),'$.correlation')=? ELSE json_extract(context,'$.correlation')=? END AND receipt IS NULL AND abandoned=0 ORDER BY rowid DESC LIMIT 128")
            .bind(&host.state_domain).bind(host.deployment.to_string())
            .bind(serde_json::to_string(&CONTROL_METHODS).map_err(error)?)
            .bind(&correlation).bind(&correlation).bind(&correlation)
            .fetch_all(&self.0).await.map_err(error)?;
        rows.into_iter().map(decode_operation).collect()
    }
    pub(crate) async fn observe(&self, operation: &Operation, receipt: &Receipt) -> Result<()> {
        let Receipt::Recorded {
            outcome: OperationOutcome::Returned { .. },
            ..
        } = receipt
        else {
            // A failed observer and an absent/unresolved journal cannot erase a returned result.
            return Ok(());
        };
        let mut connection = self.0.acquire().await.map_err(error)?;
        let mut transaction = sqlx::Connection::begin_with(&mut *connection, "BEGIN IMMEDIATE")
            .await
            .map_err(error)?;
        let row = sqlx::query("SELECT receipt FROM operations WHERE id=?")
            .bind(operation.id.to_string())
            .fetch_one(&mut *transaction)
            .await
            .map_err(error)?;
        let previous: Option<Receipt> = row
            .try_get::<Option<&str>, _>("receipt")
            .map_err(error)?
            .map(serde_json::from_str)
            .transpose()
            .map_err(error)?;
        let mut next = receipt.clone();
        if let Some(Receipt::Recorded {
            outcome: before,
            cleanup: previous_cleanup,
        }) = previous
        {
            let Receipt::Recorded { outcome, cleanup } = &mut next else {
                unreachable!()
            };
            if before != *outcome {
                return Err("original operation returned conflicting receipts".into());
            }
            if previous_cleanup == OperationCleanup::Confirmed {
                *cleanup = previous_cleanup;
            }
        }
        sqlx::query("UPDATE operations SET receipt=? WHERE id=?")
            .bind(serde_json::to_string(&next).map_err(error)?)
            .bind(operation.id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(error)?;
        if let Some(draft) = &operation.draft
            && accepted_send(&next, &draft.key.session_id)
        {
            // Receipt and conditional draft clearing are one durable product
            // transition. A later edit or unknown outcome cannot clear this text.
            sqlx::query("UPDATE drafts SET consumed=1,text='' WHERE owner=? AND revision=?")
                .bind(serde_json::to_string(&draft.key).map_err(error)?)
                .bind(draft.revision as i64)
                .execute(&mut *transaction)
                .await
                .map_err(error)?;
        }
        transaction.commit().await.map_err(error)
    }
}
fn decode_operation(row: SqliteRow) -> Result<Operation> {
    let mut operation: Operation =
        serde_json::from_str(row.try_get("context").map_err(error)?).map_err(error)?;
    operation.reference = row
        .try_get::<Option<&str>, _>("reference")
        .map_err(error)?
        .map(serde_json::from_str)
        .transpose()
        .map_err(error)?;
    operation.receipt = row
        .try_get::<Option<&str>, _>("receipt")
        .map_err(error)?
        .map(serde_json::from_str)
        .transpose()
        .map_err(error)?;
    operation.abandoned = row.try_get::<i64, _>("abandoned").map_err(error)? != 0;
    Ok(operation)
}
fn accepted_send(receipt: &Receipt, session: &str) -> bool {
    let Receipt::Recorded {
        outcome: OperationOutcome::Returned { value },
        ..
    } = receipt
    else {
        return false;
    };
    value.get("sessionId").and_then(Value::as_str) == Some(session)
        && value
            .get("messageId")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
        && match value.get("kind").and_then(Value::as_str) {
            Some("started") => value
                .get("invocationId")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty()),
            Some("queued") => value
                .get("revision")
                .and_then(Value::as_u64)
                .is_some_and(|revision| revision <= 9_007_199_254_740_991),
            _ => false,
        }
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
