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
use crate::{CONTROL_KIND, CONTROL_METHODS, Result, SETTINGS_KIND, SETTINGS_METHODS};
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

// Keep admission and pending observations on the same immutable Settings owner.
const SETTINGS_OWNER: &str = "draft_owner IS NULL AND json_type(context,'$.draft')='null' AND json_type(context,'$.sessionId')='null' AND json_type(context,'$.correlation')='object' AND json_extract(context,'$.correlation')=? AND ((json_extract(context,'$.endpoint')='settings' AND json_extract(context,'$.method') IN (SELECT value FROM json_each(?))) OR (json_extract(context,'$.endpoint')='setup' AND json_extract(context,'$.method')='discoverModels'))";

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
    #[serde(default)]
    pub upload: Option<UploadReference>,
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
    #[serde(default)]
    pub attachments: Vec<DraftAttachment>,
}

/// Display metadata for an immutable Host upload in this exact draft owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftAttachment {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub bytes: u64,
}

/// One complete editor revision, including the upload receipts it has handled.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftEdit {
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<DraftAttachment>,
    #[serde(default)]
    pub settled_uploads: Vec<Uuid>,
}
impl From<String> for DraftEdit {
    fn from(text: String) -> Self {
        Self {
            text,
            ..Self::default()
        }
    }
}
impl From<&str> for DraftEdit {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UploadReference {
    pub key: DraftKey,
    pub name: String,
    pub bytes: usize,
    pub digest: String,
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
        let mut connection = pool.acquire().await.map_err(error)?;
        // Serialize schema inspection and ALTER across independent client scopes.
        let mut transaction = sqlx::Connection::begin_with(&mut *connection, "BEGIN IMMEDIATE")
            .await
            .map_err(error)?;
        sqlx::query("CREATE TABLE IF NOT EXISTS operations (id TEXT PRIMARY KEY, domain TEXT NOT NULL, deployment TEXT NOT NULL, context TEXT NOT NULL, draft_owner TEXT, reference TEXT, receipt TEXT, abandoned INTEGER NOT NULL DEFAULT 0)")
            .execute(&mut *transaction).await.map_err(error)?;
        sqlx::query("CREATE INDEX IF NOT EXISTS operations_pending ON operations(domain,deployment,draft_owner) WHERE receipt IS NULL AND abandoned=0")
            .execute(&mut *transaction).await.map_err(error)?;
        sqlx::query("CREATE TABLE IF NOT EXISTS drafts (owner TEXT PRIMARY KEY, revision INTEGER NOT NULL, text TEXT NOT NULL, consumed INTEGER NOT NULL DEFAULT 0)")
            .execute(&mut *transaction).await.map_err(error)?;
        for (table, column, alter) in [
            (
                "drafts",
                "attachments",
                "ALTER TABLE drafts ADD COLUMN attachments TEXT NOT NULL DEFAULT '[]'",
            ),
            (
                "operations",
                "upload_adopted",
                "ALTER TABLE operations ADD COLUMN upload_adopted INTEGER NOT NULL DEFAULT 0",
            ),
        ] {
            let columns = sqlx::query(&format!("PRAGMA table_info({table})"))
                .fetch_all(&mut *transaction)
                .await
                .map_err(error)?;
            if !columns
                .iter()
                .any(|row| row.get::<&str, _>("name") == column)
            {
                sqlx::query(alter)
                    .execute(&mut *transaction)
                    .await
                    .map_err(error)?;
            }
        }
        transaction.commit().await.map_err(error)?;
        drop(connection);
        Ok(Self(
            pool,
            #[cfg(test)]
            std::sync::Arc::new(tokio::sync::Notify::new()),
        ))
    }
    pub(crate) async fn load_draft(&self, key: &DraftKey) -> Result<Option<Draft>> {
        let row = sqlx::query("SELECT revision,CASE WHEN consumed=1 THEN '' ELSE text END AS text,CASE WHEN consumed=1 THEN '[]' ELSE attachments END AS attachments FROM drafts WHERE owner=?")
            .bind(serde_json::to_string(key).map_err(error)?)
            .fetch_optional(&self.0)
            .await
            .map_err(error)?;
        row.map(|row| {
            Ok(Draft {
                revision: row.try_get::<i64, _>("revision").map_err(error)? as u64,
                text: row.try_get("text").map_err(error)?,
                attachments: serde_json::from_str(row.try_get("attachments").map_err(error)?)
                    .map_err(error)?,
            })
        })
        .transpose()
    }
    pub(crate) async fn save_draft(
        &self,
        key: &DraftKey,
        expected: Option<u64>,
        edit: DraftEdit,
    ) -> Result<Draft> {
        if edit.text.len() > 1024 * 1024
            || expected.is_some_and(|revision| revision == 0 || revision >= 9_007_199_254_740_991)
            || edit.settled_uploads.len() > 128
        {
            return Err("draft text or revision exceeds its limit".into());
        }
        let attachments = serde_json::to_string(&edit.attachments).map_err(error)?;
        if attachments.len() > 64 * 1024
            || edit.attachments.iter().any(|attachment| {
                attachment.id.is_empty()
                    || attachment.id.len() > 128
                    || attachment.name.is_empty()
                    || attachment.name.len() > 512
                    || attachment.mime_type.len() > 128
                    || attachment.bytes > 50 * 1024 * 1024
            })
            || edit
                .attachments
                .iter()
                .enumerate()
                .any(|(index, attachment)| {
                    edit.attachments[..index]
                        .iter()
                        .any(|other| other.id == attachment.id)
                })
        {
            return Err("invalid draft attachments".into());
        }
        let encoded_key = serde_json::to_string(key).map_err(error)?;
        let mut connection = self.0.acquire().await.map_err(error)?;
        let mut transaction = sqlx::Connection::begin_with(&mut *connection, "BEGIN IMMEDIATE")
            .await
            .map_err(error)?;
        // A save queued before a send receipt may still follow its consumption
        // at this same revision, as text edits already can. Keep its verified
        // metadata available for that CAS; consumed load still returns [].
        let previous: Option<String> =
            sqlx::query_scalar("SELECT attachments FROM drafts WHERE owner=?")
                .bind(&encoded_key)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(error)?;
        let mut admitted: Vec<DraftAttachment> = previous
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(error)?
            .unwrap_or_default();
        for id in &edit.settled_uploads {
            let row = sqlx::query("SELECT context,reference,receipt,abandoned FROM operations WHERE id=? AND domain=? AND deployment=? AND draft_owner=?")
                .bind(id.to_string()).bind(&key.state_domain).bind(key.deployment.to_string())
                .bind(&encoded_key).fetch_optional(&mut *transaction).await.map_err(error)?
                .ok_or("original attachment upload not found for this draft")?;
            let original = decode_operation(row)?;
            validate_upload(&original, key)?;
            let attachment = uploaded_attachment(&original)?;
            if let Some(attachment) = attachment {
                admitted.push(attachment);
            }
            sqlx::query("UPDATE operations SET upload_adopted=1 WHERE id=?")
                .bind(id.to_string())
                .execute(&mut *transaction)
                .await
                .map_err(error)?;
        }
        if edit
            .attachments
            .iter()
            .any(|attachment| !admitted.contains(attachment))
        {
            return Err("draft attachment differs from its original upload receipt".into());
        }
        let row = if let Some(revision) = expected {
            sqlx::query("UPDATE drafts SET revision=revision+1,text=?,attachments=?,consumed=0 WHERE owner=? AND revision=? RETURNING revision,text,attachments")
                .bind(edit.text).bind(attachments).bind(encoded_key).bind(revision as i64)
                .fetch_optional(&mut *transaction).await.map_err(error)?
        } else {
            sqlx::query("INSERT INTO drafts (owner,revision,text,attachments) VALUES (?,1,?,?) ON CONFLICT DO NOTHING RETURNING revision,text,attachments")
                .bind(encoded_key).bind(edit.text).bind(attachments)
                .fetch_optional(&mut *transaction).await.map_err(error)?
        }.ok_or("draft revision conflicts with a newer saved value")?;
        let draft = Draft {
            revision: row.try_get::<i64, _>("revision").map_err(error)? as u64,
            text: row.try_get("text").map_err(error)?,
            attachments: serde_json::from_str(row.try_get("attachments").map_err(error)?)
                .map_err(error)?,
        };
        transaction.commit().await.map_err(error)?;
        Ok(draft)
    }
    pub(crate) async fn reserve(&self, operation: &Operation) -> Result<()> {
        let context = encode_operation(operation)?;
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO operations (id,domain,deployment,context,draft_owner) VALUES (?,?,?,?,?)",
        )
        .bind(operation.id.to_string())
        .bind(&operation.host.state_domain)
        .bind(operation.host.deployment.to_string())
        .bind(context)
        .bind(operation.draft.as_ref().map(|draft| &draft.key).or_else(|| operation.upload.as_ref().map(|upload| &upload.key)).map(serde_json::to_string).transpose().map_err(error)?)
        .execute(&self.0)
        .await
        .map_err(error)?;
        if inserted.rows_affected() != 1 {
            return Err("client operation already exists; recover the original attempt".into());
        }
        Ok(())
    }
    pub(crate) async fn reserve_settings(&self, operation: &Operation) -> Result<()> {
        let context = encode_operation(operation)?;
        let statement = format!(
            "INSERT OR IGNORE INTO operations (id,domain,deployment,context,draft_owner) SELECT ?,?,?,?,NULL WHERE NOT EXISTS (SELECT 1 FROM operations WHERE domain=? AND deployment=? AND {SETTINGS_OWNER} AND receipt IS NULL AND abandoned=0)"
        );
        let inserted = sqlx::query(&statement)
            .bind(operation.id.to_string())
            .bind(&operation.host.state_domain)
            .bind(operation.host.deployment.to_string())
            .bind(context)
            .bind(&operation.host.state_domain)
            .bind(operation.host.deployment.to_string())
            .bind(serde_json::json!({"kind": SETTINGS_KIND}).to_string())
            .bind(serde_json::to_string(&SETTINGS_METHODS).map_err(error)?)
            .execute(&self.0)
            .await
            .map_err(error)?;
        if inserted.rows_affected() != 1 {
            return Err("settings operation already exists or has an unresolved original".into());
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
        let rows = sqlx::query("SELECT context,reference,receipt,abandoned FROM operations WHERE domain=? AND deployment=? AND draft_owner=? AND json_type(context,'$.draft')='object' AND receipt IS NULL AND abandoned=0 ORDER BY rowid DESC LIMIT 128")
            .bind(&host.state_domain).bind(host.deployment.to_string())
            .bind(serde_json::to_string(key).map_err(error)?).fetch_all(&self.0).await.map_err(error)?;
        rows.into_iter().map(decode_operation).collect()
    }
    pub(crate) async fn pending_uploads(
        &self,
        host: &DeploymentRef,
        key: &DraftKey,
    ) -> Result<Vec<Operation>> {
        let rows = sqlx::query("SELECT context,reference,receipt,abandoned FROM operations WHERE domain=? AND deployment=? AND draft_owner=? AND json_type(context,'$.upload')='object' AND upload_adopted=0 AND abandoned=0 ORDER BY rowid LIMIT 128")
            .bind(&host.state_domain).bind(host.deployment.to_string())
            .bind(serde_json::to_string(key).map_err(error)?).fetch_all(&self.0).await.map_err(error)?;
        let operations = rows
            .into_iter()
            .map(decode_operation)
            .collect::<Result<Vec<_>>>()?;
        for operation in &operations {
            validate_upload(operation, key)?;
        }
        Ok(operations)
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
    pub(crate) async fn pending_settings(&self, host: &DeploymentRef) -> Result<Vec<Operation>> {
        let statement = format!(
            "SELECT context,reference,receipt,abandoned FROM operations WHERE domain=? AND deployment=? AND {SETTINGS_OWNER} AND receipt IS NULL AND abandoned=0 ORDER BY rowid DESC LIMIT 128"
        );
        let rows = sqlx::query(&statement)
            .bind(&host.state_domain)
            .bind(host.deployment.to_string())
            .bind(serde_json::json!({"kind": SETTINGS_KIND}).to_string())
            .bind(serde_json::to_string(&SETTINGS_METHODS).map_err(error)?)
            .fetch_all(&self.0)
            .await
            .map_err(error)?;
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
fn encode_operation(operation: &Operation) -> Result<String> {
    if operation.id.is_nil()
        || operation
            .session_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 256)
    {
        return Err("invalid client operation identity".into());
    }
    maka_plugin_core::json::check_delivery(&operation.correlation, 32 * 1024)?;
    serde_json::to_string(operation).map_err(error)
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

pub(crate) fn validate_upload(operation: &Operation, key: &DraftKey) -> Result<()> {
    if operation.draft.is_some()
        || operation.endpoint != "agent"
        || operation.method != "uploadAttachment"
        || operation.session_id.as_deref() != Some(&key.session_id)
        || operation.upload.as_ref().map(|upload| &upload.key) != Some(key)
        || operation.host.state_domain != key.state_domain
        || operation.host.deployment != key.deployment
        || operation
            .reference
            .as_ref()
            .is_some_and(|reference| !key.matches(reference))
    {
        return Err("original attachment upload belongs to another draft owner".into());
    }
    Ok(())
}

pub(crate) fn uploaded_attachment(operation: &Operation) -> Result<Option<DraftAttachment>> {
    if operation.abandoned {
        return Ok(None);
    }
    let Some(Receipt::Recorded {
        outcome: OperationOutcome::Returned { value },
        ..
    }) = &operation.receipt
    else {
        return Err("original attachment upload is unresolved; recover its receipt".into());
    };
    if value["kind"] == "rejected" {
        return Ok(None);
    }
    let upload = operation.upload.as_ref().ok_or("missing upload owner")?;
    let value = value
        .get("attachment")
        .filter(|_| value["kind"] == "uploaded")
        .ok_or("unexpected attachment upload result")?;
    if value["sessionId"] != upload.key.session_id {
        return Err("attachment upload returned another session".into());
    }
    let attachment: DraftAttachment = serde_json::from_value(serde_json::json!({
        "id":value["id"],"name":value["name"],"mimeType":value["mimeType"],"bytes":value["bytes"]
    }))
    .map_err(error)?;
    if attachment.id.is_empty() || attachment.bytes != upload.bytes as u64 {
        return Err("attachment upload receipt differs from the original file".into());
    }
    Ok(Some(attachment))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_open_migrates_the_original_text_row_without_losing_its_revision() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("old-client.db");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        sqlx::query("CREATE TABLE drafts (owner TEXT PRIMARY KEY, revision INTEGER NOT NULL, text TEXT NOT NULL, consumed INTEGER NOT NULL DEFAULT 0)").execute(&pool).await.unwrap();
        let key = DraftKey {
            state_domain: "original-host".into(),
            deployment: Uuid::from_u128(1),
            namespace: InstanceId(vec![Uuid::from_u128(2)]),
            export: "Agent".into(),
            session_id: "original-session".into(),
            editor_id: "composer".into(),
        };
        sqlx::query("INSERT INTO drafts(owner,revision,text) VALUES (?,7,'original unsent text')")
            .bind(serde_json::to_string(&key).unwrap())
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let (first, second) = tokio::join!(Journal::open(&path), Journal::open(&path));
        let (first, second) = (first.unwrap(), second.unwrap());
        let loaded = first.load_draft(&key).await.unwrap().unwrap();
        assert_eq!(
            (
                loaded.revision,
                loaded.text.as_str(),
                loaded.attachments.len()
            ),
            (7, "original unsent text", 0)
        );
        first
            .save_draft(&key, Some(7), "edited after migration".to_owned().into())
            .await
            .unwrap();
        assert_eq!(
            second.load_draft(&key).await.unwrap().unwrap().text,
            "edited after migration"
        );
        assert!(
            second
                .save_draft(&key, Some(7), "stale old scope".to_owned().into())
                .await
                .is_err()
        );
    }
}
