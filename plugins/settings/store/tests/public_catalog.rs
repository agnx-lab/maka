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

use maka_agent_facts::{configuration::*, public_catalog::PublicCatalogBody};
use maka_config::{
    ConfigurationStore,
    model_fetch::ModelFetchPreparation,
    public_catalog::{PublicCatalogRecord, PublicCatalogSnapshot, PublicCatalogSource},
};
use maka_event_log::root::{RootNamespaces, RootOwner};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

fn root(path: &Path) -> Arc<RootOwner> {
    Arc::new(
        RootOwner::create(
            &path.join("root"),
            &RootNamespaces {
                ownership: path.join("owners"),
                control: path.join("control"),
            },
        )
        .unwrap(),
    )
}

fn record(body: Value, checked_at: i64) -> PublicCatalogRecord {
    PublicCatalogRecord {
        body: PublicCatalogBody::from_json(body.to_string()).unwrap(),
        etag: Some("\"original\"".into()),
        last_modified: None,
        checked_at,
    }
}

fn assert_snapshot_eq(actual: &PublicCatalogSnapshot, expected: &PublicCatalogSnapshot) {
    assert_eq!(actual.catalog_revision, expected.catalog_revision);
    assert_eq!(
        actual.sources.keys().collect::<Vec<_>>(),
        expected.sources.keys().collect::<Vec<_>>()
    );
    for (source, record) in &actual.sources {
        assert!(record.same_content(&expected.sources[source]).unwrap());
    }
}

#[tokio::test]
async fn sources_revalidate_independently_and_survive_restart_without_changing_user_state() {
    let temp = tempfile::tempdir().unwrap();
    let owner = root(temp.path());
    let store = Arc::new(ConfigurationStore::for_root(owner.clone()).await.unwrap());
    let empty = store.public_catalog().await.unwrap();
    assert_eq!(empty.catalog_revision, 0);
    assert!(empty.sources.is_empty());

    let create: CreateCatalogConnectionInput = serde_json::from_value(json!({
        "expectedCatalogRevision":0,
        "connection":{"slug":"native","name":"Native",
            "provider":{"packageId":"example","entryId":"api","scope":"profile","name":"api"},
            "configuration":{"baseUrl":"http://localhost:18080/v1"},"enabled":true,
            "enabledModelIds":["selected","manual"],
            "modelOverrides":{"selected":{"vision":false},"manual":{}},
            "requestBodyOverlay":{"temperature":0.5}}
    }))
    .unwrap();
    let CatalogMutationResult::Committed {
        connection: Some(basis),
        ..
    } = store.create_connection(create).await.unwrap()
    else {
        panic!("expected connection");
    };
    let ModelFetchPreparation::Ready(fetch) = store
        .prepare_model_fetch(&basis.connection_id)
        .await
        .unwrap()
    else {
        panic!("expected discovery preparation");
    };
    assert!(matches!(
        fetch
            .complete(vec![ModelInfo::new("selected")], 42)
            .await
            .unwrap(),
        ConnectionModelFetchResult::Committed { .. }
    ));
    store
        .set_default_target(SetDefaultConnectionTargetInput {
            expected_catalog_revision: 2,
            target: Some(ConnectionTarget {
                connection_id: basis.connection_id,
                model_id: "selected".into(),
            }),
        })
        .await
        .unwrap();
    let locator = CredentialLocator::NetworkProxy {
        kind: PasswordKind::Password,
    };
    assert!(matches!(
        store
            .set_credential(
                SetCredentialInput {
                    locator: locator.clone(),
                    expected: None,
                    expected_connection: None,
                    secret: "private fixture credential".into(),
                },
                43,
            )
            .await
            .unwrap(),
        CredentialMutationResult::Committed { .. }
    ));
    let original = store.catalog().await.unwrap();
    let credential = store.credential_status(locator.clone()).await.unwrap();
    let first = record(
        json!({"native":{"name":"Native","models":{"selected":{"name":"Initial"}}}}),
        100,
    );
    assert_eq!(
        store
            .put_public_catalog(PublicCatalogSource::ModelsDev, first.clone())
            .await
            .unwrap(),
        original.revision + 1
    );
    let router = record(
        json!({"data":[{"id":"vendor/selected","name":"Gateway"}]}),
        101,
    );
    assert_eq!(
        store
            .put_public_catalog(PublicCatalogSource::OpenRouter, router.clone())
            .await
            .unwrap(),
        original.revision + 2
    );
    // JSON whitespace/key order and conditional-response headers are not model changes.
    let revalidated = PublicCatalogRecord {
        body: PublicCatalogBody::from_json(
            "{ \"native\" : { \"models\" : { \"selected\" : { \"name\" : \"Initial\" } }, \"name\" : \"Native\" } }".into(),
        )
        .unwrap(),
        etag: None,
        last_modified: Some("Thu, 22 Oct 2026 00:00:00 GMT".into()),
        checked_at: 200,
    };
    assert_eq!(
        store
            .put_public_catalog(PublicCatalogSource::ModelsDev, revalidated.clone())
            .await
            .unwrap(),
        original.revision + 2
    );
    let mut changed = revalidated;
    let mut changed_body: Value = serde_json::from_str(changed.body.as_raw().get()).unwrap();
    changed_body["native"]["models"]["selected"]["name"] = json!("Current");
    changed.body = PublicCatalogBody::from_json(changed_body.to_string()).unwrap();
    changed.checked_at = 300;
    assert_eq!(
        store
            .put_public_catalog(PublicCatalogSource::ModelsDev, changed.clone())
            .await
            .unwrap(),
        original.revision + 3
    );
    let snapshot = store.public_catalog().await.unwrap();
    assert_eq!(snapshot.catalog_revision, original.revision + 3);
    assert_eq!(snapshot.sources.len(), 2);
    assert!(
        snapshot.sources[&PublicCatalogSource::ModelsDev]
            .same_content(&changed)
            .unwrap()
    );
    assert!(
        snapshot.sources[&PublicCatalogSource::OpenRouter]
            .same_content(&router)
            .unwrap()
    );
    let current = store.catalog().await.unwrap();
    assert_eq!(current.revision, snapshot.catalog_revision);
    assert_eq!(current.connections, original.connections);
    assert_eq!(current.default_target, original.default_target);
    assert_eq!(
        store.credential_status(locator.clone()).await.unwrap(),
        credential
    );
    assert_eq!(
        store.credential_secret(&locator, None).await.unwrap(),
        Some("private fixture credential".into())
    );
    store.shutdown().await.unwrap();
    drop(store);
    drop(owner);

    let reopened = ConfigurationStore::for_root(Arc::new(
        RootOwner::open(
            &temp.path().join("root"),
            &RootNamespaces {
                ownership: temp.path().join("owners"),
                control: temp.path().join("control"),
            },
        )
        .unwrap(),
    ))
    .await
    .unwrap();
    assert_snapshot_eq(&reopened.public_catalog().await.unwrap(), &snapshot);
    assert_eq!(reopened.catalog().await.unwrap(), current);
    assert_eq!(
        reopened.credential_status(locator).await.unwrap(),
        credential
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn failed_publication_rolls_back_the_body_and_http_metadata_with_its_revision() {
    let temp = tempfile::tempdir().unwrap();
    let owner = root(temp.path());
    let store = ConfigurationStore::for_root(owner.clone()).await.unwrap();
    let first = record(json!({"models":{"original":{}}}), 100);
    store
        .put_public_catalog(PublicCatalogSource::ModelsDev, first.clone())
        .await
        .unwrap();
    let original = store.public_catalog().await.unwrap();
    let database =
        rusqlite::Connection::open(owner.canonical_path().join("configuration-rust.sqlite"))
            .unwrap();
    assert!(
        database
            .execute("UPDATE public_catalog SET body='invalid json'", [])
            .is_err()
    );
    database
        .execute_batch(
            "CREATE TRIGGER reject_publication BEFORE UPDATE ON connection_catalog
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
        )
        .unwrap();
    assert!(
        store
            .put_public_catalog(
                PublicCatalogSource::ModelsDev,
                record(json!({"models":{"replacement":{}}}), 200),
            )
            .await
            .is_err()
    );
    assert_snapshot_eq(&store.public_catalog().await.unwrap(), &original);
    assert_eq!(store.catalog().await.unwrap().revision, 1);
    let revalidated = PublicCatalogRecord {
        checked_at: 300,
        etag: Some("\"current\"".into()),
        ..first
    };
    assert_eq!(
        store
            .put_public_catalog(PublicCatalogSource::ModelsDev, revalidated.clone())
            .await
            .unwrap(),
        1
    );
    assert!(
        store.public_catalog().await.unwrap().sources[&PublicCatalogSource::ModelsDev]
            .same_content(&revalidated)
            .unwrap()
    );
    drop(database);
    store.close().await.unwrap();
}

#[tokio::test]
async fn revoked_root_cannot_read_or_publish_cached_catalogs() {
    let temp = tempfile::tempdir().unwrap();
    let owner = root(temp.path());
    let store = ConfigurationStore::for_root(owner.clone()).await.unwrap();
    store
        .put_public_catalog(
            PublicCatalogSource::OpenRouter,
            record(json!({"data":[{"id":"original"}]}), 100),
        )
        .await
        .unwrap();
    std::fs::remove_dir_all(temp.path().join("control")).unwrap();
    assert!(owner.validate_current().is_err());
    assert!(store.public_catalog().await.is_err());
    assert!(
        store
            .put_public_catalog(
                PublicCatalogSource::OpenRouter,
                record(json!({"data":[{"id":"must-not-publish"}]}), 200),
            )
            .await
            .is_err()
    );
    store.close().await.unwrap();
    let database =
        rusqlite::Connection::open(owner.canonical_path().join("configuration-rust.sqlite"))
            .unwrap();
    let (body, revision): (String, i64) = database
        .query_row(
            "SELECT body, revision FROM public_catalog, connection_catalog",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"data":[{"id":"original"}]})
    );
    assert_eq!(revision, 1);
}

#[test]
fn migration_retires_only_observed_models_and_preserves_typed_connection_and_secret_authorities() {
    let database = rusqlite::Connection::open_in_memory().unwrap();
    for migration in [
        include_str!("../migrations/0001_configuration.sql"),
        include_str!("../migrations/0002_shell_titles.sql"),
        include_str!("../migrations/0003_complete_configuration.sql"),
        include_str!("../migrations/0004_remove_assistant_tone.sql"),
    ] {
        database.execute_batch(migration).unwrap();
    }
    let mut expected: ConnectionCatalogEntry = serde_json::from_value(json!({
        "connectionId":"11111111-1111-4111-8111-111111111111","revision":7,
        "slug":"native","name":"Native",
        "provider":{"packageId":"example","entryId":"api","scope":"profile","name":"api"},
        "configuration":{"baseUrl":"http://localhost:18080/v1"},"enabled":true,
        "enabledModelIds":["selected","manual"],
        "modelOverrides":{"selected":{"vision":false},"manual":{}},
        "requestBodyOverlay":{"temperature":0.5},
        "models":[{"id":"selected","contextWindow":8192}],
        "modelSource":"fetched","modelsFetchedAt":123
    }))
    .unwrap();
    database
        .execute(
            "INSERT INTO connections VALUES(?1,?2,?3,?4)",
            rusqlite::params![
                expected.connection_id,
                expected.slug,
                i64::try_from(expected.revision).unwrap(),
                serde_json::to_string(&expected).unwrap()
            ],
        )
        .unwrap();
    let target = json!({"connectionId":expected.connection_id,"modelId":"selected"}).to_string();
    database
        .execute(
            "UPDATE connection_catalog SET revision=11, default_target=?",
            [&target],
        )
        .unwrap();
    database
        .execute_batch(
            "UPDATE credential_vault SET revision=9;
             INSERT INTO credentials VALUES('fixture-locator','fixture-credential',3,'private fixture secret',123);",
        )
        .unwrap();
    database
        .execute_batch(include_str!("../migrations/0005_public_catalog.sql"))
        .unwrap();
    expected.models.clear();
    expected.model_source = None;
    expected.models_fetched_at = None;
    let (column_revision, document): (i64, String) = database
        .query_row("SELECT revision,document FROM connections", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    let actual: ConnectionCatalogEntry = serde_json::from_str(&document).unwrap();
    validation::catalog_entry(&actual).unwrap();
    assert_eq!(column_revision, 7);
    assert_eq!(actual, expected);
    let raw: Value = serde_json::from_str(&document).unwrap();
    assert!(raw.get("modelSource").is_none());
    assert!(raw.get("modelsFetchedAt").is_none());
    assert_eq!(
        database
            .query_row(
                "SELECT revision,default_target FROM connection_catalog",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            )
            .unwrap(),
        (12, target)
    );
    assert_eq!(
        database
            .query_row(
                "SELECT credential_id,revision,secret,updated_at FROM credentials",
                [],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?
                ))
            )
            .unwrap(),
        (
            "fixture-credential".into(),
            3,
            "private fixture secret".into(),
            123
        )
    );
    assert_eq!(
        database
            .query_row("SELECT revision FROM credential_vault", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        9
    );
}

#[test]
fn uncertain_record_comparison_includes_all_http_metadata_and_check_time() {
    let original = record(json!({"unknown":1}), 10);
    let mut candidate = original.clone();
    candidate.body =
        PublicCatalogBody::from_json(r#"{ "unknown": 0, "\u0075nknown": 1 }"#.into()).unwrap();
    assert!(original.same_content(&candidate).unwrap());
    candidate.etag = None;
    assert!(!original.same_content(&candidate).unwrap());
    candidate = original.clone();
    candidate.last_modified = Some("changed".into());
    assert!(!original.same_content(&candidate).unwrap());
    candidate = original.clone();
    candidate.checked_at += 1;
    assert!(!original.same_content(&candidate).unwrap());
    candidate = original.clone();
    candidate.body = PublicCatalogBody::from_json(r#"{"unknown":2}"#.into()).unwrap();
    assert!(!original.same_content(&candidate).unwrap());
}

#[tokio::test]
async fn raw_body_semantics_preserve_revisions_unknown_fields_and_original_json_value() {
    let temp = tempfile::tempdir().unwrap();
    let owner = root(temp.path());
    let store = ConfigurationStore::for_root(owner.clone()).await.unwrap();
    let raw = r#"{"unknown":1,"data":null,"data":[],"future":{"b":2,"a":1}}"#;
    let mut row = record(json!(null), 10);
    row.body = PublicCatalogBody::from_json(raw.into()).unwrap();
    assert_eq!(
        store
            .put_public_catalog(PublicCatalogSource::OpenRouter, row.clone())
            .await
            .unwrap(),
        1
    );
    let db = rusqlite::Connection::open(owner.canonical_path().join("configuration-rust.sqlite"))
        .unwrap();
    let stored: String = db
        .query_row("SELECT body FROM public_catalog", [], |row| row.get(0))
        .unwrap();
    assert_eq!(stored, raw);
    row.body =
        PublicCatalogBody::from_json(r#"{"future":{"a":1,"b":2},"data":[],"unknown":1}"#.into())
            .unwrap();
    row.checked_at = 20;
    assert_eq!(
        store
            .put_public_catalog(PublicCatalogSource::OpenRouter, row.clone())
            .await
            .unwrap(),
        1
    );
    row.body =
        PublicCatalogBody::from_json(r#"{"future":{"a":1,"b":2},"data":[],"unknown":1.0}"#.into())
            .unwrap();
    assert_eq!(
        store
            .put_public_catalog(PublicCatalogSource::OpenRouter, row.clone())
            .await
            .unwrap(),
        2
    );
    row.body =
        PublicCatalogBody::from_json(r#"{"future":{"a":1,"b":2},"data":[],"unknown":2}"#.into())
            .unwrap();
    assert_eq!(
        store
            .put_public_catalog(PublicCatalogSource::OpenRouter, row.clone())
            .await
            .unwrap(),
        3
    );
    // SQLite json_valid can accept numeric overflow even though Value cannot.
    db.execute(
        "UPDATE public_catalog SET body = ?",
        [r#"{"unknown":1e400,"unknown":null}"#],
    )
    .unwrap();
    assert!(store.public_catalog().await.is_err());
    assert!(
        store
            .put_public_catalog(PublicCatalogSource::OpenRouter, row)
            .await
            .is_err()
    );
    assert_eq!(store.catalog().await.unwrap().revision, 3);
    drop(db);
    store.close().await.unwrap();
}
