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
use maka_packages::MANIFEST_FILE;

fn replace_manifest(package: &Package, manifest: Value) -> Package {
    let mut files = package.files().clone();
    files.insert(MANIFEST_FILE.into(), serde_json::to_vec(&manifest).unwrap());
    Package::new(files).unwrap()
}

#[test]
#[ignore = "opt-in real Agent, JS executor, production AgentScreen and foreground PTY"]
fn executor_selection_and_composer_reach_production_timeline_before_backend_completion() {
    let (dist, mut packages) = built_packages();
    let refs = packages
        .iter()
        .map(|(name, package)| (name.clone(), package.application_ref()))
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("ui");
    let agent_root = directory.path().join("agent");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let upload = workspace.join("executor-note.txt");
    std::fs::write(&upload, "Actual file bytes for external backend\n").unwrap();
    let deployment = Uuid::new_v4();
    let agent_id = Uuid::new_v4();
    let manager_id = Uuid::new_v4();
    let (mut graph, _, accent) = authored(
        &dist.join("authoring/composition.mjs"),
        json!({
            "presentation":"tui", "locale":"en", "scheme":"light",
            "hostRoot":agent_root, "hostDeployment":agent_id, "managementDeployment":manager_id,
            "studioRoot":directory.path().join("studio"), "studioDeployment":Uuid::new_v4(),
            "workspaceRoots":[{"label":"Workspace", "path":workspace}]
        }),
        &refs,
    );
    let executor = serde_json::to_value(maka_agent_api::executor::contract()).unwrap();
    let control = json!({"name":"fixture.ExecutorControl","version":1,"transport":"remote","interface":{"kind":"service","methods":{"release":{"mode":"mutation","input":{"type":"object","additionalProperties":false},"output":{"type":"object"},"resources":{}}}}});
    let one = |contract: &Value| json!({"contract":contract,"cardinality":{"kind":"one"}});
    let required = |contract: &Value| json!({"contract":contract,"availability":"required"});
    let local = |name: &str, export: &str| json!({"kind":"instance","path":[name],"export":export});
    let mut agent_manifest = serde_json::to_value(packages["agent"].manifest()).unwrap();
    agent_manifest["application"]["entries"]["host"]["imports"]["executor"] = one(&executor);
    let agent_package = replace_manifest(&packages["agent"], agent_manifest);
    graph["host"]["instances"]["agent"]["package"]["package"] =
        serde_json::to_value(agent_package.application_ref()).unwrap();
    graph["host"]["instances"]["agent"]["configuration"]["values"]["executors"] =
        json!([{"name":"fixture-backend","import":"executor"}]);
    graph["host"]["instances"]["agent"]["bindings"]["executor"] =
        json!([local("executor", "executor")]);
    packages.insert("agent".into(), agent_package);
    let executor_package = Package::new(BTreeMap::from([
        (MANIFEST_FILE.into(),serde_json::to_vec(&json!({"schemaVersion":1,"id":"fixture.executor.progress","application":{"entries":{"host":{
            "implementation":{"kind":"javascript","module":"executor.mjs","realm":"shared"},
            "configuration":{"type":"object","additionalProperties":false},"imports":{},
            "exports":{"executor":required(&executor),"control":required(&control)}
        }}}})).unwrap()),
        ("executor.mjs".into(),format!(r#"
            let release;
            export function activate(){{ return {{executor:{{contract:{executor},service:['backend']}},control:{{contract:{control},service:['control']}}}}; }}
            export const backend={{async execute(input,observation,context){{
                if(input.message !== 'Run real external backend' || input.settings.model !== 'backend-model') throw Error(JSON.stringify(input));
                const file=await context.invoke({{operation:'metadata',slot:'attachments'}});
                const bytes=await context.invoke({{operation:'read',slot:'attachments',start:0,end:file.bytes}});
                if(file.metadata.length !== 1 || file.metadata[0].name !== 'executor-note.txt' || file.metadata[0].bytes !== file.bytes || new TextDecoder().decode(new Uint8Array(bytes)) !== 'Actual file bytes for external backend\n') throw Error('attachment was not delivered');
                await context.invoke({{operation:'emit',value:{{type:'thinking_delta',text:'Inspecting external workspace'}}}});
                await context.invoke({{operation:'emit',value:{{type:'output_delta',text:'Live external answer'}}}});
                await new Promise(resolve=>{{release=resolve;}});
                return {{status:'completed',text:'Final external answer'}};
            }}}};
            export const control={{release(){{if(!release)throw Error('not executing');release();return {{released:true}};}}}};
        "#).into_bytes())
    ])).unwrap();
    let provider_key = Uuid::new_v4();
    graph["host"]["instances"]["executor"] = json!({"key":provider_key,"package":{"source":"pinned","package":executor_package.application_ref()},"usePackage":"entry","configuration":{"values":{}},"bindings":{}});
    packages.insert("executor".into(), executor_package);
    let mut gateway_manifest = serde_json::to_value(packages["gateway"].manifest()).unwrap();
    gateway_manifest["application"]["entries"]["host"]["imports"]["control"] = one(&control);
    let gateway_package = replace_manifest(&packages["gateway"], gateway_manifest);
    graph["host"]["instances"]["gateway"]["package"]["package"] =
        serde_json::to_value(gateway_package.application_ref()).unwrap();
    graph["host"]["instances"]["gateway"]["bindings"]["control"] =
        json!([local("executor", "control")]);
    packages.insert("gateway".into(), gateway_package);
    let agent_key = graph["host"]["instances"]["agent"]["key"].clone();
    let gateway_key = graph["host"]["instances"]["gateway"]["key"].clone();
    let host = graph["connections"]["hosts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|host| host["name"] == "local")
        .unwrap();
    host["native"] = json!([{"origin":[agent_key],"consumer":[agent_key],"provider":[provider_key],"export":"executor","methods":["execute"],"recovery":true}]);
    host["endpoints"]["control"] = json!({"consumer":[gateway_key],"import":"control","grant":{"methods":["release"],"recovery":true}});
    let remote_control = json!({"deployment":"local","instance":[provider_key],"export":"control"});
    graph["remoteExports"]
        .as_array_mut()
        .unwrap()
        .push(json!({"capability":remote_control,"declaration":required(&control)}));
    let screen_contract = serde_json::to_value(
        &packages["agent-screen"]
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .entries[&maka_plugin_core::package::EntryTarget::Tui]
            .exports["AgentScreen"]
            .contract,
    )
    .unwrap();
    let agent_contract = serde_json::to_value(maka_agent_api::contract()).unwrap();
    let surface = serde_json::to_value(
        &packages["surface"]
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .entries[&maka_plugin_core::package::EntryTarget::Tui]
            .exports["Surface"]
            .contract,
    )
    .unwrap();
    let ui_package=Package::new(BTreeMap::from([
        (MANIFEST_FILE.into(),serde_json::to_vec(&json!({"schemaVersion":1,"id":"fixture.executor.ui","application":{"entries":{"tui":{
            "implementation":{"kind":"javascript","module":"ui.mjs","realm":"shared"},"scene":{"name":"maka.tui.scene","version":1},"configuration":{"type":"object","additionalProperties":false},
            "imports":{"agentScreen":one(&screen_contract),"agent":one(&agent_contract),"control":one(&control),"surface":one(&surface)},
            "exports":{"App":required(&screen_contract)}
        }}}})).unwrap()),
        ("ui.mjs".into(),format!("export function activate(){{return {{App:{{kind:'component',contract:{screen_contract},component:['app']}}}};}}\n{}",include_str!("executor-ui.js")).into_bytes())
    ])).unwrap();
    graph["ui"]["instances"]["fixture"] = json!({"key":Uuid::new_v4(),"package":{"source":"pinned","package":ui_package.application_ref()},"usePackage":"entry","configuration":{"values":{}},"bindings":{
        "agentScreen":[local("agent-screen","AgentScreen")],"surface":[local("surface","Surface")],
        "agent":graph["ui"]["instances"]["agent-screen"]["bindings"]["agent"],
        "control":[{"kind":"remote","deployment":"local","instance":[provider_key],"export":"control"}]
    }});
    graph["ui"]["launch"] = local("fixture", "App");
    packages.insert("fixture".into(), ui_package);
    let connections = directory.path().join("connections.json");
    std::fs::write(
        &connections,
        serde_json::to_vec(&graph["connections"]).unwrap(),
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let ns = RootNamespaces::for_current_account().unwrap();
    runtime.block_on(async {
        for (path, id, input, remote) in [
            (&agent_root, agent_id, &graph["host"], json!([])),
            (&root, manager_id, &graph["managementHost"], json!([])),
        ] {
            let store = EventLog::for_root(Arc::new(RootOwner::create(path, &ns).unwrap()))
                .await
                .unwrap();
            store
                .commit_application(
                    id,
                    0,
                    serde_json::from_value(input.clone()).unwrap(),
                    serde_json::from_value(remote).unwrap(),
                    packages.values().cloned().collect(),
                )
                .await
                .unwrap();
            store.shutdown().await.unwrap();
        }
        let store = EventLog::for_root(Arc::new(RootOwner::open(&root, &ns).unwrap()))
            .await
            .unwrap();
        store
            .commit_application(
                deployment,
                0,
                serde_json::from_value(graph["ui"].clone()).unwrap(),
                serde_json::from_value(graph["remoteExports"].clone()).unwrap(),
                packages.values().cloned().collect(),
            )
            .await
            .unwrap();
        store.shutdown().await.unwrap();
    });
    let mut tui = Pty::spawn(&[
        "application",
        "run",
        "--root",
        root.to_str().unwrap(),
        "--deployment",
        &deployment.to_string(),
        "--connections",
        connections.to_str().unwrap(),
    ]);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        tui.read();
        if tui.frames.ready() && tui.screen.preview(usize::MAX).unwrap().screen.contains("ready") {
            break;
        }
        assert!(
            tui.child.try_wait().unwrap().is_none() && Instant::now() < deadline,
            "fixture startup: {}",
            String::from_utf8_lossy(&tui.output)
        );
    }
    // The fixture's create receipt precedes AgentScreen's observed Session. Wait
    // for its actual composer before using footer coordinates; the home action
    // has the same label at a different position.
    tui.wait_until(|screen| screen.contains("Message…") && screen.contains("Execution backend"));
    let before = tui.screen.preview(usize::MAX).unwrap().screen;
    click(&mut tui, "[ Execution backend ]", accent);
    click(&mut tui, "[ fixture-backend ]", accent);
    click(&mut tui, "Backend default", accent);
    paste(&mut tui, "backend-model");
    tui.wait_for("backend-model");
    let picker = tui.screen.preview(usize::MAX).unwrap().screen;
    click(&mut tui, "[ Apply ]", accent);
    tui.wait_until(|screen| {
        screen.contains("Message…") && !screen.contains("Backend model (optional)")
    });
    click(&mut tui, "[ fixture-backend ]", accent);
    tui.wait_until(|screen| {
        screen.contains("Backend model (optional)") && screen.contains("backend-model")
    });
    let reopened = tui.screen.preview(usize::MAX).unwrap().screen;
    assert!(
        reopened.contains("Close"),
        "compact settings must keep the closing control visible"
    );
    tui.send(b"\x1b");
    tui.wait_until(|screen| {
        screen.contains("Message…") && !screen.contains("Backend model (optional)")
    });
    click(&mut tui, "Message…", accent);
    paste(&mut tui, upload.to_str().unwrap());
    tui.wait_until(|screen| screen.contains("executor-note.txt") && screen.contains("Ready"));
    let attached = tui.screen.preview(usize::MAX).unwrap().screen;
    click(&mut tui, "Message…", accent);
    paste(&mut tui, "Run real external backend");
    tui.wait_for("Run real external backend");
    tui.send(b"\r");
    tui.wait_for("Live external answer");
    let live = tui.screen.preview(usize::MAX).unwrap().screen;
    assert!(live.contains("Inspecting external workspace"));
    assert!(!live.contains("Final external answer"));
    click(&mut tui, "Release external", accent);
    tui.wait_until(|screen| {
        screen.contains("Final external answer")
            && !screen.contains("Live external answer")
            && screen.contains("Finished")
    });
    if let Ok(path) = std::env::var("MAKA_EXECUTOR_PROGRESS_EVIDENCE") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(Path::new(&path).join("before.txt"), before).unwrap();
        std::fs::write(Path::new(&path).join("picker.txt"), picker).unwrap();
        std::fs::write(Path::new(&path).join("reopened.txt"), reopened).unwrap();
        std::fs::write(Path::new(&path).join("attached.txt"), attached).unwrap();
        std::fs::write(Path::new(&path).join("live.txt"), live).unwrap();
        std::fs::write(
            Path::new(&path).join("completed.txt"),
            tui.screen.preview(usize::MAX).unwrap().screen,
        )
        .unwrap();
        std::fs::write(Path::new(&path).join("terminal.ansi"), &tui.output).unwrap();
    }
    tui.send(b"\x11");
    tui.finish();
    drop(RootOwner::open(&agent_root, &ns).unwrap());
}
