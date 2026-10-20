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

#![cfg(target_os = "macos")]

use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use maka_host_runtime::application::Subscription;

fn path(name: &str) -> Value {
    json!({"bytesBase64":STANDARD.encode(name.as_bytes())})
}

async fn next(stream: &mut Subscription) -> Result<Value, String> {
    tokio::time::timeout(Duration::from_secs(10), stream.recv())
        .await
        .map_err(|error| format!("reconciliation item deadline: {error}"))?
        .ok_or_else(|| "reconciliation observation ended early".into())
}

async fn quiet(stream: &mut Subscription) -> Result<(), String> {
    // A negative behavior window spanning the one-second cadence, not a sleep
    // used to synchronize a mutation or wait for a successful observation.
    match tokio::time::timeout(Duration::from_millis(1200), stream.recv()).await {
        Err(_) => Ok(()),
        Ok(value) => Err(format!("unchanged snapshot invented an item: {value:?}")),
    }
}

async fn changed(stream: &mut Subscription) -> Result<(), String> {
    let value = next(stream).await?;
    if value == json!({"kind":"changed"}) {
        Ok(())
    } else {
        Err(format!(
            "expected actual-state invalidation, received {value:?}"
        ))
    }
}

async fn listed(
    f: &Fixture,
    session: &str,
    identity: &Value,
    name: Option<&str>,
) -> Result<(), String> {
    let page = f
        .query(
            "listFiles",
            json!({"sessionId":session,"path":path("reconciled"),"after":null}),
        )
        .await?;
    let entries = match name {
        Some(name) => json!([{"path":path(&format!("reconciled/{name}")),"kind":"file"}]),
        None => json!([]),
    };
    if page["directoryIdentity"] != *identity
        || page["entries"] != entries
        || page["next"] != Value::Null
    {
        return Err(format!(
            "unexpected reconciled page: {page:?}, expected {entries:?}"
        ));
    }
    Ok(())
}

async fn phase(f: &Fixture, session: &str, reconnect: Option<&Value>) -> Result<Value, String> {
    let observed = scope(&f.issuer, true);
    let subscription = f
        .app
        .client()
        .subscribe(
            f.binding(2).await,
            "watchFiles".into(),
            json!({"sessionId":session,"path":path("reconciled")}),
            observed.clone(),
            Attachments::new().into(),
        )
        .await;
    let mut failures = Vec::new();
    let result = match subscription {
        Ok(mut stream) => {
            // Keep every fallible business result until the original native
            // Watch, Host subscription and Scope have all settled below.
            let result = async {
                let ready = next(&mut stream).await?;
                if ready["kind"] != "ready" {
                    return Err(format!("expected Ready: {ready:?}"));
                }
                let identity = ready["directoryIdentity"].clone();
                if reconnect.is_some_and(|original| original != &identity) {
                    return Err("reconnect changed the original directory identity".into());
                }
                listed(f, session, &identity, reconnect.map(|_| "created")).await?;
                quiet(&mut stream).await?;
                let directory = f.workspace.join("reconciled");
                if reconnect.is_none() {
                    std::fs::write(directory.join("created"), b"first")
                        .map_err(|e| e.to_string())?;
                    changed(&mut stream).await?;
                    listed(f, session, &identity, Some("created")).await?;
                    if std::fs::read(directory.join("created")).map_err(|e| e.to_string())?
                        != b"first"
                    {
                        return Err("created bytes differ".into());
                    }
                } else {
                    // Reopening establishes a baseline containing this existing
                    // child; a directory-only watcher cannot cover its overwrite.
                    std::fs::write(directory.join("created"), b"other")
                        .map_err(|e| e.to_string())?;
                    changed(&mut stream).await?;
                    listed(f, session, &identity, Some("created")).await?;
                    if std::fs::read(directory.join("created")).map_err(|e| e.to_string())?
                        != b"other"
                    {
                        return Err("same-size overwrite bytes differ".into());
                    }
                    std::fs::rename(directory.join("created"), directory.join("renamed"))
                        .map_err(|e| e.to_string())?;
                    changed(&mut stream).await?;
                    listed(f, session, &identity, Some("renamed")).await?;
                    if std::fs::read(directory.join("renamed")).map_err(|e| e.to_string())?
                        != b"other"
                        || directory.join("created").exists()
                    {
                        return Err("rename did not preserve bytes and remove the old name".into());
                    }
                    std::fs::remove_file(directory.join("renamed")).map_err(|e| e.to_string())?;
                    changed(&mut stream).await?;
                    listed(f, session, &identity, None).await?;
                    quiet(&mut stream).await?;
                    // With native hints disabled, only the scheduled scan can
                    // notice that the held directory is no longer at its path.
                    std::fs::rename(&directory, f.workspace.join("moved-reconciled"))
                        .map_err(|e| e.to_string())?;
                    let terminal = next(&mut stream).await?;
                    if terminal != json!({"kind":"unavailable","reason":"directory_changed"}) {
                        return Err(format!("expected terminal directory change: {terminal:?}"));
                    }
                    if tokio::time::timeout(Duration::from_secs(10), stream.recv())
                        .await
                        .map_err(|error| format!("terminal observation deadline: {error}"))?
                        .is_some()
                    {
                        return Err("directory change did not end observation".into());
                    }
                }
                Ok(identity)
            }
            .await;
            stream.cancel();
            let completed = stream.completion().await;
            if let Err(error) = completed.cleanup {
                failures.push(format!("watch cleanup: {error}"));
            }
            result
        }
        Err(error) => Err(error),
    };
    let finished = observed.resources.finish().await;
    if let Err(error) = finished {
        failures.push(format!("Scope finish: {error:?}"));
    }
    if !observed.resources.is_idle() {
        failures.push("original watch Scope retained resources".into());
    }
    f.drained().await;
    match result {
        Ok(identity) if failures.is_empty() => Ok(identity),
        Ok(_) => Err(failures.join("; ")),
        Err(error) => {
            failures.insert(0, error);
            Err(failures.join("; "))
        }
    }
}

#[tokio::test]
async fn actual_shallow_changes_reconcile_without_consuming_native_hints() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let f = Fixture::new("http://127.0.0.1:1/v1", None).await;
        f.owner
            .file_native_hints
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let session = f.create("Independent file reconciliation").await;
        let result = async {
            std::fs::create_dir(f.workspace.join("reconciled")).map_err(|e| e.to_string())?;
            let identity = phase(&f, &session, None).await?;
            phase(&f, &session, Some(&identity)).await?;
            Ok::<(), String>(())
        }
        .await;
        f.drained().await;
        let app_shutdown = f.app.shutdown().await;
        let config_shutdown = f.config.shutdown().await;
        assert!(app_shutdown.is_ok(), "app shutdown: {app_shutdown:?}");
        assert!(
            config_shutdown.is_ok(),
            "config shutdown: {config_shutdown:?}"
        );
        assert!(result.is_ok(), "independent reconciliation: {result:?}");
    })
    .await
    .unwrap();
}
