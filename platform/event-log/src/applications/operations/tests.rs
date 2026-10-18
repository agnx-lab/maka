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
use crate::applications::tests::{input, store};
use maka_agent_facts::language::Language;
use maka_plugin_core::contract::{Interface, Method, MethodMode, Transport};
use std::collections::BTreeMap;

async fn request(store: &EventLog) -> OperationRequest {
    OperationRequest {
        source: store
            .commit_application(Uuid::new_v4(), 0, input(), vec![], vec![])
            .await
            .unwrap(),
        namespace: InstanceId(vec![Uuid::new_v4()]),
        consumer: InstanceId(vec![Uuid::new_v4()]),
        export: "service".into(),
        method: "change".into(),
        operation: Uuid::new_v4(),
        contract: Contract {
            name: "changes".into(),
            version: 1,
            transport: Transport::Local,
            interface: Interface::Service {
                methods: BTreeMap::from([(
                    "change".into(),
                    Method {
                        progress: None,
                        mode: MethodMode::Mutation,
                        input: serde_json::json!({}),
                        output: serde_json::json!({}),
                        resources: BTreeMap::new(),
                    },
                )]),
            },
        },
        input: serde_json::json!({"outer":{"z":1,"a":2},"text":"original"}),
        capture: None,
        origin: Identity::Remote {
            request_id: Uuid::new_v4(),
        },
    }
}

#[tokio::test]
async fn compact_receipts_preserve_original_identity_and_reject_inline_contracts() {
    let (_dir, store) = store().await;
    let mut request = request(&store).await;
    let Interface::Service { methods } = &mut request.contract.interface else {
        unreachable!()
    };
    methods.get_mut("change").unwrap().input =
        serde_json::json!({"description": "x".repeat(250_000)});
    let compact = request.reference().unwrap();
    assert!(serde_json::to_vec(&compact).unwrap().len() < 1024);
    let mut inline = serde_json::to_value(&compact).unwrap();
    inline["contract"] = serde_json::to_value(&request.contract).unwrap();
    assert!(serde_json::from_value::<RecoveryRef>(inline).is_err());
    let Dispatch::New(claim) = store.dispatch_operation(request.clone()).await.unwrap() else {
        panic!("first claim")
    };
    let outcome = OperationOutcome::Returned {
        value: serde_json::json!({"original": true}),
    };
    store
        .finish_operation(&claim, outcome.clone(), OperationCleanup::Confirmed)
        .await
        .unwrap();
    let receipt = store
        .operation_observation(&compact)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.reference, compact);
    assert_eq!(receipt.outcome, outcome);
    let mut second = request.clone();
    second.operation = Uuid::new_v4();
    store.dispatch_operation(second).await.unwrap();
    let (contracts, stored_contract, max_request): (i64, String, i64) = store
        .connection
        .run(|connection| {
            Box::pin(async move {
                let contracts =
                    sqlx::query_scalar("SELECT count(*) FROM application_operation_contracts")
                        .fetch_one(&mut *connection)
                        .await?;
                let contract =
                    sqlx::query_scalar("SELECT contract_json FROM application_operation_contracts")
                        .fetch_one(&mut *connection)
                        .await?;
                let size = sqlx::query_scalar(
                    "SELECT max(length(CAST(request_json AS BLOB))) FROM application_operations",
                )
                .fetch_one(connection)
                .await?;
                Ok((contracts, contract, size))
            })
        })
        .await
        .unwrap();
    assert_eq!(contracts, 1, "shared original schemas are stored once");
    assert_eq!(
        serde_json::from_str::<Contract>(&stored_contract).unwrap(),
        request.contract
    );
    assert!(
        max_request < 1024,
        "individual operations contain only contract identity"
    );
    let mut forged = compact.clone();
    forged.contract.digest = format!("sha256-{}", "a".repeat(64));
    assert!(matches!(
        store.operation_observation(&forged).await,
        Err(StoreError::EventConflict)
    ));
    assert!(
        compact
            .contract
            .matches(&request.contract, "change")
            .unwrap()
    );
    let Interface::Service { methods } = &mut request.contract.interface else {
        unreachable!()
    };
    methods.insert("another".into(), methods["change"].clone());
    assert!(
        !compact
            .contract
            .matches(&request.contract, "change")
            .unwrap()
    );
    let Interface::Service { methods } = &mut request.contract.interface else {
        unreachable!()
    };
    methods.get_mut("change").unwrap().output = serde_json::json!({"type": "string"});
    assert!(
        !compact
            .contract
            .matches(&request.contract, "change")
            .unwrap()
    );
    store.close().await.unwrap();
}
#[tokio::test]
async fn concurrent_dispatch_has_one_claim_and_duplicate_preserves_original_cleanup() {
    let (_dir, store) = store().await;
    let request = request(&store).await;
    let reference = request.reference().unwrap();
    let (a, b) = tokio::join!(
        store.dispatch_operation(request.clone()),
        store.dispatch_operation(request.clone())
    );
    let claim = match (a.unwrap(), b.unwrap()) {
        (Dispatch::New(claim), Dispatch::Existing(observation))
        | (Dispatch::Existing(observation), Dispatch::New(claim)) => {
            assert_eq!(observation.outcome, OperationOutcome::Unresolved);
            claim
        }
        _ => panic!("exactly one dispatch may execute"),
    };
    let outcome = OperationOutcome::Returned {
        value: serde_json::json!({"changed":true}),
    };
    let cleanup = OperationCleanup::Failed {
        diagnostic: "actual resource remains open".into(),
    };
    let settled = store
        .finish_operation(&claim, outcome.clone(), cleanup.clone())
        .await
        .unwrap();
    assert_eq!(
        store
            .finish_operation(&claim, outcome, cleanup)
            .await
            .unwrap(),
        settled
    );
    assert!(matches!(
        store
            .finish_operation(
                &claim,
                OperationOutcome::Unknown {
                    diagnostic: "rewrite".into()
                },
                OperationCleanup::Confirmed
            )
            .await,
        Err(StoreError::EventConflict)
    ));
    let Dispatch::Existing(duplicate) = store.dispatch_operation(request).await.unwrap() else {
        panic!("no second claim")
    };
    assert_eq!(duplicate, settled);
    assert_eq!(
        store
            .operation_observation(&reference)
            .await
            .unwrap()
            .unwrap(),
        settled
    );
    store.close().await.unwrap();
}
#[tokio::test]
async fn fingerprint_ignores_object_order_and_new_scope_but_preserves_original_request() {
    let (_dir, store) = store().await;
    let original = request(&store).await;
    let reference = original.reference().unwrap();
    let mut reordered = original.clone();
    reordered.origin = Identity::Remote {
        request_id: Uuid::new_v4(),
    };
    reordered.input = serde_json::from_str(r#"{"text":"original","outer":{"a":2,"z":1}}"#).unwrap();
    assert_eq!(reordered.reference().unwrap(), reference);
    assert!(matches!(
        store.dispatch_operation(original.clone()).await.unwrap(),
        Dispatch::New(_)
    ));
    assert!(matches!(
        store.dispatch_operation(reordered).await.unwrap(),
        Dispatch::Existing(_)
    ));
    for change in 0..5 {
        let mut changed = original.clone();
        match change {
            0 => changed.input = serde_json::json!("changed"),
            1 => changed.consumer = InstanceId(vec![Uuid::new_v4()]),
            2 => changed.export = "other".into(),
            3 => changed.contract.version += 1,
            _ => {
                changed.source = store
                    .commit_application(original.source.deployment, 1, input(), vec![], vec![])
                    .await
                    .unwrap();
            }
        }
        assert!(matches!(
            store.dispatch_operation(changed).await,
            Err(StoreError::EventConflict)
        ));
    }
    store.close().await.unwrap();
}
#[tokio::test]
async fn fixed_original_receipt_survives_replacement_and_reopen_without_execution_claim() {
    let (dir, store) = store().await;
    let request = request(&store).await;
    let reference = request.reference().unwrap();
    let Dispatch::New(claim) = store.dispatch_operation(request).await.unwrap() else {
        panic!()
    };
    store
        .commit_application(reference.source.deployment, 1, input(), vec![], vec![])
        .await
        .unwrap();
    let settled = store
        .finish_operation(
            &claim,
            OperationOutcome::Unknown {
                diagnostic: "callback ended after possible effect".into(),
            },
            OperationCleanup::Confirmed,
        )
        .await
        .unwrap();
    drop(claim);
    store.close().await.unwrap();
    let namespaces = crate::root::RootNamespaces {
        ownership: dir.path().join("owners"),
        control: dir.path().join("control"),
    };
    let owner = Arc::new(RootOwner::open(&dir.path().join("root"), &namespaces).unwrap());
    let reopened = EventLog::for_root(owner).await.unwrap();
    assert_eq!(
        reopened
            .operation_observation(&reference)
            .await
            .unwrap()
            .unwrap(),
        settled
    );
    assert_eq!(
        reopened
            .load_application_at(&reference.source)
            .await
            .unwrap()
            .unwrap()
            .source
            .reference,
        reference.source
    );
    let mut missing = reference.clone();
    missing.operation = Uuid::new_v4();
    assert!(
        reopened
            .operation_observation(&missing)
            .await
            .unwrap()
            .is_none()
    );
    let (_other_dir, other) = crate::applications::tests::store().await;
    assert!(
        other
            .operation_observation(&reference)
            .await
            .unwrap_err()
            .to_string()
            .contains("foreign operation state domain")
    );
    other.close().await.unwrap();
    reopened.close().await.unwrap();
}
#[tokio::test]
async fn dispatch_requires_original_source_and_settlement_requires_original_claim() {
    let (_dir, store) = store().await;
    let original = request(&store).await;
    let mut missing = original.clone();
    missing.source.revision += 1;
    assert!(store.dispatch_operation(missing.clone()).await.is_err());
    assert!(
        store
            .operation_observation(&missing.reference().unwrap())
            .await
            .unwrap()
            .is_none()
    );
    let mut nil = original.clone();
    nil.operation = Uuid::nil();
    assert!(store.dispatch_operation(nil).await.is_err());
    let Dispatch::New(claim) = store.dispatch_operation(original).await.unwrap() else {
        panic!()
    };
    let (_other_dir, other) = crate::applications::tests::store().await;
    assert!(
        other
            .finish_operation(
                &claim,
                OperationOutcome::Returned { value: Value::Null },
                OperationCleanup::Confirmed
            )
            .await
            .is_err()
    );
    assert!(
        store
            .finish_operation(
                &claim,
                OperationOutcome::Unresolved,
                OperationCleanup::Confirmed
            )
            .await
            .is_err()
    );
    assert!(
        store
            .finish_operation(
                &claim,
                OperationOutcome::Unknown {
                    diagnostic: "x".repeat(MAX_DIAGNOSTIC + 1)
                },
                OperationCleanup::Confirmed
            )
            .await
            .is_err()
    );
    other.close().await.unwrap();
    store.close().await.unwrap();
}
#[tokio::test]
async fn stored_receipt_bounds_precede_materialization() {
    for column in ["request_json", "outcome_json", "cleanup_json"] {
        let (_dir, store) = store().await;
        let original = request(&store).await;
        let reference = original.reference().unwrap();
        store.dispatch_operation(original).await.unwrap();
        store.connection.run(move |conn| Box::pin(async move {
            let sql = match column {
                "request_json" => "UPDATE application_operations SET request_json=json_object('huge', printf('%.*c', 4194304, 'x'))",
                "outcome_json" => "UPDATE application_operations SET outcome_json=json_object('huge', printf('%.*c', 4194304, 'x'))",
                _ => "UPDATE application_operations SET cleanup_json=json_object('huge', printf('%.*c', 4194304, 'x'))",
            };
            sqlx::query(sql).execute(conn).await?; Ok(())
        })).await.unwrap();
        assert!(
            store
                .operation_observation(&reference)
                .await
                .unwrap_err()
                .to_string()
                .contains("exceeds limit")
        );
        store.close().await.unwrap();
    }
}

#[tokio::test]
async fn original_resource_capture_is_required_nullable_and_part_of_the_fingerprint() {
    let (_dir, store) = store().await;
    let mut original = request(&store).await;
    let mut json = serde_json::to_value(&original).unwrap();
    json.as_object_mut().unwrap().remove("capture");
    assert!(serde_json::from_value::<OperationRequest>(json).is_err());
    let resource_free = original.reference().unwrap();
    original.capture = Some(Uuid::new_v4());
    assert!(original.reference().is_err());
    let Interface::Service { methods } = &mut original.contract.interface else {
        unreachable!()
    };
    methods.get_mut("change").unwrap().resources.insert(
        "secret".into(),
        maka_plugin_core::contract::ResourceType {
            name: "example.Secret".into(),
            version: 1,
        }
        .required(),
    );
    let captured = original.reference().unwrap();
    assert_ne!(captured.fingerprint, resource_free.fingerprint);
    let first_capture = original.capture;
    original.capture = Some(Uuid::new_v4());
    assert_ne!(
        original.reference().unwrap().fingerprint,
        captured.fingerprint
    );
    original.capture = first_capture;
    assert_eq!(original.reference().unwrap(), captured);
    original.capture = None;
    assert!(original.reference().is_err());
    original.capture = Some(Uuid::nil());
    assert!(original.reference().is_err());
    store.close().await.unwrap();
}

#[tokio::test]
async fn auxiliary_model_requires_exact_pending_application_operation_and_cannot_replay() {
    use crate::usage::AuxiliarySource;
    use maka_agent_facts::execution::ModelBinding;
    let (_dir, store) = store().await;
    let request = request(&store).await;
    let reference = request.reference().unwrap();
    store
        .create_session("recap", "create", &serde_json::json!({}), 1)
        .await
        .unwrap();
    let binding = ModelBinding {
        connection_id: "connection".into(),
        connection_slug: "named".into(),
        model: "chosen".into(),
    };
    assert!(
        store
            .begin_auxiliary_operation_model(
                reference.clone(),
                binding.clone(),
                "recap".into(),
                Language::En,
                None
            )
            .await
            .is_err()
    );
    let Dispatch::New(claim) = store.dispatch_operation(request.clone()).await.unwrap() else {
        panic!("new claim")
    };
    for changed in [
        RecoveryRef {
            operation: Uuid::new_v4(),
            ..reference.clone()
        },
        RecoveryRef {
            namespace: InstanceId(vec![Uuid::new_v4()]),
            ..reference.clone()
        },
        RecoveryRef {
            fingerprint: "a".repeat(64),
            ..reference.clone()
        },
        RecoveryRef {
            source: maka_plugin_core::deployment::DeploymentRef {
                revision: reference.source.revision + 1,
                ..reference.source.clone()
            },
            ..reference.clone()
        },
    ] {
        assert!(
            store
                .begin_auxiliary_operation_model(
                    changed,
                    binding.clone(),
                    "recap".into(),
                    Language::En,
                    None
                )
                .await
                .is_err()
        );
    }
    assert!(
        store
            .begin_auxiliary_model(
                AuxiliarySource::ApplicationOperation {
                    source: reference.source.clone(),
                    namespace: reference.namespace.clone(),
                    operation: reference.operation,
                    fingerprint: reference.fingerprint.clone(),
                },
                Language::En,
                None
            )
            .await
            .is_err(),
        "a displayed usage source is not admission evidence"
    );
    let attempt = store
        .begin_auxiliary_operation_model(
            reference.clone(),
            binding.clone(),
            "recap".into(),
            Language::En,
            None,
        )
        .await
        .unwrap();
    assert!(
        store
            .begin_auxiliary_operation_model(
                reference.clone(),
                binding.clone(),
                "recap".into(),
                Language::ZhTw,
                None
            )
            .await
            .is_err()
    );
    store
        .settle_auxiliary_model(attempt, crate::usage::Outcome::Unknown)
        .await
        .unwrap();
    assert!(
        store
            .begin_auxiliary_operation_model(
                reference.clone(),
                binding.clone(),
                "recap".into(),
                Language::En,
                None
            )
            .await
            .is_err(),
        "unknown cost is never safe to replay"
    );
    store
        .finish_operation(
            &claim,
            OperationOutcome::Returned {
                value: serde_json::Value::Null,
            },
            OperationCleanup::Confirmed,
        )
        .await
        .unwrap();
    assert!(
        store
            .begin_auxiliary_operation_model(
                reference,
                binding.clone(),
                "recap".into(),
                Language::En,
                None
            )
            .await
            .is_err()
    );
    let mut completed = request;
    completed.operation = Uuid::new_v4();
    let original = completed.reference().unwrap();
    let Dispatch::New(claim) = store.dispatch_operation(completed).await.unwrap() else {
        panic!("new claim")
    };
    store
        .finish_operation(
            &claim,
            OperationOutcome::Returned {
                value: serde_json::Value::Null,
            },
            OperationCleanup::Confirmed,
        )
        .await
        .unwrap();
    assert!(
        store
            .begin_auxiliary_operation_model(original, binding, "recap".into(), Language::En, None)
            .await
            .is_err(),
        "completed operations cannot start auxiliary work"
    );
}

fn dynamic_request(request: &OperationRequest) -> OperationRequest<()> {
    OperationRequest {
        source: request.source.clone(),
        namespace: request.namespace.clone(),
        consumer: request.consumer.clone(),
        export: request.export.clone(),
        method: request.method.clone(),
        contract: (),
        operation: request.operation,
        input: request.input.clone(),
        capture: request.capture,
        origin: request.origin.clone(),
    }
}

#[tokio::test]
async fn compiled_operation_preserves_reference_capture_budget_and_original_dispatch() {
    let (_dir, store) = store().await;
    let mut original = request(&store).await;
    for required in [false, true] {
        if required {
            let Interface::Service { methods } = &mut original.contract.interface else {
                unreachable!()
            };
            methods.get_mut("change").unwrap().resources.insert(
                "secret".into(),
                maka_plugin_core::contract::ResourceType {
                    name: "example.Secret".into(),
                    version: 1,
                }
                .required(),
            );
        }
        let templates = OperationTemplate::for_contract(&original.contract).unwrap();
        let template = &templates["change"];
        for capture in [None, Some(Uuid::nil()), Some(Uuid::new_v4())] {
            original.capture = capture;
            let old = original.reference();
            let compiled = template.prepare(dynamic_request(&original));
            assert_eq!(old.is_ok(), compiled.is_ok());
            if let (Ok(reference), Ok(prepared)) = (old, compiled) {
                assert_eq!(&reference, prepared.reference());
            }
        }
        let mut wrong_method = dynamic_request(&original);
        wrong_method.method = "missing".into();
        assert!(template.prepare(wrong_method).is_err());
    }
    original.capture = Some(Uuid::new_v4());
    let Interface::Service { methods } = &mut original.contract.interface else {
        unreachable!()
    };
    methods.get_mut("change").unwrap().input =
        serde_json::json!({"description": "x".repeat(100_000)});
    let templates = OperationTemplate::for_contract(&original.contract).unwrap();
    let template = &templates["change"];
    // Compact journal alone fits, but original full contract plus input exceeds its budget.
    original.input = serde_json::json!("x".repeat(MAX_REQUEST - 50_000));
    assert!(original.reference().is_err());
    assert!(template.prepare(dynamic_request(&original)).is_err());
    original.input = serde_json::json!("");
    let overhead = encode(&original, MAX_REQUEST).unwrap().len();
    original.input = serde_json::json!("x".repeat(MAX_REQUEST - overhead));
    assert_eq!(encode(&original, MAX_REQUEST).unwrap().len(), MAX_REQUEST);
    assert_eq!(
        template
            .prepare(dynamic_request(&original))
            .unwrap()
            .reference(),
        &original.reference().unwrap()
    );
    original.input = serde_json::json!("x".repeat(MAX_REQUEST - overhead + 1));
    assert!(original.reference().is_err());
    assert!(template.prepare(dynamic_request(&original)).is_err());
    original.input = serde_json::json!({"original": true});
    let mut invalid_consumer = dynamic_request(&original);
    invalid_consumer.consumer = InstanceId(vec![Uuid::nil()]);
    assert!(template.prepare(invalid_consumer).is_err());
    let prepared = template.prepare(dynamic_request(&original)).unwrap();
    assert_eq!(prepared.reference(), &original.reference().unwrap());
    let Dispatch::New(claim) = store.dispatch_prepared_operation(prepared).await.unwrap() else {
        panic!("first claim")
    };
    let Dispatch::Existing(observation) = store.dispatch_operation(original.clone()).await.unwrap()
    else {
        panic!("original identity must deduplicate")
    };
    assert_eq!(observation.reference, *claim.reference());
    assert_eq!(observation.outcome, OperationOutcome::Unresolved);
    original.input = serde_json::json!({"replacement": true});
    assert!(matches!(
        store
            .dispatch_prepared_operation(template.prepare(dynamic_request(&original)).unwrap())
            .await,
        Err(StoreError::EventConflict)
    ));
    drop(claim);
    store.close().await.unwrap();
}

#[tokio::test]
async fn contract_templates_keep_each_mutations_resources_and_request_identity() {
    let (_directory, store) = store().await;
    let mut original = request(&store).await;
    let Interface::Service { methods } = &mut original.contract.interface else {
        unreachable!()
    };
    let mut upload = methods["change"].clone();
    upload.resources.insert(
        "secret".into(),
        maka_plugin_core::contract::ResourceType {
            name: "example.Secret".into(),
            version: 1,
        }
        .required(),
    );
    methods.insert("upload".into(), upload);
    let mut query = methods["change"].clone();
    query.mode = MethodMode::Query;
    methods.insert("read".into(), query);
    let templates = OperationTemplate::for_contract(&original.contract).unwrap();
    assert_eq!(
        templates.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["change", "upload"]
    );
    for (method, capture_required) in [("change", false), ("upload", true)] {
        original.method = method.into();
        for capture in [None, Some(Uuid::nil()), Some(Uuid::new_v4())] {
            original.capture = capture;
            let expected_valid = if capture_required {
                capture.is_some_and(|id| !id.is_nil())
            } else {
                capture.is_none()
            };
            let legacy = original.reference();
            let prepared = templates[method].prepare(dynamic_request(&original));
            assert_eq!(legacy.is_ok(), expected_valid);
            assert_eq!(prepared.is_ok(), expected_valid);
            if expected_valid {
                let prepared = prepared.unwrap();
                assert_eq!(*prepared.reference(), legacy.unwrap());
                assert_eq!(prepared.journal, original.journal().unwrap());
                assert_eq!(
                    &*prepared.contract_json,
                    encode(&original.contract, MAX_REQUEST).unwrap()
                );
            }
        }
        for wrong in [
            "read",
            "missing",
            if method == "change" {
                "upload"
            } else {
                "change"
            },
        ] {
            let mut request = dynamic_request(&original);
            request.method = wrong.into();
            assert!(templates[method].prepare(request).is_err());
        }
    }
    original.method = "read".into();
    original.capture = None;
    assert!(original.reference().is_err());
    let Interface::Service { methods } = &mut original.contract.interface else {
        unreachable!()
    };
    methods.retain(|_, method| method.mode == MethodMode::Query);
    assert!(
        OperationTemplate::for_contract(&original.contract)
            .unwrap()
            .is_empty()
    );
    store.close().await.unwrap();
}
