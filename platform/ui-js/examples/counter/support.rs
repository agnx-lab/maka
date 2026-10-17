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

#[allow(dead_code)]
pub(crate) mod scene;

use maka_packages::MANIFEST_FILE;
use maka_packages::Package;
use maka_plugin_core::{
    composition::{
        Configuration, Environment, ExecutionLocation, Input, Node, PackageUse, PresentationTarget,
        Target,
    },
    contract::{Availability, Cardinality, Contract, Export, Import, Interface, Transport},
    package::{Declaration, EntryDeclaration, EntryTarget, Implementation, Realm},
};
use maka_ui_runtime::surface_contract;
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

pub(crate) fn node(package: &Package, key: u128) -> Node {
    Node {
        key: Uuid::from_u128(key),
        package: package.application_ref().into(),
        use_package: PackageUse::Entry,
        configuration: Configuration::default(),
        parameters: Configuration::default(),
        bindings: BTreeMap::new(),
        disabled: false,
        overrides: Vec::new(),
    }
}
pub(crate) fn package(id: &str, declaration: Declaration, source: Option<String>) -> Package {
    let mut files = BTreeMap::from([(
        MANIFEST_FILE.into(),
        serde_json::to_vec(&json!({"schemaVersion":1,"id":id,"application":declaration})).unwrap(),
    )]);
    if let Some(source) = source {
        files.insert("index.mjs".into(), source.into_bytes());
    }
    Package::new(files).unwrap()
}
pub fn fixture(handler: &str) -> (Input, Vec<Package>) {
    let surface = surface_contract(PresentationTarget::Tui);
    let provider = package(
        "native.surface",
        Declaration {
            entries: BTreeMap::from([(
                EntryTarget::Tui,
                EntryDeclaration {
                    scene: None,
                    implementation: Implementation::Native {
                        implementation: "maka.ui.surface.tui".into(),
                    },
                    configuration: json!({"type":"object"}),
                    imports: BTreeMap::new(),
                    exports: BTreeMap::from([(
                        "Surface".into(),
                        Export {
                            contract: surface.clone(),
                            availability: Availability::Required,
                        },
                    )]),
                },
            )]),
            composition: None,
        },
        None,
    );
    let component = Contract {
        name: "example.App".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: json!({"type":"object"}),
            events: BTreeMap::new(),
            captures: BTreeMap::new(),
        },
    };
    let source = format!(
        r#"
        const contract = {};
        export function activate(configuration) {{ return {{App:{{contract,kind:'component',component:['app']}}}}; }}
        function frame(state) {{ return {{actions:[],events:[],frame:{{revision:state.revision,state,surface:{{kind:'terminal',binding:'surface'}},view:{{kind:'stack',key:'root',direction:'column',padding:1,gap:1,background:1513239,children:[{{kind:'text',key:'count',text:String(state.count),color:16777215}},{{kind:'button',key:'inc',label:'Increment'}}]}}}}}}; }}
        export const app = {{mount(props){{ return frame({{count:props.count??0,revision:0}}); }},event(state,event){{ {} }},update(state,props){{ return frame({{...state,revision:state.revision+1}}); }} }};
    "#,
        serde_json::to_string(&component).unwrap(),
        handler
    );
    let app = package(
        "dynamic.app",
        Declaration {
            entries: BTreeMap::from([(
                EntryTarget::Tui,
                EntryDeclaration {
                    scene: Some(scene::language(PresentationTarget::Tui)),
                    implementation: Implementation::JavaScript {
                        module: "index.mjs".into(),
                        realm: Realm::Dedicated,
                    },
                    configuration: json!({"type":"object"}),
                    imports: BTreeMap::from([(
                        "surface".into(),
                        Import {
                            contract: surface,
                            cardinality: Cardinality::One,
                        },
                    )]),
                    exports: BTreeMap::from([(
                        "App".into(),
                        Export {
                            contract: component,
                            availability: Availability::Required,
                        },
                    )]),
                },
            )]),
            composition: None,
        },
        Some(source),
    );
    let mut root = node(&app, 2);
    root.bindings.insert(
        "surface".into(),
        vec![Target::Instance {
            path: vec!["surface".into()],
            export: "Surface".into(),
        }],
    );
    (
        Input {
            environment: Environment {
                location: ExecutionLocation::Ui,
                presentation: Some(PresentationTarget::Tui),
            },
            instances: BTreeMap::from([
                ("surface".into(), node(&provider, 1)),
                ("app".into(), root),
            ]),
            recovery: None,
            launch: Some(Target::Instance {
                path: vec!["app".into()],
                export: "App".into(),
            }),
        },
        vec![provider, app],
    )
}

pub(crate) fn composed_fixture(child_update: &str, parent_event: &str) -> (Input, Vec<Package>) {
    let (mut input, mut packages) = fixture("");
    let root = packages.pop().unwrap();
    let mut declaration = root
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    let root_contract = declaration.entries[&EntryTarget::Tui].exports["App"]
        .contract
        .clone();
    let counter_contract = Contract {
        name: "example.Counter".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: json!({"type":"object","properties":{"label":{"type":"string"},"initial":{"type":"integer"}},"required":["label","initial"],"additionalProperties":false}),
            events: BTreeMap::new(),
            captures: BTreeMap::new(),
        },
    };
    declaration
        .entries
        .get_mut(&EntryTarget::Tui)
        .unwrap()
        .imports
        .insert(
            "counter".into(),
            Import {
                contract: counter_contract.clone(),
                cardinality: Cardinality::One,
            },
        );
    let source = format!(
        r#"
        export function activate(){{return {{App:{{contract:{},kind:'component',component:['app']}}}};}}
        function frame(state){{
            const children=[{{kind:'component',key:'left',binding:'counter',member:0,on:{{}},props:{{label:'Left '+state.version,initial:10}}}}];
            if(state.right)children.push({{kind:'component',key:'right',binding:'counter',member:0,on:{{}},props:{{label:'Right '+state.version,initial:20}}}});
            return {{actions:[],events:[],frame:{{state,revision:state.revision,surface:{{kind:'terminal',binding:'surface'}},view:{{kind:'stack',key:'root',direction:'column',gap:8,padding:16,background:1513239,children:[{{kind:'stack',key:'counters',direction:'row',gap:16,padding:0,background:1513239,children}},{{kind:'button',key:'parent',label:'Change labels'}},{{kind:'button',key:'toggle',label:'Toggle right counter'}}]}}}}}};
        }}
        export const app={{mount(){{return frame({{revision:0,version:0,right:true}});}},event(state,event){{{}}},update(state,props){{return frame({{...state,revision:state.revision+1}});}}}};
    "#,
        serde_json::to_string(&root_contract).unwrap(),
        parent_event
    );
    let root = package("dynamic.app", declaration, Some(source));
    let source = format!(
        r#"
        export function activate(){{return {{Counter:{{contract:{},kind:'component',component:['counter']}}}};}}
        function frame(state){{return {{actions:[],events:[],frame:{{state,revision:state.revision,surface:null,view:{{kind:'stack',key:'counter',direction:'column',gap:8,padding:8,background:1513239,children:[{{kind:'text',key:'label',text:state.label,color:16777215}},{{kind:'text',key:'count',text:String(state.count),color:16777215}},{{kind:'button',key:'inc',label:'Increment '+state.label}}]}}}}}};}}
        export const counter={{mount(props){{return frame({{count:props.initial,label:props.label,revision:0}});}},event(state,event){{return frame({{...state,count:state.count+1,revision:state.revision+1}});}},update(state,props){{{}}}}};
    "#,
        serde_json::to_string(&counter_contract).unwrap(),
        child_update
    );
    let counter = package(
        "dynamic.counter",
        Declaration {
            entries: BTreeMap::from([(
                EntryTarget::Tui,
                EntryDeclaration {
                    scene: Some(scene::language(PresentationTarget::Tui)),
                    implementation: Implementation::JavaScript {
                        module: "index.mjs".into(),
                        realm: Realm::Dedicated,
                    },
                    configuration: json!({"type":"object"}),
                    imports: BTreeMap::new(),
                    exports: BTreeMap::from([(
                        "Counter".into(),
                        Export {
                            contract: counter_contract,
                            availability: Availability::Required,
                        },
                    )]),
                },
            )]),
            composition: None,
        },
        Some(source),
    );
    input.instances.get_mut("app").unwrap().package = root.application_ref().into();
    input.instances.get_mut("app").unwrap().bindings.insert(
        "counter".into(),
        vec![Target::Instance {
            path: vec!["counter".into()],
            export: "Counter".into(),
        }],
    );
    input.instances.insert("counter".into(), node(&counter, 3));
    packages.extend([root, counter]);
    (input, packages)
}

pub(crate) const CHILD_UPDATE: &str =
    "return frame({...state,label:props.label,revision:state.revision+1});";
pub(crate) const PARENT_EVENT: &str = "const next={...state,revision:state.revision+1};if(event.key==='toggle')next.right=!state.right;else next.version++;return frame(next);";

// These values belong to an ordinary fixture brand package, not the renderer.
pub(crate) fn brand_appearance() -> serde_json::Value {
    let text = json!({"font":{"family":".SystemUIFont","fallbacks":[],"size":16,"lineHeight":1.5,"weight":400},"foreground":0x242724ffu32});
    let thumb = json!({"color":0x92998cffu32,"width":8,"inset":3,"radius":3,"minLength":40});
    json!({"label":text,"menu":{
        "appearance":{"text":text,"background":0xfaf9f6ffu32,"border":0xd8ddd5ffu32,
            "activeBackground":0x416653ffu32,"activeForeground":0xffffffffu32,
            "disabledForeground":0x92998cffu32,"borderWidth":1,"radius":10,
            "padding":{"top":4,"right":4,"bottom":4,"left":4},
            "rowPadding":{"top":7,"right":12,"bottom":7,"left":12},
            "width":220,"windowMargin":8},
        "labels":{"title":"Edit draft","undo":"Undo","redo":"Redo","cut":"Cut",
            "copy":"Copy","paste":"Paste","selectAll":"Select all"}
    },"textarea":{
        "text":text,"mutedForeground":0x656a63ffu32,"background":0xfaf9f6ffu32,
        "selection":0x41665340u32,"caret":0x242724ffu32,"border":0xd8ddd5ffu32,
        "focusedBorder":0x416653ffu32,"borderWidth":1,"radius":12,
        "padding":{"top":12,"right":12,"bottom":12,"left":12},
        "scrollbar":{"mode":"hover","entrance":"fade","idleMillis":2000,
            "enterMillis":120,"exitMillis":150,"expandMillis":120,"trackWidth":14,
            "track":0,"trackHover":0,"trackActive":0,
            "thumb":thumb,"thumbHover":thumb,"thumbActive":thumb}
    }})
}
fn bind_brand(input: &mut Input, packages: &mut Vec<Package>, declaration: &mut Declaration) {
    let mut generator = schemars::SchemaGenerator::default();
    let label = generator.subschema_for::<maka_ui_runtime::appearance::TextAppearance>();
    let textarea = generator.subschema_for::<maka_ui_runtime::appearance::TextareaAppearance>();
    let menu = generator.subschema_for::<maka_ui_runtime::textarea::TextareaMenu>();
    let schema = json!({"type":"object","properties":{"label":label,"textarea":textarea,"menu":menu},
        "required":["label","textarea","menu"],"additionalProperties":false,"$defs":generator.definitions()});
    let contract = Contract {
        name: "example.brand.Appearance".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Value {
            schema: schema.clone(),
        },
    };
    let brand = package(
        "dynamic.brand",
        Declaration {
            entries: BTreeMap::from([(
                EntryTarget::Tui,
                EntryDeclaration {
                    scene: None,
                    implementation: Implementation::JavaScript {
                        module: "index.mjs".into(),
                        realm: Realm::Shared,
                    },
                    configuration: schema,
                    imports: BTreeMap::new(),
                    exports: BTreeMap::from([(
                        "Appearance".into(),
                        Export {
                            contract: contract.clone(),
                            availability: Availability::Required,
                        },
                    )]),
                },
            )]),
            composition: None,
        },
        Some(format!(
            "const contract={};export function activate(configuration){{return {{Appearance:{{kind:'value',contract,value:configuration}}}};}}",
            serde_json::to_string(&contract).unwrap()
        )),
    );
    declaration
        .entries
        .get_mut(&EntryTarget::Tui)
        .unwrap()
        .imports
        .insert(
            "brand".into(),
            Import {
                contract,
                cardinality: Cardinality::One,
            },
        );
    input.instances.get_mut("app").unwrap().bindings.insert(
        "brand".into(),
        vec![Target::Instance {
            path: vec!["brand".into()],
            export: "Appearance".into(),
        }],
    );
    let mut instance = node(&brand, 990);
    instance.configuration.values = serde_json::from_value(brand_appearance()).unwrap();
    input.instances.insert("brand".into(), instance);
    packages.push(brand);
}

/// Mixed JS/native composition. The package selects a compiled implementation;
/// placing, updating and removing it remains ordinary product component code.
pub(crate) fn native_fixture() -> (Input, Vec<Package>) {
    let (mut input, mut packages) = composed_fixture(CHILD_UPDATE, PARENT_EVENT);
    let index = packages
        .iter()
        .position(|package| package.manifest().id == "dynamic.app")
        .unwrap();
    let root = packages.remove(index);
    let mut declaration = root
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    bind_brand(&mut input, &mut packages, &mut declaration);
    let contract = maka_ui_runtime::label_contract();
    declaration
        .entries
        .get_mut(&EntryTarget::Tui)
        .unwrap()
        .imports
        .insert(
            "label".into(),
            Import {
                contract: contract.clone(),
                cardinality: Cardinality::One,
            },
        );
    let source = std::str::from_utf8(root.file("index.mjs").unwrap()).unwrap();
    let source = source.replace("children:[{kind:'stack',key:'counters'", "children:[...(state.right?[{kind:'component',key:'nativeLabel',binding:'label',member:0,on:{},props:{text:'Native '+state.version,appearance:state.brand.label}}]:[]),{kind:'stack',key:'counters'");
    let source = source
        .replace("mount(){return frame({revision:0,version:0,right:true});}", "mount(props,values){return frame({revision:0,version:0,right:true,brand:values.brand[0]});}")
        .replace("update(state,props){return frame({...state,revision:state.revision+1});}", "update(state,props,values){return frame({...state,revision:state.revision+1,brand:values.brand[0]});}");
    let root = package("dynamic.app", declaration, Some(source));
    let label = package(
        "native.label",
        Declaration {
            entries: BTreeMap::from([(
                EntryTarget::Tui,
                EntryDeclaration {
                    scene: None,
                    implementation: Implementation::Native {
                        implementation: "maka.ui.label".into(),
                    },
                    configuration: json!({"type":"object"}),
                    imports: BTreeMap::new(),
                    exports: BTreeMap::from([(
                        "Label".into(),
                        Export {
                            contract,
                            availability: Availability::Required,
                        },
                    )]),
                },
            )]),
            composition: None,
        },
        None,
    );
    let app = input.instances.get_mut("app").unwrap();
    app.package = root.application_ref().into();
    app.bindings.insert(
        "label".into(),
        vec![Target::Instance {
            path: vec!["label".into()],
            export: "Label".into(),
        }],
    );
    input.instances.insert("label".into(), node(&label, 4));
    packages.extend([root, label]);
    (input, packages)
}

pub(crate) fn editor_fixture() -> (Input, Vec<Package>) {
    let (mut input, mut packages) = native_fixture();
    let index = packages
        .iter()
        .position(|package| package.manifest().id == "dynamic.app")
        .unwrap();
    let root = packages.remove(index);
    let mut declaration = root
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    let contract = maka_ui_runtime::textarea::textarea_contract();
    declaration
        .entries
        .get_mut(&EntryTarget::Tui)
        .unwrap()
        .imports
        .insert(
            "editor".into(),
            Import {
                contract: contract.clone(),
                cardinality: Cardinality::One,
            },
        );
    let source = std::str::from_utf8(root.file("index.mjs").unwrap()).unwrap();
    let source = source.replace("...(state.right?[{kind:'component',key:'nativeLabel'", "...(state.right?[{kind:'component',key:'editor',binding:'editor',member:0,on:{},props:{initialText:'Start',placeholder:'Write here',appearance:state.brand.textarea,menu:state.brand.menu,accessibilityLabel:'Write a draft',readOnly:false,disabled:false,softWrap:true,tabIndex:0,command:state.command??null}},{kind:'component',key:'nativeLabel'");
    let source = source.replace("const next={...state,revision:state.revision+1};", "const next={...state,revision:state.revision+1};if(event.event?.command)next.command=event.event.command;");
    let source = source.replace(
        "title:'Composed product',width:640,height:400",
        "title:'Native editor product',width:640,height:600",
    );
    let root = package("dynamic.app", declaration, Some(source));
    let editor = package(
        "native.editor",
        Declaration {
            entries: BTreeMap::from([(
                EntryTarget::Tui,
                EntryDeclaration {
                    scene: None,
                    implementation: Implementation::Native {
                        implementation: "maka.ui.textarea".into(),
                    },
                    configuration: json!({"type":"object"}),
                    imports: BTreeMap::new(),
                    exports: BTreeMap::from([(
                        "Textarea".into(),
                        Export {
                            contract,
                            availability: Availability::Required,
                        },
                    )]),
                },
            )]),
            composition: None,
        },
        None,
    );
    let app = input.instances.get_mut("app").unwrap();
    app.package = root.application_ref().into();
    app.bindings.insert(
        "editor".into(),
        vec![Target::Instance {
            path: vec!["editor".into()],
            export: "Textarea".into(),
        }],
    );
    input.instances.insert("editor".into(), node(&editor, 5));
    packages.extend([root, editor]);
    (input, packages)
}

/// A product component subscribes to the Textarea's public metadata event. No
/// global lookup or text-resource read is granted by receiving this payload.
#[allow(dead_code)] // Shared with tests that do not load the terminal editor.
pub(crate) const EDITOR_RECEIVE: &str = "if(event.kind==='component'&&event.name==='changed'){return frame({...state,revision:state.revision+1,observation:event.event});}";
#[allow(dead_code)] // Shared with tests that do not load the terminal editor.
pub(crate) fn editor_events_fixture() -> (Input, Vec<Package>) {
    let (mut input, mut packages) = editor_fixture();
    let index = packages
        .iter()
        .position(|package| package.manifest().id == "dynamic.app")
        .unwrap();
    let old = packages.remove(index);
    let source=std::str::from_utf8(old.file("index.mjs").unwrap()).unwrap()
        .replace("on:{},props:{initialText:","on:{changed:'editor_changed'},props:{initialText:")
        .replace("const next={...state,revision:state.revision+1};",&format!("{EDITOR_RECEIVE}const next={{...state,revision:state.revision+1}};"))
        .replace("children:[...(state.right?", "children:[{kind:'text',key:'editor_status',text:'Draft revision '+(state.observation?.revision??0),color:16777215},...(state.right?");
    let root = package(
        "dynamic.app",
        old.manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone(),
        Some(source),
    );
    input.instances.get_mut("app").unwrap().package = root.application_ref().into();
    packages.push(root);
    (input, packages)
}

/// Root consumes the public Composer contract; its private editor slot is only
/// known by that wrapper. All resource authority stays in the request sidecar.
pub(crate) fn resource_fixture() -> (Input, Vec<Package>) {
    let (mut input, mut packages) = editor_fixture();
    let root_index = packages
        .iter()
        .position(|p| p.manifest().id == "dynamic.app")
        .unwrap();
    let old = packages.remove(root_index);
    let mut declaration = old
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    let entry = declaration.entries.get_mut(&EntryTarget::Tui).unwrap();
    entry.imports.remove("counter");
    entry.imports.remove("label");
    entry.imports.insert(
        "reader".into(),
        Import {
            contract: maka_ui_runtime::text::reader_contract(),
            cardinality: Cardinality::One,
        },
    );
    let root_source = format!(
        r#"
    const contract={};
    export function activate(){{return {{App:{{contract,kind:'component',component:['app']}}}};}}
    function frame(state){{return {{actions:[],events:[],frame:{{revision:state.revision,state,surface:{{kind:'terminal',binding:'surface'}},view:{{kind:'stack',key:'root',direction:'column',padding:16,gap:12,background:1513239,children:[{{kind:'text',key:'status',text:'Captured: '+(state.outcome?.value?.text??state.outcome?.status??'—'),color:16777215}},...(state.show?[{{kind:'component',key:'editor',binding:'editor',member:0,on:{{changed:'changed'}},props:{{initialText:'Start',placeholder:'Write here',appearance:state.brand.textarea,menu:state.brand.menu,accessibilityLabel:'Write a draft',readOnly:false,disabled:false,softWrap:true,tabIndex:0,command:state.command??null}}}}]:[]),{{kind:'button',key:'read',label:'Capture draft'}},{{kind:'button',key:'remove',label:'Remove composer'}}]}}}}}};}}
    export const app={{mount(props,values){{return frame({{revision:0,show:true,brand:values.brand[0]}});}},update(state,props,values){{return frame({{...state,revision:state.revision+1,brand:values.brand[0]}});}},event(state,event){{
        const next={{...state,revision:state.revision+1}};
        if(event.kind==='action')next.outcome=event.event;
        else if(event.kind==='component')next.observation=event.event;
        else if(event.key==='remove')next.show=false;
        else if(event.key==='read'){{if(event.event?.command)next.command=event.event.command;const out=frame(next);out.actions.push({{key:'read_done',binding:'reader',member:0,method:'read',input:{{start:event.event?.start??0,end:event.event?.end??5}},resources:{{text:{{child:'editor',method:'snapshot',input:event.event?.capture??{{kind:'current'}}}}}}}});return out;}}
        return frame(next);
    }}}};
    "#,
        serde_json::to_string(&entry.exports["App"].contract).unwrap()
    );
    let root = package("dynamic.app", declaration, Some(root_source));
    let editor = maka_ui_runtime::textarea::textarea_contract();
    let composer = package(
        "dynamic.composer",
        Declaration {
            composition: None,
            entries: BTreeMap::from([(
                EntryTarget::Tui,
                EntryDeclaration {
                    scene: Some(scene::language(PresentationTarget::Tui)),
                    implementation: Implementation::JavaScript {
                        module: "index.mjs".into(),
                        realm: Realm::Shared,
                    },
                    configuration: json!({"type":"object"}),
                    imports: BTreeMap::from([(
                        "editor".into(),
                        Import {
                            contract: editor.clone(),
                            cardinality: Cardinality::One,
                        },
                    )]),
                    exports: BTreeMap::from([(
                        "Textarea".into(),
                        Export {
                            contract: editor.clone(),
                            availability: Availability::Required,
                        },
                    )]),
                },
            )]),
        },
        Some(format!(
            r#"
        const contract={};export function activate(){{return {{Textarea:{{contract,kind:'component',component:['composer']}}}};}}
        function frame(state,props,events=[]){{return {{actions:[],events,frame:{{revision:state.revision,state,surface:null,view:{{kind:'component',key:'draft',binding:'editor',member:0,props,on:{{changed:'changed'}}}}}}}};}}
        export const composer={{mount(props){{return frame({{revision:0}},props);}},update(state,props){{return frame({{...state,revision:state.revision+1}},props);}},event(state,event,props){{return frame({{revision:state.revision+1}},props,[{{name:'changed',value:event.event}}]);}},capture(state,props,method,input){{return {{kind:'forward',source:{{child:'draft',method,input}}}};}}}};
    "#,
            serde_json::to_string(&editor).unwrap()
        )),
    );
    let reader = package(
        "native.reader",
        Declaration {
            composition: None,
            entries: BTreeMap::from([(
                EntryTarget::Tui,
                EntryDeclaration {
                    scene: None,
                    implementation: Implementation::Native {
                        implementation: "maka.text.reader".into(),
                    },
                    configuration: json!({"type":"object"}),
                    imports: BTreeMap::new(),
                    exports: BTreeMap::from([(
                        "Reader".into(),
                        Export {
                            contract: maka_ui_runtime::text::reader_contract(),
                            availability: Availability::Required,
                        },
                    )]),
                },
            )]),
        },
        None,
    );
    let root_node = input.instances.get_mut("app").unwrap();
    root_node.package = root.application_ref().into();
    root_node.bindings.remove("counter");
    root_node.bindings.remove("label");
    root_node.bindings.insert(
        "editor".into(),
        vec![Target::Instance {
            path: vec!["composer".into()],
            export: "Textarea".into(),
        }],
    );
    root_node.bindings.insert(
        "reader".into(),
        vec![Target::Instance {
            path: vec!["reader".into()],
            export: "Reader".into(),
        }],
    );
    let mut composer_node = node(&composer, 6);
    composer_node.bindings.insert(
        "editor".into(),
        vec![Target::Instance {
            path: vec!["editor".into()],
            export: "Textarea".into(),
        }],
    );
    input.instances.insert("composer".into(), composer_node);
    input.instances.insert("reader".into(), node(&reader, 7));
    input.instances.remove("counter");
    input.instances.remove("label");
    packages.retain(|p| !matches!(p.manifest().id.as_str(), "dynamic.counter" | "native.label"));
    packages.extend([root, composer, reader]);
    (input, packages)
}

/// Real wrapper/native capture declarations shared by continuation regressions.
// The example and other fixture consumers select different subsets.
#[allow(dead_code)]
pub(crate) fn continuation_fixture() -> (Input, Vec<Package>, maka_plugin_core::contract::Contract)
{
    let (mut input, mut packages) = resource_fixture();
    let mut contract = maka_ui_runtime::textarea::textarea_contract();
    let original = serde_json::to_string(&contract).unwrap();
    let maka_plugin_core::contract::Interface::Component { captures, .. } = &mut contract.interface
    else {
        unreachable!()
    };
    captures.insert(
        "reading".into(),
        maka_plugin_core::contract::Capture {
            input: json!({"type":"null"}),
            output: maka_plugin_core::contract::CaptureOutput::Continuation,
        },
    );
    let changed = serde_json::to_string(&contract).unwrap();
    for package in &mut packages {
        let mut declaration = package
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone();
        let mut updated = false;
        for entry in declaration.entries.values_mut() {
            for import in entry.imports.values_mut() {
                if import.contract.name == contract.name {
                    import.contract = contract.clone();
                    updated = true;
                }
            }
            for export in entry.exports.values_mut() {
                if export.contract.name == contract.name {
                    export.contract = contract.clone();
                    updated = true;
                }
            }
        }
        let source = package.file("index.mjs").map(|bytes| {
                let mut source = std::str::from_utf8(bytes).unwrap().replace(&original, &changed);
                if package.manifest().id == "dynamic.app" {
                    updated = true;
                    source = source.replace("out.actions.push({key:'read_done'", "out.actions.push({continuations:[{child:'editor',method:'reading',input:null}],key:event.event?.key??'read_done'")
                        .replace("if(event.kind==='action')next.outcome=event.event;", "if(event.kind==='action'){next.outcome=event.event;next.results=[...(state.results??[]),{key:event.key,outcome:event.event}];}")
                        .replace("else if(event.key==='remove')next.show=false;", "else if(event.key==='remove')next.show=false;else if(event.key==='restore')next.show=true;")
                        .replace("{kind:'button',key:'remove',label:'Remove composer'}", "{kind:'button',key:'remove',label:'Remove composer'},{kind:'button',key:'restore',label:'Restore composer'}");
                }
                source
            });
        if updated {
            let old = package.application_ref();
            let replacement = self::package(&package.manifest().id, declaration, source);
            for instance in input.instances.values_mut() {
                if instance.package == old.clone().into() {
                    instance.package = replacement.application_ref().into();
                }
            }
            *package = replacement;
        }
    }
    (input, packages, contract)
}

/// A layout/appearance fixture, not a completed agent product. All native
/// children remain ordinary bindings of the original JavaScript parent.
#[allow(dead_code)] // Shared support is also compiled by other tests.
pub(crate) fn workspace_fixture() -> (Input, Vec<Package>) {
    let (mut input, mut packages) = editor_fixture();
    let index = packages
        .iter()
        .position(|p| p.manifest().id == "dynamic.app")
        .unwrap();
    let original = packages.remove(index);
    let mut source = std::str::from_utf8(original.file("index.mjs").unwrap())
        .unwrap()
        .to_owned();
    source.push_str(r#"
const previousFrame=frame;
const fill={unit:'fraction',value:1}, pixels=(value)=>({unit:'pixels',value});
const text=(key,label,size=15,weight=400,foreground=0x242724ff)=>({kind:'text',key,text:label,color:0x242724,style:{text:{font:{family:'.SystemUIFont',fallbacks:[],size,lineHeight:1.55,weight},foreground}}});
const box=(key,children,style={},direction='column',gap=12,padding=0)=>({kind:'stack',key,direction,gap,padding,background:0xfaf9f6,style,children});
const control=(key,label)=>({kind:'button',key,label,style:{text:{font:{family:'.SystemUIFont',fallbacks:[],size:13,lineHeight:1.4,weight:500},foreground:0x416653ff},padding:{top:8,right:12,bottom:8,left:12},radius:8,border:0xd8ddd5ff,borderWidth:1},hover:{background:0xe8ede5ff},focus:{border:0x416653ff,borderWidth:2},active:{background:0xd8e3d4ff}});
frame=(state)=>{
    const output=previousFrame(state);
    const components={};
    const visit=(node)=>{if(node.kind==='component')components[node.key]=node;for(const child of node.children??[])visit(child);};
    visit(output.frame.view);
    const editor=components.editor??{kind:'component',key:'editor',binding:'editor',member:0,on:{},props:{appearance:state.brand.textarea,menu:state.brand.menu,readOnly:false,disabled:false,softWrap:true,tabIndex:0,command:state.command??null}};
    editor.props={...editor.props,initialText:'',placeholder:'Describe what you would like to work on…',accessibilityLabel:'Compose a draft'};
    const context=[components.nativeLabel,components.left,components.right].filter(Boolean);
    for(const item of context){if(item.key==='nativeLabel')item.props.text='Native view · bound component';}
    output.frame.surface={...output.frame.surface,title:'Maka · Plugin workspace',width:1100,height:780};
    output.frame.view=box('workspace',[
        box('navigation',[
            text('wordmark','maka',24,650),
            text('workspace-label','PLUGIN WORKSPACE',11,600,0x656a63ff),
            text('active-context','A place to think',15,500),
            text('navigation-note','Your tools. Your workflow.\nYour own composition.',13,400,0x656a63ff),
            box('navigation-space',[],{grow:1,background:0}),
            text('runtime-label','Local presentation',12,400,0x656a63ff)
        ],{width:pixels(220),height:fill,shrink:0,background:0xf2f3eeff,padding:{top:28,right:24,bottom:24,left:24}},'column',24),
        box('main',[
            box('header',[text('breadcrumb','Workspace  /  Draft',13,400,0x656a63ff),control('toggle','Toggle context')],{width:fill,justify:'between',align:'center'},'row',16),
            box('conversation',[
                text('welcome-title','Make room for the next idea.',30,600),
                text('welcome-copy','A calm workspace, composed by plugins.\nLayout, appearance and native editing can change independently.',16,400,0x656a63ff),
                box('details',[text('details-heading','Built around your work',15,550),text('details-copy','The composer keeps its draft, focus and selection while the surrounding layout changes.',14,400,0x656a63ff)],{width:fill,background:0xf2f3eeff,radius:12,padding:{top:20,right:24,bottom:20,left:24}})
            ],{width:fill,maxWidth:720,grow:1,justify:'center'},'column',20),
            ...(editor?[box('composer',[editor],{width:fill,height:pixels(180),shrink:0})]:[]),
            box('composer-footer',[text('draft-note','Draft stays on this device',12,400,0x656a63ff),control('parent','Update context')],{width:fill,justify:'between',align:'center'},'row',16)
        ],{grow:1,shrink:1,height:fill,minWidth:360,padding:{top:28,right:40,bottom:28,left:40}},'column',24),
        ...(state.right?[box('inspector',[text('inspector-title','Context',15,600),text('inspector-description','Independent component bindings',12,400,0x656a63ff),...context],{width:pixels(220),height:fill,shrink:0,background:0xf2f3eeff,padding:{top:28,right:20,bottom:24,left:20}},'column',16)]:[])
    ],{width:fill,height:fill,text:state.brand.label},'row',0);
    return output;
};
"#);
    // The counter fixture also authors its own appearance; it is deliberately
    // transformed in its package, rather than overwritten by the renderer.
    let counter_index = packages
        .iter()
        .position(|p| p.manifest().id == "dynamic.counter")
        .unwrap();
    let counter = packages.remove(counter_index);
    let code = std::str::from_utf8(counter.file("index.mjs").unwrap())
        .unwrap()
        .replace("background:1513239", "background:0xf2f3ee")
        .replace("color:16777215", "color:0x242724");
    let counter = package(
        "dynamic.counter",
        counter
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone(),
        Some(code),
    );
    input.instances.get_mut("counter").unwrap().package = counter.application_ref().into();
    packages.push(counter);
    let root = package(
        "dynamic.app",
        original
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone(),
        Some(source),
    );
    input.instances.get_mut("app").unwrap().package = root.application_ref().into();
    packages.push(root);
    (input, packages)
}

#[allow(dead_code)] // Shared support is also compiled by tests without recovery.
pub fn add_recovery(input: &mut Input, packages: &mut Vec<Package>) {
    let (source, recovery_packages) =
        fixture("return frame({count:state.count+1,revision:state.revision+1});");
    let mut root = source.instances["app"].clone();
    root.key = Uuid::from_u128(99);
    input.instances.insert("recovery".into(), root);
    input.recovery = Some(Target::Instance {
        path: vec!["recovery".into()],
        export: "App".into(),
    });
    for package in recovery_packages {
        if !packages
            .iter()
            .any(|existing| existing.application_ref() == package.application_ref())
        {
            packages.push(package);
        }
    }
}
