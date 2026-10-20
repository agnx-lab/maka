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

//! Exercise an actual migration-35 database, including retained image evidence.
use std::{path::Path, sync::Arc};

use crate::{
    EventLog,
    connection::{ConnectionAuthority, OwnedConnection},
};
use maka_agent_facts::{
    artifact::{Artifact, ArtifactKind, ArtifactSource, content_digest},
    event::{EventWrite, Fact, Invocation, InvocationInput, InvocationOutcome, RuntimeEvent},
    session::CopyPurpose,
    tool_call::ToolCallIdentity,
    tool_output::ToolSuccess,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use sqlx::sqlite::SqliteConnectOptions;

// Run the real historical migrations, then use the real current writer APIs.
// No migration ledger entry or checksum is removed to manufacture an old schema.
async fn open_at_35(path: &Path) -> EventLog {
    let lease =
        Arc::new(crate::root::FileLease::acquire(&path.with_extension("writer.lock")).unwrap());
    let connection = OwnedConnection::open(
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
        ConnectionAuthority::Writer { lease, root: None },
        |connection| {
            Box::pin(async move {
                sqlx::raw_sql(
                    "PRAGMA application_id = 1296124754; PRAGMA auto_vacuum = INCREMENTAL;",
                )
                .execute(&mut *connection)
                .await?;
                crate::sqlite_functions::register(connection).await?;
                super::terminal_display::register(connection, true).await?;
                super::MIGRATIONS
                    .run_direct(Some(35), &mut *connection, false)
                    .await?;
                super::terminal_display::register(connection, false).await?;
                sqlx::raw_sql("PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;")
                    .execute(&mut *connection)
                    .await?;
                crate::message_sources::initialize(connection).await?;
                crate::sessions::initialize_execution(connection).await?;
                crate::transcript::directory::initialize(connection).await
            })
        },
    )
    .await
    .unwrap();
    let context_cache = Arc::new(crate::context::cache::Cache::default());
    let install = context_cache.clone();
    connection
        .run(move |connection| Box::pin(async move { install.install(connection).await }))
        .await
        .unwrap();
    EventLog {
        context_cache,
        connection,
        root_owner: None,
        commits: tokio::sync::watch::channel(0).0,
        shell_changes: tokio::sync::broadcast::channel(256).0,
    }
}

fn invocation() -> Invocation {
    Invocation {
        session_id: "original".into(),
        turn_id: "turn".into(),
        run_id: "run".into(),
        invocation_id: "invocation".into(),
    }
}
fn event(fact: Fact) -> EventWrite {
    EventWrite::plain(RuntimeEvent::new(invocation(), fact)).unwrap()
}

async fn seed_canonical_image(log: &EventLog) -> Artifact {
    log.create_session("original", "create", &json!({"name": "kept"}), 10)
        .await
        .unwrap();
    log.append(&event(Fact::InvocationOpened {
        configuration: None,
        input: InvocationInput::Message {
            source_messages: Vec::new(),
            content: "preserve image".into(),
            request_fingerprint: None,
        },
    }))
    .await
    .unwrap();
    log.append(&event(Fact::ToolDispatched {
        presentation: None,
        title: None,
        operation_id: "image".into(),
        call: ToolCallIdentity::standalone("image-call".into()),
        name: "Read".into(),
        input: json!({}),
    }))
    .await
    .unwrap();
    let image = vec![
        71, 73, 70, 56, 57, 97, 1, 0, 1, 0, 128, 0, 0, 0, 0, 0, 255, 255, 255, 33, 249, 4, 1, 0, 0,
        0, 0, 44, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 1, 68, 0, 59,
    ];
    let (result, _) = EventWrite::tool_success(
        "image-result".into(),
        std::time::SystemTime::now(),
        invocation(),
        "image".into(),
        ToolSuccess::image(image, "image/gif".into()).unwrap(),
    )
    .unwrap();
    let artifact = result.projection_artifacts()[0].artifact().clone();
    log.append(&result).await.unwrap();
    log.append(&event(Fact::InvocationEnded {
        outcome: InvocationOutcome::Completed,
    }))
    .await
    .unwrap();
    let source = log.get_session::<Value>("original").await.unwrap().unwrap();
    assert!(matches!(
        log.copy_session(
            crate::sessions::SessionCopy {
                source_session_id: "original".into(),
                target_session_id: "copied".into(),
                expected_source_revision: source.revision,
                purpose: CopyPurpose::Branch {
                    turn_id: None,
                    side_conversation: false
                },
            },
            &json!({"name": "copy"}),
            20
        )
        .await
        .unwrap(),
        crate::sessions::SessionCopyResult::Committed(_)
    ));
    artifact
}

type StoredArtifact = (String, String, i64, String, String, Vec<u8>, Option<i64>);
fn artifacts(db: &Connection) -> Vec<StoredArtifact> {
    db.prepare("SELECT session_id, id, created_at, record_json, content_sha256, payload, canonical_at FROM artifacts ORDER BY session_id, id")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)))
        .unwrap().collect::<Result<_, _>>().unwrap()
}
fn mappings(db: &Connection) -> Vec<(String, String, String, String)> {
    db.prepare("SELECT session_id, source_session_id, source_artifact_id, artifact_id FROM session_history_artifacts ORDER BY session_id, source_session_id, source_artifact_id")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        .unwrap().collect::<Result<_, _>>().unwrap()
}
fn catalogs(db: &Connection) -> Vec<(String, i64)> {
    db.prepare("SELECT session_id, revision FROM artifact_catalog ORDER BY session_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[tokio::test]
async fn migration_36_preserves_canonical_images_and_copies_and_accepts_large_tool_originals() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.sqlite");
    let log = open_at_35(&path).await;
    let image = seed_canonical_image(&log).await;
    let upload_bytes = b"uncaptured upload";
    log.commit_artifact(
        Artifact {
            id: "uncaptured".into(),
            session_id: "original".into(),
            turn_id: "upload".into(),
            created_at: 21,
            name: "notes.txt".into(),
            kind: ArtifactKind::File,
            size_bytes: upload_bytes.len() as u64,
            mime_type: Some("text/plain".into()),
            source: ArtifactSource::UserUpload,
            summary: Some(content_digest(upload_bytes)),
        },
        upload_bytes,
    )
    .await
    .unwrap();
    let facts = serde_json::to_vec(&log.prefix(32, 64 * 1024).await.unwrap()).unwrap();
    let original = log.list_artifacts("original", 0, 128).await.unwrap();
    let copied = log.list_artifacts("copied", 0, 128).await.unwrap();
    log.close().await.unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT max(version) FROM _sqlx_migrations", [], |row| row
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        35
    );
    let prior = artifacts(&db);
    assert_eq!(prior.len(), 3);
    assert_eq!(
        prior
            .iter()
            .filter(|row| row.6.is_some_and(|at| at > 0))
            .count(),
        2
    );
    let uncaptured: Vec<_> = prior.iter().filter(|row| row.6.is_none()).collect();
    assert_eq!(uncaptured.len(), 1);
    assert_eq!(uncaptured[0].1, "uncaptured");
    let prior_mappings = mappings(&db);
    assert_eq!(prior_mappings.len(), 1);
    assert_eq!(prior_mappings[0].0, "copied");
    assert_eq!(prior_mappings[0].1, "original");
    assert_eq!(prior_mappings[0].2, image.id);
    let prior_catalogs = catalogs(&db);
    drop(db);

    let log = EventLog::open(&path).await.unwrap();
    assert_eq!(
        serde_json::to_vec(&log.prefix(32, 64 * 1024).await.unwrap()).unwrap(),
        facts
    );
    for (session, expected) in [("original", original), ("copied", copied)] {
        let actual = log.list_artifacts(session, 0, 128).await.unwrap();
        assert_eq!(actual.revision, expected.revision);
        assert_eq!(actual.total, expected.total);
        assert_eq!(actual.records, expected.records);
    }
    let db = Connection::open(&path).unwrap();
    assert_eq!(artifacts(&db), prior);
    assert_eq!(mappings(&db), prior_mappings);
    assert_eq!(catalogs(&db), prior_catalogs);
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    drop(db);

    let bytes = vec![0x5a; 50 * 1024 * 1024 + 1];
    let record = Artifact {
        id: "large-tool-original".into(),
        session_id: "original".into(),
        turn_id: "turn".into(),
        created_at: 30,
        name: "original.png".into(),
        kind: ArtifactKind::Image,
        size_bytes: bytes.len() as u64,
        mime_type: Some("image/png".into()),
        source: ArtifactSource::ToolResult,
        summary: Some(content_digest(&bytes)),
    };
    assert_eq!(
        log.commit_artifact(record.clone(), bytes).await.unwrap(),
        record
    );
    assert_eq!(
        log.read_artifact_chunk("original", &record.id, 50 * 1024 * 1024, 1)
            .await
            .unwrap()
            .unwrap()
            .bytes,
        [0x5a]
    );
    log.close().await.unwrap();
    let db = Connection::open(&path).unwrap();
    let stored: (i64, String) = db.query_row("SELECT length(payload), content_sha256 FROM artifacts WHERE id = 'large-tool-original'", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(stored.0, 50 * 1024 * 1024 + 1);
    assert_eq!(Some(stored.1), record.summary);
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
}
