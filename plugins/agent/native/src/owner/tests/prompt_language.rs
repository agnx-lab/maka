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

#[tokio::test]
async fn shared_language_preference_can_be_saved_through_settings_host() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let fixture = Fixture::new("http://127.0.0.1:9/v1", None).await;
        let client = fixture.app.client();
        let binding = client
            .bind(client.activation(&id(2)).unwrap(), "settings".into())
            .await
            .unwrap()
            .remove(0);
        let initial = client
            .start(
                binding.clone(),
                "assistantPreferences".into(),
                json!({}),
                scope(&fixture.issuer, true),
                Attachments::new().into(),
            )
            .await
            .unwrap()
            .result()
            .await
            .unwrap();
        let revision = initial["revision"].as_u64().unwrap();
        let outcome: Result<Value, String> = async {
            let prepared = client
                .prepare_mutation(
                    binding,
                    "updateAssistantPreferences".into(),
                    json!({
                        "expectedRevision":revision,
                        "mutation":{"kind":"language","preference":"zh-CN"}
                    }),
                    scope(&fixture.issuer, true),
                    Uuid::new_v4(),
                    Attachments::new().into(),
                )
                .await?;
            let reply = client.mutate_owned(prepared).await?.result().await;
            match reply.receipt {
                Receipt::Recorded {
                    outcome: OperationOutcome::Returned { value },
                    ..
                } => Ok(value),
                receipt => Err(format!(
                    "unexpected language preference receipt: {receipt:?}"
                )),
            }
        }
        .await;
        let saved_revision = fixture.config.runtime_policy().await.unwrap().revision;
        fixture.app.shutdown().await.unwrap();
        fixture.config.shutdown().await.unwrap();

        assert_eq!(
            outcome,
            Ok(json!({"kind":"committed","revision":revision + 1})),
            "the shared language preference must be accepted through the public Settings capability"
        );
        assert_eq!(
            saved_revision,
            revision + 1,
            "the preference commit must reach the root-owned policy"
        );
    })
    .await
    .unwrap();
}

async fn settings_binding(app: &Application, consumer: u128) -> Binding {
    let client = app.client();
    client
        .bind(client.activation(&id(consumer)).unwrap(), "settings".into())
        .await
        .unwrap()
        .remove(0)
}

async fn preferences(app: &Application, issuer: &Issuer, consumer: u128) -> Value {
    app.client()
        .start(
            settings_binding(app, consumer).await,
            "assistantPreferences".into(),
            json!({}),
            scope(issuer, true),
            Attachments::new().into(),
        )
        .await
        .unwrap()
        .result()
        .await
        .unwrap()
}

async fn update_preferences(
    app: &Application,
    issuer: &Issuer,
    consumer: u128,
    revision: &Value,
    mutation: Value,
) -> Value {
    let client = app.client();
    let prepared = client
        .prepare_mutation(
            settings_binding(app, consumer).await,
            "updateAssistantPreferences".into(),
            json!({"expectedRevision":revision,"mutation":mutation}),
            scope(issuer, true),
            Uuid::new_v4(),
            Attachments::new().into(),
        )
        .await
        .unwrap();
    let reply = client.mutate_owned(prepared).await.unwrap().result().await;
    let Receipt::Recorded {
        outcome: OperationOutcome::Returned { value },
        ..
    } = reply.receipt
    else {
        panic!("unexpected preference receipt: {:?}", reply.receipt)
    };
    value
}

async fn start_settings_app(fixture: &Fixture, deployment: Uuid) -> Application {
    let mut native = NativeRegistry::default();
    maka_settings_native::provide(&mut native).unwrap();
    provide_all(&mut native).unwrap();
    let app = Application::start_from_store(
        fixture.store.clone(),
        deployment,
        native,
        fixture.issuer.clone(),
        Arc::new(Grant(fixture.issuer.clone(), None)),
        Some(maka_host_js::factory(Default::default())),
    )
    .await
    .unwrap();
    app.ready().await.unwrap();
    app
}

async fn configured_settings_app(fixture: &Fixture, locale: &str) -> (Uuid, Application) {
    let (mut composition, mut packages) = input(&fixture.owner.settings);
    let package = packages
        .iter_mut()
        .find(|package| package.manifest().id == "settings")
        .unwrap();
    let previous = package.application_ref();
    let mut manifest = package.manifest().clone();
    let mut declaration = manifest.application.as_ref().unwrap().declaration().clone();
    declaration
        .entries
        .get_mut(&EntryTarget::Host)
        .unwrap()
        .configuration = maka_settings_native::native().configuration;
    manifest.application =
        Some(maka_plugin_core::package::CheckedDeclaration::new(declaration).unwrap());
    let mut files = package.files().clone();
    files.insert(MANIFEST_FILE.into(), serde_json::to_vec(&manifest).unwrap());
    *package = Package::new(files).unwrap();
    for node in composition
        .instances
        .values_mut()
        .filter(|node| node.package == previous.clone().into())
    {
        node.package = package.application_ref().into();
        node.configuration
            .values
            .insert("locale".into(), json!(locale));
    }
    let deployment = Uuid::new_v4();
    fixture
        .store
        .commit_application(deployment, 0, composition, vec![], packages)
        .await
        .unwrap();
    (deployment, start_settings_app(fixture, deployment).await)
}

#[tokio::test]
async fn settings_surfaces_share_saved_language_and_cas_across_activation() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let fixture = Fixture::new("http://127.0.0.1:9/v1", None).await;
        let native = preferences(&fixture.app, &fixture.issuer, 2).await;
        let (deployment, configured) = configured_settings_app(&fixture, "zh-Hans-CN").await;
        let initial = preferences(&configured, &fixture.issuer, 3).await;
        let personalized = update_preferences(&configured, &fixture.issuer, 3, &initial["revision"],
            json!({"kind":"personalization","displayName":"language-authority-用户","assistantTone":"Retain my exact custom tone."})).await;
        let chinese = update_preferences(&fixture.app, &fixture.issuer, 2, &personalized["revision"],
            json!({"kind":"language","preference":"zh-TW"})).await;
        let shared = preferences(&configured, &fixture.issuer, 3).await;
        let stale = update_preferences(&configured, &fixture.issuer, 3, &personalized["revision"],
            json!({"kind":"language","preference":"en"})).await;
        let after_stale = preferences(&fixture.app, &fixture.issuer, 2).await;
        let english = update_preferences(&fixture.app, &fixture.issuer, 2, &shared["revision"],
            json!({"kind":"language","preference":"en"})).await;
        configured.shutdown().await.unwrap();
        let reopened = start_settings_app(&fixture, deployment).await;
        let restored = preferences(&reopened, &fixture.issuer, 3).await;
        let automatic = update_preferences(&reopened, &fixture.issuer, 3, &restored["revision"],
            json!({"kind":"language","preference":"auto"})).await;
        let auto_configured = preferences(&reopened, &fixture.issuer, 3).await;
        let auto_native = preferences(&fixture.app, &fixture.issuer, 2).await;
        reopened.shutdown().await.unwrap();
        fixture.app.shutdown().await.unwrap();
        fixture.config.shutdown().await.unwrap();

        assert_eq!(native["language"], Value::Null);
        assert_eq!(initial["language"], Value::Null);
        assert_eq!(initial["resolvedLanguage"], "zh-CN");
        assert_eq!(personalized["kind"], "committed");
        assert_eq!(chinese["kind"], "committed");
        assert_eq!(shared["language"], "zh-TW");
        assert_eq!(shared["resolvedLanguage"], "zh-TW");
        assert_eq!(stale, json!({"kind":"revision_conflict","expectedRevision":personalized["revision"],"actualRevision":chinese["revision"]}));
        assert_eq!(after_stale, shared, "a stale surface cannot replace the shared policy");
        assert_eq!(english["kind"], "committed");
        assert_eq!(restored["language"], "en", "explicit English survives reopening as a saved override");
        assert_eq!(restored["resolvedLanguage"], "en");
        assert_eq!(automatic["kind"], "committed");
        assert_eq!(auto_configured["language"], "auto");
        assert_eq!(auto_configured["resolvedLanguage"], native["resolvedLanguage"], "explicit Auto bypasses the configured Chinese locale");
        assert_eq!(auto_configured, auto_native);
        assert_eq!(auto_configured["displayName"], "language-authority-用户");
        assert_eq!(auto_configured["assistantTone"], "Retain my exact custom tone.");
    }).await.unwrap();
}
