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
use crate::{
    composition::{Environment, ExecutionLocation, Parameter, PresentationTarget, Template},
    contract::{Cardinality, Contract},
    package::{Declaration, Implementation, Realm},
};
use serde_json::json;
use uuid::Uuid;

fn package(id: &str, revision: u8) -> PackageRef {
    PackageRef {
        id: id.into(),
        revision: format!("sha256-{revision:064x}"),
    }
}

fn contract() -> Contract {
    Contract {
        name: "example.Content".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: json!({"type":"object"}),
            events: BTreeMap::new(),
            captures: BTreeMap::new(),
        },
    }
}

fn entry() -> EntryDeclaration {
    EntryDeclaration {
        scene: Some(crate::package::SceneLanguage {
            name: "test.scene".into(),
            version: 1,
        }),
        implementation: Implementation::JavaScript {
            module: "index.mjs".into(),
            realm: Realm::Dedicated,
        },
        configuration: json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"],"additionalProperties":false}),
        imports: BTreeMap::new(),
        exports: BTreeMap::from([(
            "Content".into(),
            Export {
                contract: contract(),
                availability: Availability::Required,
            },
        )]),
    }
}

fn node(key: u128, package: PackageRef, use_package: PackageUse) -> Node {
    Node {
        key: Uuid::from_u128(key),
        package: package.into(),
        use_package,
        configuration: Configuration::default(),
        parameters: Configuration::default(),
        bindings: BTreeMap::new(),
        disabled: false,
        overrides: Vec::new(),
    }
}

fn local(path: &[&str], export: &str) -> Target {
    Target::Instance {
        path: path.iter().map(|part| (*part).into()).collect(),
        export: export.into(),
    }
}

fn template(default: &str) -> Template {
    let mut content = node(10, package("content", 1), PackageUse::Entry);
    content
        .configuration
        .parameters
        .insert("title".into(), "title".into());
    Template {
        parameters: BTreeMap::from([(
            "title".into(),
            Parameter {
                schema: json!({"type":"string"}),
                default: Some(json!(default)),
            },
        )]),
        imports: BTreeMap::new(),
        exports: BTreeMap::from([("App".into(), local(&["content"], "Content"))]),
        children: BTreeMap::from([("content".into(), content)]),
    }
}

fn fixture() -> (Input, Packages) {
    let mut packages = Packages::default();
    packages
        .insert(
            package("content", 1),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, entry())]),
                composition: None,
            },
        )
        .unwrap();
    packages
        .insert(
            package("product", 1),
            Declaration {
                entries: BTreeMap::new(),
                composition: Some(template("default")),
            },
        )
        .unwrap();
    let input = Input {
        environment: Environment {
            location: ExecutionLocation::Ui,
            presentation: Some(PresentationTarget::Tui),
        },
        instances: BTreeMap::from([
            (
                "a".into(),
                node(1, package("product", 1), PackageUse::Composition),
            ),
            (
                "b".into(),
                node(2, package("product", 1), PackageUse::Composition),
            ),
        ]),
        recovery: None,
        launch: Some(local(&["a"], "App")),
    };
    (input, packages)
}

fn instance<'a>(plan: &'a Plan, path: &[&str]) -> &'a Instance {
    plan.instances()
        .values()
        .find(|instance| {
            instance
                .path
                .iter()
                .map(String::as_str)
                .eq(path.iter().copied())
        })
        .unwrap()
}

#[test]
fn forwarded_exports_and_plan_snapshots_share_frozen_declarations() {
    let (input, packages) = fixture();
    let plan = Plan::prepare(&input, &packages, &RemoteExports::new()).unwrap();
    let content = instance(&plan, &["a", "content"]);
    let original = &content.exports["Content"].declaration;
    assert!(Arc::ptr_eq(
        original,
        &instance(&plan, &["a"]).exports["App"].declaration
    ));
    assert!(Arc::ptr_eq(original, &plan.launch().unwrap().declaration));
    let snapshot = plan.clone();
    let retained = instance(&snapshot, &["a", "content"]);
    assert!(Arc::ptr_eq(
        original,
        &retained.exports["Content"].declaration
    ));
    assert!(Arc::ptr_eq(
        content.entry.as_ref().unwrap(),
        retained.entry.as_ref().unwrap()
    ));
    assert_eq!(
        serde_json::to_value(original).unwrap(),
        json!({
            "contract": contract(), "availability": "required"
        })
    );
}

#[test]
fn template_instances_keep_overrides_and_identity_through_upgrade_and_alias_change() {
    let (mut input, mut packages) = fixture();
    input
        .instances
        .get_mut("a")
        .unwrap()
        .overrides
        .push(Override {
            path: vec!["content".into()],
            configuration: BTreeMap::from([("title".into(), json!("A override"))]),
            bindings: BTreeMap::new(),
            disabled: None,
            ..Override::default()
        });
    let initial = Plan::prepare(&input, &packages, &RemoteExports::new()).unwrap();
    let identity = instance(&initial, &["a", "content"]).identity.clone();
    assert_ne!(identity, instance(&initial, &["b", "content"]).identity);
    assert_eq!(
        instance(&initial, &["a", "content"]).configuration["title"],
        "A override"
    );
    assert_eq!(
        instance(&initial, &["b", "content"]).configuration["title"],
        "default"
    );

    packages
        .insert(
            package("product", 2),
            Declaration {
                entries: BTreeMap::new(),
                composition: Some(template("new default")),
            },
        )
        .unwrap();
    for root in input.instances.values_mut() {
        root.package = package("product", 2).into();
    }
    let upgraded = Plan::prepare(&input, &packages, &RemoteExports::new()).unwrap();
    assert_eq!(instance(&upgraded, &["a", "content"]).identity, identity);
    assert_eq!(
        instance(&upgraded, &["a", "content"]).configuration["title"],
        "A override"
    );
    assert_eq!(
        instance(&upgraded, &["b", "content"]).configuration["title"],
        "new default"
    );
    input.instances.remove("b");
    let a = input.instances.remove("a").unwrap();
    input.instances.insert("renamed".into(), a);
    input.launch = Some(local(&["renamed"], "App"));
    let renamed = Plan::prepare(&input, &packages, &RemoteExports::new()).unwrap();
    assert_eq!(
        instance(&renamed, &["renamed", "content"]).identity,
        identity
    );
    assert_eq!(renamed.instances().len(), 2);
    let recovered: Input = serde_json::from_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    assert_eq!(
        Plan::prepare(&recovered, &packages, &RemoteExports::new()).unwrap(),
        renamed
    );
}

#[test]
fn removed_source_or_template_child_cannot_silently_discard_other_intent() {
    let (mut input, mut packages) = fixture();
    let mut consumer = node(3, package("consumer", 1), PackageUse::Entry);
    consumer
        .configuration
        .values
        .insert("title".into(), json!("consumer"));
    consumer
        .bindings
        .insert("content".into(), vec![local(&["b"], "App")]);
    let mut consumer_entry = entry();
    consumer_entry.imports.insert(
        "content".into(),
        Import {
            contract: contract(),
            cardinality: Cardinality::One,
        },
    );
    packages
        .insert(
            package("consumer", 1),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, consumer_entry)]),
                composition: None,
            },
        )
        .unwrap();
    input.instances.insert("consumer".into(), consumer);
    let prior = Plan::prepare(&input, &packages, &RemoteExports::new()).unwrap();
    let mut candidate = input.clone();
    candidate.instances.remove("b");
    assert!(Plan::prepare(&candidate, &packages, &RemoteExports::new()).is_err());
    assert_eq!(
        Plan::prepare(&input, &packages, &RemoteExports::new()).unwrap(),
        prior
    );

    input
        .instances
        .get_mut("a")
        .unwrap()
        .overrides
        .push(Override {
            path: vec!["removed".into()],
            configuration: BTreeMap::new(),
            bindings: BTreeMap::new(),
            disabled: Some(false),
            ..Override::default()
        });
    assert!(
        Plan::prepare(&input, &packages, &RemoteExports::new())
            .unwrap_err()
            .to_string()
            .contains("override target disappeared")
    );
}

#[test]
fn structural_imports_forward_exact_capabilities_and_detect_dependency_cycles() {
    let (mut input, mut packages) = fixture();
    let mut content = entry();
    content.imports.insert(
        "base".into(),
        Import {
            contract: contract(),
            cardinality: Cardinality::One,
        },
    );
    packages
        .insert(
            package("dependent", 1),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, content)]),
                composition: None,
            },
        )
        .unwrap();
    let mut wrapper = template("dependent");
    wrapper.imports.insert(
        "base".into(),
        Import {
            contract: contract(),
            cardinality: Cardinality::One,
        },
    );
    let child = wrapper.children.get_mut("content").unwrap();
    child.package = package("dependent", 1).into();
    child.bindings.insert(
        "base".into(),
        vec![Target::Import {
            name: "base".into(),
        }],
    );
    packages
        .insert(
            package("wrapper", 1),
            Declaration {
                entries: BTreeMap::new(),
                composition: Some(wrapper),
            },
        )
        .unwrap();
    input.instances.get_mut("a").unwrap().package = package("wrapper", 1).into();
    input
        .instances
        .get_mut("a")
        .unwrap()
        .bindings
        .insert("base".into(), vec![local(&["b"], "App")]);
    let plan = Plan::prepare(&input, &packages, &RemoteExports::new()).unwrap();
    assert_eq!(
        instance(&plan, &["a", "content"]).bindings["base"][0].provider,
        Provider::Local {
            instance: instance(&plan, &["b", "content"]).identity.clone(),
            export: "Content".into()
        }
    );
    input
        .instances
        .get_mut("a")
        .unwrap()
        .bindings
        .insert("base".into(), vec![local(&["a"], "App")]);
    assert!(
        Plan::prepare(&input, &packages, &RemoteExports::new())
            .unwrap_err()
            .to_string()
            .contains("cyclic capability dependency")
    );
}

#[test]
fn actual_export_checks_use_the_full_public_contract() {
    let entry = entry();
    assert!(
        entry
            .check_exports(&BTreeMap::new(), |contract: &Contract| contract)
            .is_err()
    );
    let mut actual = BTreeMap::from([("Content".into(), contract())]);
    entry.check_exports(&actual, |contract| contract).unwrap();
    actual.get_mut("Content").unwrap().interface = Interface::Value {
        schema: json!({"type":"object"}),
    };
    assert!(entry.check_exports(&actual, |contract| contract).is_err());
    actual.insert("undeclared".into(), contract());
    assert!(entry.check_exports(&actual, |contract| contract).is_err());
}

#[test]
fn scene_declarations_are_required_only_for_javascript_component_entries() {
    let valid = entry();
    valid.validate().unwrap();
    let mut optional = valid.clone();
    optional.exports.get_mut("Content").unwrap().availability = Availability::Optional;
    optional.validate().unwrap();
    optional.scene = None;
    assert!(
        optional
            .validate()
            .unwrap_err()
            .to_string()
            .contains("require a scene language")
    );
    for language in [
        crate::package::SceneLanguage {
            name: String::new(),
            version: 1,
        },
        crate::package::SceneLanguage {
            name: "bad/name".into(),
            version: 1,
        },
        crate::package::SceneLanguage {
            name: "x".repeat(129),
            version: 1,
        },
        crate::package::SceneLanguage {
            name: "valid.scene".into(),
            version: 0,
        },
    ] {
        let mut invalid = valid.clone();
        invalid.scene = Some(language);
        assert!(invalid.validate().is_err());
    }
    let host = Declaration {
        entries: BTreeMap::from([(EntryTarget::Host, valid.clone())]),
        composition: None,
    };
    assert!(
        host.validate()
            .unwrap_err()
            .to_string()
            .contains("Host entry")
    );
    let mut native = valid.clone();
    native.implementation = Implementation::Native {
        implementation: "compiled.component".into(),
    };
    assert!(native.validate().is_err());
    native.scene = None;
    native.validate().unwrap();
    let mut value = valid.clone();
    value.exports.get_mut("Content").unwrap().contract.interface = Interface::Value {
        schema: json!(true),
    };
    assert!(value.validate().is_err());
    value.scene = None;
    value.validate().unwrap();
    let mut empty = valid;
    empty.exports.clear();
    assert!(empty.validate().is_err());
    empty.scene = None;
    empty.validate().unwrap();
}

#[test]
fn incompatible_binding_missing_variant_and_optional_provider_fail_before_activation() {
    let (mut input, mut packages) = fixture();
    let mut consumer = node(3, package("consumer", 1), PackageUse::Entry);
    consumer
        .configuration
        .values
        .insert("title".into(), json!("consumer"));
    consumer
        .bindings
        .insert("content".into(), vec![local(&["a"], "App")]);
    let mut entry = entry();
    let mut expected = contract();
    expected.version = 2;
    entry.imports.insert(
        "content".into(),
        Import {
            contract: expected,
            cardinality: Cardinality::One,
        },
    );
    packages
        .insert(
            package("consumer", 1),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, entry.clone())]),
                composition: None,
            },
        )
        .unwrap();
    input.instances.insert("consumer".into(), consumer);
    assert!(matches!(
        Plan::prepare(&input, &packages, &RemoteExports::new()),
        Err(Error::Contract(_))
    ));
    entry.imports.get_mut("content").unwrap().contract = contract();
    packages
        .insert(
            package("consumer", 2),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, entry)]),
                composition: None,
            },
        )
        .unwrap();
    input.instances.get_mut("consumer").unwrap().package = package("consumer", 2).into();
    Plan::prepare(&input, &packages, &RemoteExports::new()).unwrap();
    let mut provider = self::entry();
    provider.exports.get_mut("Content").unwrap().availability = Availability::Optional;
    packages
        .insert(
            package("content", 2),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, provider)]),
                composition: None,
            },
        )
        .unwrap();
    let mut updated = template("optional");
    updated.children.get_mut("content").unwrap().package = package("content", 2).into();
    packages
        .insert(
            package("product", 2),
            Declaration {
                entries: BTreeMap::new(),
                composition: Some(updated),
            },
        )
        .unwrap();
    input.instances.get_mut("a").unwrap().package = package("product", 2).into();
    input.launch = Some(local(&["b"], "App"));
    assert!(matches!(
        Plan::prepare(&input, &packages, &RemoteExports::new()),
        Err(Error::Contract(_))
    ));
}

#[test]
fn remote_binding_requires_a_remotable_observed_contract() {
    let (mut input, mut packages) = fixture();
    let capability = RemoteCapability {
        deployment: "primary".into(),
        instance: InstanceId(vec![Uuid::from_u128(20)]),
        export: "Content".into(),
    };
    let mut consumer = node(3, package("consumer", 1), PackageUse::Entry);
    consumer
        .configuration
        .values
        .insert("title".into(), json!("remote"));
    consumer.bindings.insert(
        "content".into(),
        vec![Target::Remote {
            deployment: capability.deployment.clone(),
            instance: capability.instance.clone(),
            export: capability.export.clone(),
        }],
    );
    let mut entry = entry();
    let mut remote_contract = contract();
    remote_contract.transport = Transport::Remote;
    entry.imports.insert(
        "content".into(),
        Import {
            contract: remote_contract.clone(),
            cardinality: Cardinality::One,
        },
    );
    packages
        .insert(
            package("consumer", 1),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, entry)]),
                composition: None,
            },
        )
        .unwrap();
    input.instances.insert("consumer".into(), consumer);
    let mut remote = RemoteExports::new();
    assert!(Plan::prepare(&input, &packages, &remote).is_err());
    remote.insert(
        capability.clone(),
        Export {
            contract: contract(),
            availability: Availability::Required,
        },
    );
    assert!(Plan::prepare(&input, &packages, &remote).is_err());
    remote.insert(
        capability,
        Export {
            contract: remote_contract,
            availability: Availability::Required,
        },
    );
    let plan = Plan::prepare(&input, &packages, &remote).unwrap();
    assert!(matches!(
        instance(&plan, &["consumer"]).bindings["content"][0].provider,
        Provider::Remote { .. }
    ));
}

#[test]
fn nested_override_precedence_preserves_unrelated_template_defaults() {
    let (mut input, mut packages) = fixture();
    let mut nested = node(30, package("product", 1), PackageUse::Composition);
    nested.overrides.push(Override {
        path: vec!["content".into()],
        configuration: BTreeMap::from([("title".into(), json!("inner"))]),
        bindings: BTreeMap::new(),
        disabled: None,
        ..Override::default()
    });
    packages
        .insert(
            package("outer", 1),
            Declaration {
                entries: BTreeMap::new(),
                composition: Some(Template {
                    parameters: BTreeMap::new(),
                    imports: BTreeMap::new(),
                    exports: BTreeMap::from([("App".into(), local(&["nested"], "App"))]),
                    children: BTreeMap::from([("nested".into(), nested)]),
                }),
            },
        )
        .unwrap();
    let root = input.instances.get_mut("a").unwrap();
    root.package = package("outer", 1).into();
    let mut replacement = self::entry();
    replacement.scene = None;
    replacement.implementation = Implementation::Native {
        implementation: "replacement.Content".into(),
    };
    packages
        .insert(
            package("replacement", 1),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, replacement)]),
                composition: None,
            },
        )
        .unwrap();
    root.overrides.push(Override {
        path: vec!["nested".into(), "content".into()],
        package: Some(package("replacement", 1)),
        configuration: BTreeMap::from([("title".into(), json!("outer user"))]),
        bindings: BTreeMap::new(),
        disabled: None,
        ..Override::default()
    });
    let plan = Plan::prepare(&input, &packages, &RemoteExports::new()).unwrap();
    assert_eq!(
        instance(&plan, &["a", "nested", "content"]).configuration["title"],
        "outer user"
    );
    assert_eq!(
        instance(&plan, &["a", "nested", "content"]).package,
        package("replacement", 1)
    );
    assert_eq!(
        instance(&plan, &["b", "content"]).configuration["title"],
        "default"
    );
}

#[test]
fn recursive_template_expansion_is_bounded_and_source_keys_are_unique() {
    let (mut input, mut packages) = fixture();
    input.instances.get_mut("b").unwrap().key = input.instances["a"].key;
    assert!(Plan::prepare(&input, &packages, &RemoteExports::new()).is_err());
    input.instances.remove("b");
    packages
        .insert(
            package("recursive", 1),
            Declaration {
                entries: BTreeMap::new(),
                composition: Some(Template {
                    parameters: BTreeMap::new(),
                    imports: BTreeMap::new(),
                    exports: BTreeMap::new(),
                    children: BTreeMap::from([(
                        "child".into(),
                        node(42, package("recursive", 1), PackageUse::Composition),
                    )]),
                }),
            },
        )
        .unwrap();
    input.instances.get_mut("a").unwrap().package = package("recursive", 1).into();
    input.launch = None;
    assert!(
        Plan::prepare(&input, &packages, &RemoteExports::new())
            .unwrap_err()
            .to_string()
            .contains("expansion limits")
    );
}

#[test]
fn template_can_use_its_own_package_without_a_self_referential_revision() {
    let (mut input, mut packages) = fixture();
    let reference = package("integrated", 1);
    let mut own = template("integrated");
    own.children.get_mut("content").unwrap().package = PackageSelection::Current;
    packages
        .insert(
            reference.clone(),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, entry())]),
                composition: Some(own),
            },
        )
        .unwrap();
    input.instances.get_mut("a").unwrap().package = reference.clone().into();
    let plan = Plan::prepare(&input, &packages, &RemoteExports::new()).unwrap();
    assert_eq!(instance(&plan, &["a", "content"]).package, reference);
    input.instances.get_mut("a").unwrap().package = PackageSelection::Current;
    assert!(
        Plan::prepare(&input, &packages, &RemoteExports::new())
            .unwrap_err()
            .to_string()
            .contains("roots require a pinned")
    );
}

#[test]
fn explicit_null_parameter_default_survives_package_metadata_roundtrip() {
    let (mut input, mut packages) = fixture();
    let mut declaration = template("unused");
    let parameter = declaration.parameters.get_mut("title").unwrap();
    parameter.schema = json!({"type":["string","null"]});
    parameter.default = Some(Value::Null);
    declaration.children.get_mut("content").unwrap().package = package("nullable", 1).into();
    let recovered: Template =
        serde_json::from_value(serde_json::to_value(declaration).unwrap()).unwrap();
    let mut nullable = entry();
    nullable.configuration["properties"]["title"] = json!({"type":["string","null"]});
    packages
        .insert(
            package("nullable", 1),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, nullable)]),
                composition: None,
            },
        )
        .unwrap();
    packages
        .insert(
            package("nullable-product", 1),
            Declaration {
                entries: BTreeMap::new(),
                composition: Some(recovered),
            },
        )
        .unwrap();
    input.instances.get_mut("a").unwrap().package = package("nullable-product", 1).into();
    let plan = Plan::prepare(&input, &packages, &RemoteExports::new()).unwrap();
    assert!(instance(&plan, &["a", "content"]).configuration["title"].is_null());
}
