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

use super::Application;
use super::{support, timer};
use maka_packages::Package;
use maka_plugin_core::{
    composition::{Input, PresentationTarget, Target},
    contract::{Availability, Cardinality, Contract, Export, Import, Interface, Transport},
    package::{Declaration, EntryDeclaration, EntryTarget, Implementation},
};
use maka_ui_runtime::{NativeCapability, NativeExport, NativeRegistry};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};

fn contract() -> Contract {
    Contract {
        name: "test.Palette".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Value {
            schema: json!({"type":["string","integer"]}).into(),
        },
    }
}
fn fixture(
    value: Value,
    javascript: bool,
    duplicate: bool,
) -> (Input, Vec<Package>, NativeRegistry<()>) {
    let (mut input, mut packages) = support::fixture(
        "return frame({revision:state.revision+1,count:++globalThis.calls,size:values.brand[0].length??0});",
    );
    let old = packages.pop().unwrap();
    let mut declaration = old
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    let entry = declaration.entries.get_mut(&EntryTarget::Tui).unwrap();
    for name in if duplicate {
        vec!["brand", "second"]
    } else {
        vec!["brand"]
    } {
        entry.imports.insert(
            name.into(),
            Import {
                contract: contract(),
                cardinality: Cardinality::One,
            },
        );
        input.instances.get_mut("app").unwrap().bindings.insert(
            name.into(),
            vec![Target::Instance {
                path: vec!["brand".into()],
                export: "Palette".into(),
            }],
        );
    }
    entry.imports.insert(
        "absent".into(),
        Import {
            contract: contract(),
            cardinality: Cardinality::Optional,
        },
    );
    input
        .instances
        .get_mut("app")
        .unwrap()
        .bindings
        .insert("absent".into(), vec![]);
    let source=std::str::from_utf8(old.file("index.mjs").unwrap()).unwrap()
        .replace("activate(configuration)","activate(configuration,values)")
        .replace("return {App:","globalThis.calls=0;return {App:")
        .replace("mount(props)","mount(props,values)")
        .replace("event(state,event)","event(state,event,props,values)")
        .replace("count:props.count??0,revision:0","count:++globalThis.calls,revision:0,size:values.brand[0].length??0,empty:values.absent,second:values.second?.[0]===values.brand[0]");
    let app = support::package("value.app", declaration, Some(source));
    input.instances.get_mut("app").unwrap().package = app.application_ref().into();
    let export = Export {
        contract: contract(),
        availability: Availability::Required,
    };
    let implementation = if javascript {
        Implementation::JavaScript {
            module: "index.mjs".into(),
            realm: maka_plugin_core::package::Realm::Dedicated,
        }
    } else {
        Implementation::Native {
            implementation: "test.palette".into(),
        }
    };
    let source=javascript.then(||format!("export function activate(){{return {{Palette:{{kind:'value',contract:{},value:{}}}}};}}",serde_json::to_string(&contract()).unwrap(),serde_json::to_string(&value).unwrap()));
    let provider = support::package(
        "value.brand",
        Declaration {
            entries: BTreeMap::from([(
                EntryTarget::Tui,
                EntryDeclaration {
                    scene: None,
                    implementation,
                    configuration: json!({"type":"object"}).into(),
                    imports: BTreeMap::new(),
                    exports: BTreeMap::from([("Palette".into(), export)]),
                },
            )]),
            composition: None,
        },
        source,
    );
    input
        .instances
        .insert("brand".into(), support::node(&provider, 99));
    packages.extend([provider, app]);
    let mut native = crate::support::scene::registry(PresentationTarget::Tui);
    native
        .register("test.palette", move |_, _| {
            Ok(BTreeMap::from([(
                "Palette".into(),
                NativeExport {
                    contract: contract(),
                    value: NativeCapability::Value(value.clone()),
                },
            )]))
        })
        .unwrap();
    (input, packages, native)
}
fn start(
    input: &Input,
    packages: Vec<Package>,
    native: NativeRegistry<()>,
    timer: &tokio::runtime::Runtime,
) -> maka_ui_runtime::Result<Application> {
    Application::new(
        input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(maka_js_runtime::plugin::Limits {
            synchronous_slice: Duration::from_millis(200),
            ..Default::default()
        }),
        native,
        Default::default(),
    )
}
#[test]
fn native_and_js_value_exports_feed_actual_callbacks_and_empty_optional_members() {
    let timer = timer();
    for javascript in [false, true] {
        let (input, packages, native) = fixture(json!("sage"), javascript, true);
        let mut app = start(&input, packages, native, &timer).unwrap();
        let mount = app.mount_launch(json!({})).unwrap();
        let state = &app.frame(&mount).unwrap().state;
        assert_eq!(state["size"], 4);
        assert_eq!(state["empty"], json!([]));
        assert_eq!(state["second"], true);
        app.dispatch(&mount, 0, "inc", Value::Null).unwrap();
        assert_eq!(app.frame(&mount).unwrap().state["count"], 2);
        app.close(&mount, Ok(())).unwrap();
        app.shutdown().unwrap();
    }
}

#[test]
fn value_schema_and_native_to_js_precision_are_checked_before_activation() {
    let timer = timer();
    for (value, javascript) in [
        (json!({"wrong":"shape"}), false),
        (json!({"wrong":"shape"}), true),
        (json!(9_007_199_254_740_993u64), false),
    ] {
        let (input, packages, native) = fixture(value, javascript, false);
        assert!(start(&input, packages, native, &timer).is_err());
    }
}

struct Exact;
impl maka_ui_runtime::NativeComponent<()> for Exact {
    fn call(
        &self,
        call: maka_ui_runtime::ComponentCall<'_>,
        values: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<maka_ui_runtime::ComponentOutput<maka_ui_runtime::NativeView<()>>>
    {
        assert!(matches!(call, maka_ui_runtime::ComponentCall::Mount { .. }));
        Ok(maka_ui_runtime::Frame {
            revision: 0,
            state: json!({"exact":values.get("brand").unwrap()[0]}),
            surface: Some(maka_ui_runtime::Surface::Terminal {
                binding: "surface".into(),
            }),
            view: crate::support::scene::view(crate::support::scene::Node::Text {
                key: "value".into(),
                text: "Exact".into(),
                color: 0,
            })?,
        }
        .output(vec![]))
    }
}
#[test]
fn native_activation_and_callback_preserve_exact_integers_without_a_js_roundtrip() {
    let timer = timer();
    let (mut input, mut packages, mut native) =
        fixture(json!(9_007_199_254_740_993u64), false, false);
    let old = packages.pop().unwrap();
    let mut declaration = old
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    let entry = declaration.entries.get_mut(&EntryTarget::Tui).unwrap();
    entry.scene = None;
    entry.implementation = Implementation::Native {
        implementation: "test.exact-value".into(),
    };
    let contract = entry.exports["App"].contract.clone();
    let app = support::package("native.exact", declaration, None);
    input.instances.get_mut("app").unwrap().package = app.application_ref().into();
    packages.push(app);
    native
        .register("test.exact-value", move |_, values| {
            assert_eq!(
                values.get("brand").unwrap()[0].as_u64(),
                Some(9_007_199_254_740_993)
            );
            Ok(BTreeMap::from([(
                "App".into(),
                NativeExport {
                    contract: contract.clone(),
                    value: NativeCapability::Component(std::rc::Rc::new(Exact)),
                },
            )]))
        })
        .unwrap();
    let mut app = start(&input, packages, native, &timer).unwrap();
    let root = app.mount_launch(json!({})).unwrap();
    assert_eq!(
        app.frame(&root).unwrap().state["exact"].as_u64(),
        Some(9_007_199_254_740_993)
    );
    app.close(&root, Ok(())).unwrap();
    app.shutdown().unwrap();
}
