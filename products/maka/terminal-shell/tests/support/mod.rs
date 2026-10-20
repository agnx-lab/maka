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

use maka_app_client::{Client, Journal};
use maka_capabilities::call::Attachments;
use maka_event_log::{
    EventLog,
    root::{RootNamespaces, RootOwner, private_directory},
};
use maka_host_runtime::{
    application::{
        NativeRegistry,
        host::{Client as HostClient, Host, discovery::Registration},
        transport::{Grant, HostEndpoint, NativeGrant, wire},
    },
    ipc::{self, LocalListener},
};
use maka_packages::Package;
use maka_plugin_core::{
    composition::InstanceId,
    operation::{OperationOutcome, Receipt, receipt_reader_contract},
};
use nsh_engine::Bundle;
use nsh_platform_tui::{AppOptions, AppView};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, mpsc},
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const EDITOR: &str = "composer";

#[cfg(unix)]
pub mod pty;

/// A real Agent, settings store and public Host gateway, without the old UI.
pub struct Fixture {
    pub address: PathBuf,
    pub session: String,
    pub selection: Value,
    pub requests: mpsc::Receiver<ModelRequest>,
    pub request_count: Arc<AtomicUsize>,
    registration: Option<Registration>,
    host: Arc<Host>,
    server: JoinHandle<Result<(), String>>,
    provider: JoinHandle<()>,
    // Registration must be removed while its original Root still exists on failure.
    pub directory: tempfile::TempDir,
}

impl Fixture {
    pub async fn open() -> Self {
        Self::open_with_options(false, false).await
    }
    #[cfg(unix)]
    pub async fn registered() -> Self {
        Self::open_with_options(true, false).await
    }
    #[cfg(unix)]
    pub async fn registered_images() -> Self {
        Self::open_with_options(true, true).await
    }
    async fn open_with_options(registered: bool, images: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        private_directory(directory.path()).unwrap();
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let namespaces = if registered {
            RootNamespaces::for_current_account().unwrap()
        } else {
            RootNamespaces {
                ownership: directory.path().join("owners"),
                control: directory.path().join("control"),
            }
        };
        let owner =
            Arc::new(RootOwner::create(&directory.path().join("host"), &namespaces).unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let selection = configure(owner.clone(), &base, images).await;
        let store = Arc::new(EventLog::for_root(owner).await.unwrap());
        let package = |id: &str,
                       implementation: &str,
                       configuration: Value,
                       imports: Value,
                       exports: Value| {
            Package::new(BTreeMap::from([(
                "maka.extension.json".into(),
                serde_json::to_vec(&json!({
                    "schemaVersion":1,"id":id,"application":{"entries":{"host":{
                        "implementation":{"kind":"native","implementation":implementation},
                        "configuration":configuration,"imports":imports,"exports":exports
                    }}}
                }))
                .unwrap(),
            )]))
            .unwrap()
        };
        let agent_contract = maka_agent_api::contract();
        let settings_contract = maka_settings_native::contract::contract();
        let receipts_contract = receipt_reader_contract();
        let files_contract = maka_workspace_api::files::contract();
        let empty = json!({"type":"object","additionalProperties":false});
        let agent = package(
            "test.nsh.agent",
            maka_agent_api::IMPLEMENTATION,
            maka_agent_api::configuration_schema(),
            if images {
                json!({"files":{"contract":files_contract,"cardinality":{"kind":"one"}}})
            } else {
                json!({})
            },
            json!({"agent":{"contract":agent_contract,"availability":"required"}}),
        );
        let files = package(
            "test.nsh.files",
            maka_workspace_api::files::IMPLEMENTATION,
            empty.clone(),
            json!({}),
            json!({"files":{"contract":files_contract,"availability":"required"}}),
        );
        let settings = package(
            "test.nsh.settings",
            maka_settings_native::IMPLEMENTATION,
            empty.clone(),
            json!({}),
            json!({"Settings":{"contract":settings_contract,"availability":"required"}}),
        );
        let receipts = package(
            "test.nsh.receipts",
            "maka.operations.receipts",
            empty.clone(),
            json!({}),
            json!({"Receipts":{"contract":receipts_contract,"availability":"required"}}),
        );
        let gateway = package(
            "test.nsh.gateway",
            "maka.application.gateway",
            empty,
            json!({
                "agent":{"contract":agent_contract,"cardinality":{"kind":"one"}},
                "settings":{"contract":settings_contract,"cardinality":{"kind":"one"}},
                "receipts":{"contract":receipts_contract,"cardinality":{"kind":"one"}}
            }),
            json!({}),
        );
        let node = |package: &Package, key: u128, configuration: Value, bindings: Value| {
            json!({
                "key":Uuid::from_u128(key),"package":{"source":"pinned","package":package.application_ref()},
                "usePackage":"entry","configuration":{"values":configuration},"bindings":bindings
            })
        };
        let deployment = Uuid::new_v4();
        let mut plan = json!({"environment":{"location":"host"},"instances":{
            "agent":node(&agent, 1, json!({"locale":"en","workspaceRoots":[{"label":"Workspace","path":workspace}],"tools":if images { json!([{"name":"Read","description":"Read an image in the selected workspace.","import":"files","method":"readImage","result":"image","confirmation":"preapproved"}]) } else { json!([]) },"todo":true,"permissions":{"access":"read_only","requestPermissions":true}}), if images { json!({"files":[{"kind":"instance","path":["files"],"export":"files"}]}) } else { json!({}) }),
            "settings":node(&settings, 2, json!({}), json!({})),
            "receipts":node(&receipts, 3, json!({}), json!({})),
            "gateway":node(&gateway, 4, json!({}), json!({
                "agent":[{"kind":"instance","path":["agent"],"export":"agent"}],
                "settings":[{"kind":"instance","path":["settings"],"export":"Settings"}],
                "receipts":[{"kind":"instance","path":["receipts"],"export":"Receipts"}]
            }))
        }});
        let mut packages = vec![agent, settings, receipts, gateway];
        if images {
            plan["instances"]["files"] = node(&files, 5, json!({}), json!({}));
            packages.push(files);
        }
        let input = serde_json::from_value(plan).unwrap();
        store
            .commit_application(deployment, 0, input, vec![], packages)
            .await
            .unwrap();
        let mut native = NativeRegistry::default();
        maka_agent_native::provide(&mut native).unwrap();
        maka_settings_native::provide(&mut native).unwrap();
        if images {
            maka_files_native::provide(&mut native).unwrap();
        }
        let endpoint = |import: &str, methods: &[&str], recovery| HostEndpoint {
            consumer: InstanceId(vec![Uuid::from_u128(4)]),
            import: import.into(),
            grant: Grant {
                methods: methods.iter().map(|name| (*name).into()).collect(),
                recovery,
            },
        };
        let host = Host::open(
            store,
            deployment,
            native,
            None,
            BTreeMap::from([
                (
                    "agent".into(),
                    endpoint(
                        "agent",
                        &[
                            "list",
                            "directories",
                            "create",
                            "selectBackend",
                            "send",
                            "stop",
                            "execution",
                            "pendingInteractions",
                            "interaction",
                            "respond",
                            "source",
                            "toolImage",
                            "checklist",
                            "watchChecklist",
                            "observe",
                            "history",
                            "watch",
                        ],
                        false,
                    ),
                ),
                (
                    "settings".into(),
                    endpoint("settings", &["models", "resolveModel"], false),
                ),
                ("receipts".into(), endpoint("receipts", &["read"], true)),
            ]),
            if images {
                vec![NativeGrant {
                    origin: InstanceId(vec![Uuid::from_u128(1)]),
                    consumer: InstanceId(vec![Uuid::from_u128(1)]),
                    provider: InstanceId(vec![Uuid::from_u128(5)]),
                    export: maka_workspace_api::files::EXPORT.into(),
                    methods: ["readImage".into()].into_iter().collect(),
                    recovery: false,
                }]
            } else {
                vec![]
            },
        )
        .await
        .unwrap();
        let address = socket(directory.path(), "host");
        let local = LocalListener::bind(&address).unwrap();
        let registration = registered.then(|| host.publish_registration(&address, None).unwrap());
        let server = tokio::spawn({
            let host = host.clone();
            async move { host.serve_local(local, CancellationToken::new()).await }
        });
        let (send, requests) = mpsc::channel(8);
        let request_count = Arc::new(AtomicUsize::new(0));
        let provider = tokio::spawn({
            let count = request_count.clone();
            async move {
                let mut connections = JoinSet::new();
                loop {
                    tokio::select! {
                        accepted = listener.accept() => {
                            let (stream, _) = accepted.unwrap(); let send = send.clone(); let count = count.clone();
                            connections.spawn(async move {
                                let request = ModelRequest::read(stream).await;
                                count.fetch_add(1, Ordering::SeqCst);
                                send.send(request).await.unwrap();
                            });
                        }
                        Some(result) = connections.join_next(), if !connections.is_empty() => { result.unwrap(); }
                    }
                }
            }
        });
        let mut fixture = Self {
            directory,
            address,
            session: String::new(),
            selection,
            requests,
            request_count,
            registration,
            host,
            server,
            provider,
        };
        let client = fixture.client().await;
        let roots = client
            .query("agent", "directories", json!({"kind":"roots"}))
            .await
            .unwrap();
        let created = mutate(&client, "create", json!({"title":"NSH real Agent", "labels":[], "workspace":{"kind":"directory","selection":roots["roots"][0]["selection"]}})).await;
        fixture.session = created["session"]["id"].as_str().unwrap().to_owned();
        client.close().await.unwrap();
        fixture
    }
    pub async fn client(&self) -> Client {
        self.client_at(&self.address).await
    }
    #[cfg(unix)]
    pub fn register_at(&mut self, address: &Path) {
        if let Some(registration) = self.registration.take() {
            registration.remove().unwrap();
        }
        self.registration = Some(self.host.publish_registration(address, None).unwrap());
    }
    pub async fn client_at(&self, address: &Path) -> Client {
        let host = HostClient::connect(address, Some(self.host.epoch()))
            .await
            .unwrap();
        assert!(
            host.catalogue()
                .describe("agent")
                .unwrap()
                .registration_epoch
                .is_some(),
            "{:?}",
            host.catalogue().startup_error()
        );
        Client::new(
            host,
            Journal::open(&self.directory.path().join("client.db"))
                .await
                .unwrap(),
            "receipts".into(),
        )
        .unwrap()
    }
    pub async fn next_request(&mut self) -> ModelRequest {
        tokio::time::timeout(Duration::from_secs(20), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }
    pub async fn idle(&self, client: &Client, session: &str) {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let observed = client
                    .query("agent", "observe", json!({"sessionId":session}))
                    .await
                    .unwrap();
                if observed["session"]["activity"] == "idle" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("real Agent did not finish");
    }
    pub async fn seed_history(&mut self, turns: usize) {
        let client = self.client().await;
        for turn in 0..turns {
            let reply = mutate(&client, "send", json!({"sessionId":self.session,"message":format!("History question {turn:02}"),"model":self.selection})).await;
            assert_eq!(reply["kind"], "started", "{reply}");
            let mut request = self.next_request().await;
            let text: String = (0..30).map(|row| format!("History {turn:02} paragraph {row:02}: original provider text with **bold** and `code`.\n\n")).collect();
            request.delta(&text).await;
            request.finish().await;
            self.idle(&client, &self.session).await;
        }
        client.close().await.unwrap();
    }
    pub fn view(
        &self,
        client: Client,
        initial_session: Option<String>,
    ) -> (AppView, maka_terminal_shell::bindings::Bindings) {
        let bindings =
            maka_terminal_shell::bindings::Bindings::new(client, tokio::runtime::Handle::current());
        let mut view = AppView::new(AppOptions {
            size: evidence_size(),
            data_dir: Some(self.directory.path().join("nsh")),
            ui_sets: Some(vec![
                nsh_base::registry(),
                bindings.registry(initial_session),
                maka_terminal_shell::markdown::registry(),
            ]),
            ..Default::default()
        });
        view.load(Bundle::source(maka_terminal_shell::APPLICATION.into()).into())
            .unwrap();
        (view, bindings)
    }
    pub async fn finish(self) {
        self.host.shutdown().await.unwrap();
        self.server.await.unwrap().unwrap();
        if let Some(registration) = self.registration {
            registration.remove().unwrap();
        }
        self.provider.abort();
    }
}

async fn configure(owner: Arc<RootOwner>, base: &str, vision: bool) -> Value {
    use maka_agent_facts::configuration::CatalogMutationResult;
    let config = Arc::new(
        maka_config::ConfigurationStore::for_root(owner)
            .await
            .unwrap(),
    );
    let provider = json!({"packageId":"maka.providers","entryId":"maka.providers","scope":"profile","name":"openai-compatible"});
    let CatalogMutationResult::Committed {connection:Some(connection),..} = config.create_connection(serde_json::from_value(json!({
        "expectedCatalogRevision":0,"connection":{"slug":"nsh-fixture","name":"NSH fixture","provider":provider,"configuration":{"baseUrl":base},"enabled":true,"enabledModelIds":["fixture-model","fixture-model-alt"],"modelOverrides":{"fixture-model":{"contextWindow":200000,"vision":vision},"fixture-model-alt":{"contextWindow":200000,"vision":vision}}}
    })).unwrap()).await.unwrap() else { panic!("create fixture connection") };
    let maka_config::oauth::enrollment::LoginPreparation::Ready(login) = config.prepare_oauth_login(serde_json::from_value(json!({
        "expectedCredential":null,"attemptId":Uuid::new_v4(),"target":{"kind":"existing","expected":{"connectionId":connection.connection_id,"revision":connection.revision,"provider":provider,"slug":"nsh-fixture","configuration":{"baseUrl":base}},"configuration":{"baseUrl":base}},
        "authentication":{"method":"api-key","input":{"apiKey":"nsh-real-provider-key"}}
    })).unwrap()).await.unwrap() else { panic!("fixture credential") };
    assert!(login.claim().await.unwrap());
    assert!(matches!(
        login
            .complete(
                maka_agent_facts::provider::Credential {
                    secret: "nsh-real-provider-key".into(),
                    refresh_at: None
                },
                1
            )
            .await
            .unwrap(),
        maka_config::oauth::enrollment::LoginCompletion::Committed(_)
    ));
    assert!(matches!(config.set_default_target(serde_json::from_value(json!({"expectedCatalogRevision":config.catalog().await.unwrap().revision,"target":{"connectionId":connection.connection_id,"modelId":"fixture-model"}})).unwrap()).await.unwrap(), CatalogMutationResult::Committed {..}));
    let connection = config.catalog().await.unwrap().connections.remove(0);
    let selection = json!({"connectionId":connection.connection_id,"revision":connection.revision,"modelId":"fixture-model"});
    config.shutdown().await.unwrap();
    selection
}

pub async fn mutate(client: &Client, method: &str, input: Value) -> Value {
    let mutation = client
        .mutation(
            Uuid::new_v4(),
            "agent",
            method,
            input,
            Attachments::new(),
            None,
            Value::Null,
        )
        .await
        .unwrap();
    mutation.prepare().await.unwrap();
    let reply = mutation.confirm().await.unwrap();
    match reply.receipt {
        Receipt::Recorded {
            outcome: OperationOutcome::Returned { value },
            ..
        } => value,
        other => panic!("unexpected actual Host receipt: {other:?}"),
    }
}

pub struct ModelRequest {
    pub body: Value,
    socket: TcpStream,
}
impl ModelRequest {
    async fn read(mut socket: TcpStream) -> Self {
        let mut bytes = Vec::new();
        let boundary = loop {
            let mut buffer = [0; 4096];
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0);
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() < 2 * 1024 * 1024);
            if let Some(at) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break at + 4;
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..boundary]);
        assert!(
            headers.starts_with("POST /v1/chat/completions "),
            "{headers}"
        );
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer nsh-real-provider-key")
        );
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
        while bytes.len() < boundary + length {
            let mut buffer = [0; 4096];
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0);
            bytes.extend_from_slice(&buffer[..count]);
        }
        let body = serde_json::from_slice(&bytes[boundary..boundary + length]).unwrap();
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
        Self { body, socket }
    }
    async fn packet(&mut self, value: Value) {
        let data = format!("data: {value}\n\n");
        self.socket
            .write_all(format!("{:x}\r\n{data}\r\n", data.len()).as_bytes())
            .await
            .unwrap();
        self.socket.flush().await.unwrap();
    }
    pub async fn delta(&mut self, text: &str) {
        self.packet(json!({"id":"nsh-real-model","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]})).await;
    }
    pub async fn thinking(&mut self, text: &str) {
        self.packet(json!({"id":"nsh-real-model","choices":[{"index":0,"delta":{"reasoning_content":text},"finish_reason":null}]})).await;
    }

    pub async fn tool_prefix(&mut self, name: &str, input: &str) {
        self.packet(json!({"id":"nsh-real-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"nsh-native-todo-call","type":"function","function":{"name":name,"arguments":input}}]},"finish_reason":null}]})).await;
    }

    pub async fn tool_suffix(mut self, input: &str) {
        self.packet(json!({"id":"nsh-real-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":input}}]},"finish_reason":null}]})).await;
        self.finish_with_reason("tool_calls").await;
    }

    pub async fn tool(mut self, name: &str, input: Value) {
        self.tool_prefix(name, &input.to_string()).await;
        self.finish_with_reason("tool_calls").await;
    }

    pub async fn finish(self) {
        self.finish_with_reason("stop").await;
    }

    async fn finish_with_reason(mut self, reason: &str) {
        self.packet(json!({"id":"nsh-real-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}})).await;
        self.socket
            .write_all(b"e\r\ndata: [DONE]\n\n\r\n0\r\n\r\n")
            .await
            .unwrap();
        self.socket.flush().await.unwrap();
    }
}

// A public accessibility frame chooses coordinates; real native mouse events act.
pub fn click_text(view: &mut AppView, prefix: &str) {
    use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let node = view
        .accessibility_snapshot()
        .into_iter()
        .filter(|node| node.label.starts_with(prefix) && node.frame.origin.y.0 >= 0.0)
        .max_by(|a, b| a.frame.origin.y.0.total_cmp(&b.frame.origin.y.0))
        .unwrap_or_else(|| panic!("no rendered native text starts with {prefix:?}"));
    // Native input uses cell centers; a half-cell origin must not select its exclusive far edge.
    let column = (node.frame.origin.x.0 / 8.0 - 0.5).ceil().max(0.0) as u16;
    let row = (node.frame.origin.y.0 / 16.0 - 0.5).ceil().max(0.0) as u16;
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        view.handle_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }
}

// Hit rendered body text so nested scrolling is selected by the native pointer path.
pub fn scroll_text(view: &mut AppView, text: &str, kind: crossterm::event::MouseEventKind) {
    use crossterm::event::{Event, KeyModifiers, MouseEvent};
    let snapshot = view.snapshot();
    let (row, line) = snapshot
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(text))
        .unwrap_or_else(|| panic!("no rendered native text contains {text:?}:\n{snapshot}"));
    let column = line[..line.find(text).unwrap()].chars().count() as u16;
    view.handle_event(Event::Mouse(MouseEvent {
        kind,
        column,
        row: row as u16,
        modifiers: KeyModifiers::NONE,
    }));
}

// Call after pressing a row/header. Run the normal layout cycle before Home.
pub async fn focus_activity(view: &mut AppView) {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    view.handle_event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)));
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            view.poll().unwrap();
            if view
                .accessibility_snapshot()
                .iter()
                .any(|node| node.label == "Turn activity" && node.focused)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|error| {
        panic!(
            "Alt+Down did not focus Turn activity: {error}; app error: {:?}; focus: {:?}\n{}",
            view.last_error(),
            view.surface().focused(),
            view.snapshot()
        )
    });
}

pub async fn wait_view(view: &mut AppView, predicate: impl Fn(&str) -> bool) -> String {
    let started = Instant::now();
    loop {
        view.poll().unwrap();
        view.tick(started.elapsed().as_secs_f64()).unwrap();
        assert!(
            view.last_error().is_none(),
            "NSH app error: {:?}",
            view.last_error()
        );
        let snapshot = view.snapshot();
        if predicate(&snapshot) {
            return snapshot;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "expected real UI state did not appear:\n{snapshot}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

pub async fn wait_draft(client: &Client, session: &str, text: &str) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if client
                .load_draft("agent", session, EDITOR)
                .await
                .unwrap()
                .is_some_and(|draft| draft.text == text)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the editor did not persist its exact draft");
}

fn evidence_size() -> (u16, u16) {
    if std::env::var_os("MAKA_NSH_EVIDENCE").is_none() {
        return (120, 36);
    }
    match std::env::var("MAKA_NSH_EVIDENCE_SIZE").as_deref() {
        Ok("80x24") => (80, 24),
        Ok("120x36") | Err(_) => (120, 36),
        Ok("120x40") => (120, 40),
        Ok(size) => panic!("unsupported evidence size: {size}"),
    }
}

pub fn evidence_cells(name: &str, view: &AppView) {
    use ratatui::{buffer::CellWidth, layout::Rect, style::Color};
    let Some(directory) = std::env::var_os("MAKA_NSH_EVIDENCE") else {
        return;
    };
    let (columns, rows) = evidence_size();
    let area = Rect::new(0, 0, columns, rows);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    view.render(area, &mut buffer);
    let color = |value| match value {
        Color::Rgb(r, g, b) => json!({"rgb":[r,g,b]}),
        Color::Indexed(index) => json!({"indexed":index}),
        value => json!({"named":format!("{value:?}")}),
    };
    let cells: Vec<Vec<_>> = (0..rows).map(|y| (0..columns).map(|x| {
        let cell = &buffer[(x, y)];
        json!({"symbol":cell.symbol(),"width":cell.cell_width(),"fg":color(cell.fg),"bg":color(cell.bg),"modifier":cell.modifier.bits()})
    }).collect()).collect();
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(format!("{name}-{columns}x{rows}.cells.json")), serde_json::to_vec(&json!({
        "source":"NSH AppView::render / Ratatui Buffer","columns":columns,"rows":rows,"cellWidth":8,"cellHeight":16,"cells":cells
    })).unwrap()).unwrap();
}

pub fn evidence(name: &str, snapshot: &str) {
    if let Some(directory) = std::env::var_os("MAKA_NSH_EVIDENCE") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(format!("{name}.txt")), snapshot).unwrap();
    }
}

/// Withhold one real IPC result without manufacturing a Host fact or Agent effect.
pub struct ReplyGate {
    pub address: PathBuf,
    armed: Arc<AtomicBool>,
    pub held: Arc<Notify>,
    release: Arc<Notify>,
    worker: JoinHandle<()>,
}
impl ReplyGate {
    pub fn start(directory: &Path, target: PathBuf) -> Self {
        Self::for_method(directory, target, "send", "mutation")
    }
    #[cfg(unix)]
    pub fn history(directory: &Path, target: PathBuf) -> Self {
        Self::for_method(directory, target, "history", "finished")
    }
    fn for_method(
        directory: &Path,
        target: PathBuf,
        method: &'static str,
        reply: &'static str,
    ) -> Self {
        let address = socket(directory, "gate");
        let mut listener = LocalListener::bind(&address).unwrap();
        let armed = Arc::new(AtomicBool::new(false));
        let held = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let worker = tokio::spawn({
            let armed = armed.clone();
            let held = held.clone();
            let release = release.clone();
            async move {
                let mut connections = JoinSet::new();
                loop {
                    tokio::select! {
                        accepted=listener.accept()=> {
                            let downstream=accepted.unwrap(); let upstream=ipc::connect(&target).await.unwrap();
                            let armed=armed.clone(); let held=held.clone(); let release=release.clone();
                            connections.spawn(async move {
                                let (down_read,mut down_write)=tokio::io::split(downstream);
                                let (up_read,mut up_write)=tokio::io::split(upstream);
                                let selected=Arc::new(AtomicBool::new(false)); let mark=selected.clone();
                                let up=async move {let mut reader=wire::Reader::new(down_read); while let Ok(value)=reader.read::<Value>().await {
                                    if value["endpoint"]=="agent" && value["method"]==method {mark.store(true,Ordering::SeqCst);}
                                    if wire::write(&mut up_write,&value).await.is_err(){break;}
                                }
                                let _ = up_write.shutdown().await;
                                };
                                let down=async move {let mut reader=wire::Reader::new(up_read); while let Ok(value)=reader.read::<Value>().await {
                                    if selected.load(Ordering::SeqCst) && value["kind"]==reply && armed.swap(false,Ordering::SeqCst) {held.notify_one();release.notified().await;}
                                    if wire::write(&mut down_write,&value).await.is_err(){break;}
                                }
                                let _ = down_write.shutdown().await;
                                };
                                tokio::join!(up,down);
                            });
                        }
                        Some(result)=connections.join_next(),if !connections.is_empty()=>{result.unwrap();}
                    }
                }
            }
        });
        Self {
            address,
            armed,
            held,
            release,
            worker,
        }
    }
    pub fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
    pub fn release(&self) {
        self.release.notify_one();
    }
}
impl Drop for ReplyGate {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

fn socket(directory: &Path, name: &str) -> PathBuf {
    #[cfg(unix)]
    {
        directory.join(format!("{name}.sock"))
    }
    #[cfg(windows)]
    {
        let _ = directory;
        PathBuf::from(format!(r"\\.\pipe\maka-nsh-{name}-{}", Uuid::new_v4()))
    }
}
