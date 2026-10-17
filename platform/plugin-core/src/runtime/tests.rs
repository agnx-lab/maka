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

use std::{cell::Cell, rc::Rc};

use serde_json::json;

use super::*;
use crate::{
    composition::{
        Configuration, Environment, ExecutionLocation, Input, Node, PackageUse, Target, Template,
    },
    contract::{Availability, Cardinality, Export, Import, Interface, Transport},
    package::{Declaration, EntryDeclaration, EntryTarget, Implementation, PackageRef, Packages},
    plan::{RemoteCapability, RemoteExports},
};

fn contract() -> Contract {
    Contract {
        name: "example.Action".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Value {
            schema: json!({"type":"string"}),
        },
    }
}

fn entry(provider: bool) -> EntryDeclaration {
    EntryDeclaration {
        scene: None,
        implementation: Implementation::Native {
            implementation: "example.native".into(),
        },
        configuration: json!({"type":"object"}),
        imports: if provider {
            BTreeMap::new()
        } else {
            BTreeMap::from([(
                "action".into(),
                Import {
                    contract: contract(),
                    cardinality: Cardinality::One,
                },
            )])
        },
        exports: if provider {
            BTreeMap::from([(
                "action".into(),
                Export {
                    contract: contract(),
                    availability: Availability::Required,
                },
            )])
        } else {
            BTreeMap::new()
        },
    }
}

fn reference(name: &str) -> PackageRef {
    PackageRef {
        id: name.into(),
        revision: format!("sha256-{:064x}", 1),
    }
}

fn target(name: &str) -> Target {
    Target::Instance {
        path: vec![name.into()],
        export: "action".into(),
    }
}

fn node(name: &str, key: u128, provider: bool) -> Node {
    Node {
        key: Uuid::from_u128(key),
        package: reference(name).into(),
        use_package: PackageUse::Entry,
        configuration: Configuration::default(),
        parameters: Configuration::default(),
        bindings: if provider {
            BTreeMap::new()
        } else {
            BTreeMap::from([("action".into(), vec![target("p")])])
        },
        disabled: false,
        overrides: Vec::new(),
    }
}

struct Fixture {
    input: Input,
    packages: Packages,
}

impl Fixture {
    fn new() -> Self {
        let mut packages = Packages::default();
        for (name, provider) in [("provider", true), ("consumer", false)] {
            packages
                .insert(
                    reference(name),
                    Declaration {
                        entries: BTreeMap::from([(EntryTarget::Host, entry(provider))]),
                        composition: None,
                    },
                )
                .unwrap();
        }
        Self {
            packages,
            input: Input {
                environment: Environment {
                    location: ExecutionLocation::Host,
                    presentation: None,
                },
                instances: BTreeMap::from([
                    ("p".into(), node("provider", 1, true)),
                    ("a".into(), node("consumer", 2, false)),
                    ("b".into(), node("consumer", 3, false)),
                ]),
                recovery: None,
                launch: None,
            },
        }
    }

    fn plan(&self) -> Plan {
        Plan::prepare(&self.input, &self.packages, &RemoteExports::new()).unwrap()
    }
    fn id(&self, name: &str) -> InstanceId {
        self.plan()
            .instances()
            .values()
            .find(|instance| instance.path.last().unwrap() == name)
            .unwrap()
            .identity
            .clone()
    }

    fn activate<T>(
        &self,
        runtime: &mut Runtime<T>,
        name: &str,
        batch: BTreeMap<String, Published<T>>,
    ) -> Activation {
        let activation = runtime.begin(&self.id(name)).unwrap();
        runtime.publish(&activation, batch).unwrap();
        activation
    }
}

fn published<T>(value: T) -> BTreeMap<String, Published<T>> {
    BTreeMap::from([(
        "action".into(),
        Published {
            contract: contract(),
            value,
        },
    )])
}

// Rc and Cell deliberately model foreground-only native objects.
struct Action {
    effects: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
}
impl Action {
    fn invoke(&self) {
        self.effects.set(self.effects.get() + 1);
    }
}
impl Drop for Action {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

#[test]
fn actual_export_batch_is_atomic_and_rejected_values_return_to_creator() {
    let fixture = Fixture::new();
    let mut runtime = Runtime::new(fixture.plan());
    let provider = runtime.begin(&fixture.id("p")).unwrap();
    let consumer = runtime.begin(&fixture.id("a")).unwrap();
    let drops = Rc::new(Cell::new(0));
    let mut batch = published(Action {
        effects: Rc::new(Cell::new(0)),
        drops: drops.clone(),
    });
    batch.get_mut("action").unwrap().contract.version = 2;
    let rejected = runtime.publish(&provider, batch).unwrap_err();
    assert!(matches!(rejected.error, Error::Contract(_)));
    assert_eq!(drops.get(), 0);
    assert_eq!(runtime.status(&provider).unwrap().phase, Phase::Loading);
    assert!(matches!(
        runtime.bind(&consumer, "action"),
        Err(Error::NotReady(_))
    ));
    drop(rejected.batch);
    assert_eq!(drops.get(), 1);
    assert!(runtime.publish(&provider, BTreeMap::new()).is_err());
    runtime
        .publish(
            &provider,
            published(Action {
                effects: Rc::new(Cell::new(0)),
                drops,
            }),
        )
        .unwrap();
    assert_eq!(runtime.bind(&consumer, "action").unwrap().len(), 1);
}

#[test]
fn consumer_retirement_closes_queued_admission_but_keeps_accepted_native_work() {
    let fixture = Fixture::new();
    let mut runtime = Runtime::new(fixture.plan());
    let effects = Rc::new(Cell::new(0));
    let provider = fixture.activate(
        &mut runtime,
        "p",
        published(Action {
            effects: effects.clone(),
            drops: Rc::new(Cell::new(0)),
        }),
    );
    let a = fixture.activate(&mut runtime, "a", BTreeMap::new());
    let b = fixture.activate(&mut runtime, "b", BTreeMap::new());
    let from_a = runtime.bind(&a, "action").unwrap().pop().unwrap();
    let from_b = runtime.bind(&b, "action").unwrap().pop().unwrap();
    assert!(runtime.inspect(&from_a).is_ok());
    assert_eq!(runtime.status(&provider).unwrap().calls, 0);
    let accepted = runtime.admit(&from_a).unwrap();
    assert_eq!(runtime.retire(&a).unwrap(), vec![a.clone()]);
    assert!(matches!(runtime.inspect(&from_a), Err(Error::Retired)));
    assert!(matches!(runtime.admit(&from_a), Err(Error::Retired)));
    runtime.finish_cleanup(&a).unwrap();
    accepted.value().invoke();
    let other = runtime.admit(&from_b).unwrap();
    other.value().invoke();
    assert_eq!(effects.get(), 2);
    assert_eq!(runtime.status(&provider).unwrap().calls, 2);
    runtime.settle(&accepted, Ok(())).unwrap();
    runtime.settle(&other, Ok(())).unwrap();
    assert_eq!(runtime.status(&provider).unwrap().calls, 0);
}

#[test]
fn replacement_waits_for_resource_close_and_call_settlement_and_never_retargets_old_handles() {
    let fixture = Fixture::new();
    let mut runtime = Runtime::new(fixture.plan());
    let provider = fixture.activate(&mut runtime, "p", published("old"));
    let consumer = fixture.activate(&mut runtime, "a", BTreeMap::new());
    let capability = runtime.bind(&consumer, "action").unwrap().pop().unwrap();
    let execution = runtime.admit(&capability).unwrap();
    let resource = runtime.own_resource(&provider, "subscription").unwrap();
    let retired = runtime.retire(&provider).unwrap();
    assert!(retired.contains(&consumer));
    assert!(matches!(runtime.admit(&capability), Err(Error::Retired)));
    assert!(matches!(
        runtime.finish_cleanup(&provider),
        Err(Error::CleanupPending)
    ));
    assert!(runtime.begin(&fixture.id("p")).is_err());
    runtime.settle(&execution, Ok(())).unwrap();
    assert!(matches!(
        runtime.finish_cleanup(&provider),
        Err(Error::CleanupPending)
    ));
    runtime.complete_resource(&resource, Ok(())).unwrap();
    runtime.finish_cleanup(&provider).unwrap();
    runtime.finish_cleanup(&consumer).unwrap();
    let replacement = fixture.activate(&mut runtime, "p", published("new"));
    let replacement_consumer = fixture.activate(&mut runtime, "a", BTreeMap::new());
    let current = runtime
        .bind(&replacement_consumer, "action")
        .unwrap()
        .pop()
        .unwrap();
    assert_ne!(provider.epoch(), replacement.epoch());
    assert_ne!(
        capability.registration_epoch(),
        current.registration_epoch()
    );
    assert!(matches!(runtime.admit(&capability), Err(Error::Retired)));
    assert!(matches!(runtime.status(&provider), Err(Error::Retired)));
    let current_work = runtime.admit(&current).unwrap();
    assert_eq!(*current_work.value(), "new");
    runtime.settle(&current_work, Ok(())).unwrap();
}

#[test]
fn dropping_executor_token_does_not_pretend_settlement_or_release_owner() {
    let fixture = Fixture::new();
    let mut runtime = Runtime::new(fixture.plan());
    let drops = Rc::new(Cell::new(0));
    let provider = fixture.activate(
        &mut runtime,
        "p",
        published(Action {
            effects: Rc::new(Cell::new(0)),
            drops: drops.clone(),
        }),
    );
    let consumer = fixture.activate(&mut runtime, "a", BTreeMap::new());
    let capability = runtime.bind(&consumer, "action").unwrap().pop().unwrap();
    let execution = runtime.admit(&capability).unwrap();
    let _observer_id = execution.id();
    drop(execution);
    drop(capability);
    runtime.retire(&provider).unwrap();
    runtime.finish_cleanup(&consumer).unwrap();
    assert_eq!(runtime.status(&provider).unwrap().calls, 1);
    assert!(matches!(
        runtime.finish_cleanup(&provider),
        Err(Error::CleanupPending)
    ));
    assert_eq!(drops.get(), 0);
}

#[test]
fn failed_cleanup_fences_replacement_until_executor_confirms_recovery() {
    let fixture = Fixture::new();
    let mut runtime = Runtime::new(fixture.plan());
    let provider = fixture.activate(&mut runtime, "p", published(1));
    let consumer = fixture.activate(&mut runtime, "a", BTreeMap::new());
    let capability = runtime.bind(&consumer, "action").unwrap().pop().unwrap();
    let execution = runtime.admit(&capability).unwrap();
    let resource = runtime.own_resource(&provider, "database writer").unwrap();
    let stopped = runtime
        .settle(&execution, Err("unknown work outcome".into()))
        .unwrap();
    assert!(stopped.contains(&provider));
    assert!(stopped.contains(&consumer));
    runtime
        .complete_resource(&resource, Err("writer close failed".into()))
        .unwrap();
    assert_eq!(runtime.status(&consumer).unwrap().phase, Phase::Retiring);
    assert_eq!(runtime.status(&provider).unwrap().failures.len(), 2);
    assert!(matches!(
        runtime.finish_cleanup(&provider),
        Err(Error::CleanupFailed(_))
    ));
    assert!(runtime.begin(&fixture.id("p")).is_err());
    runtime.settle(&execution, Ok(())).unwrap();
    assert!(matches!(
        runtime.finish_cleanup(&provider),
        Err(Error::CleanupFailed(_))
    ));
    runtime.complete_resource(&resource, Ok(())).unwrap();
    runtime.finish_cleanup(&provider).unwrap();
    assert!(runtime.complete_resource(&resource, Ok(())).is_err());
    fixture.activate(&mut runtime, "p", published(2));
    assert!(runtime.settle(&execution, Ok(())).is_err());
}

#[test]
fn structural_owner_waits_for_children_without_requiring_native_exports() {
    let mut fixture = Fixture::new();
    fixture
        .packages
        .insert(
            reference("product"),
            Declaration {
                entries: BTreeMap::new(),
                composition: Some(Template {
                    parameters: BTreeMap::new(),
                    imports: BTreeMap::new(),
                    exports: BTreeMap::new(),
                    children: std::mem::take(&mut fixture.input.instances),
                }),
            },
        )
        .unwrap();
    let mut product = node("product", 100, true);
    product.use_package = PackageUse::Composition;
    fixture.input.instances.insert("product".into(), product);
    let mut runtime = Runtime::new(fixture.plan());
    assert!(matches!(
        runtime.begin(&fixture.id("p")),
        Err(Error::NotReady(_))
    ));
    let parent = fixture.activate(&mut runtime, "product", BTreeMap::new());
    let provider = fixture.activate(&mut runtime, "p", published(1));
    let consumer = fixture.activate(&mut runtime, "a", BTreeMap::new());
    assert_eq!(runtime.retire(&parent).unwrap().len(), 3);
    assert!(matches!(
        runtime.finish_cleanup(&parent),
        Err(Error::CleanupPending)
    ));
    runtime.finish_cleanup(&consumer).unwrap();
    runtime.finish_cleanup(&provider).unwrap();
    runtime.finish_cleanup(&parent).unwrap();
}

#[test]
fn structural_routing_does_not_turn_a_child_dependency_into_whole_product_retirement() {
    let mut fixture = Fixture::new();
    let mut child = node("consumer", 2, false);
    child.bindings.insert(
        "action".into(),
        vec![Target::Import {
            name: "action".into(),
        }],
    );
    fixture
        .packages
        .insert(
            reference("product"),
            Declaration {
                entries: BTreeMap::new(),
                composition: Some(Template {
                    parameters: BTreeMap::new(),
                    imports: BTreeMap::from([(
                        "action".into(),
                        Import {
                            contract: contract(),
                            cardinality: Cardinality::One,
                        },
                    )]),
                    exports: BTreeMap::new(),
                    children: BTreeMap::from([
                        ("a".into(), child),
                        ("b".into(), node("provider", 3, true)),
                    ]),
                }),
            },
        )
        .unwrap();
    let mut product = node("product", 100, false);
    product.use_package = PackageUse::Composition;
    fixture.input.instances.retain(|name, _| name == "p");
    fixture.input.instances.insert("product".into(), product);
    let mut runtime = Runtime::new(fixture.plan());
    let parent = fixture.activate(&mut runtime, "product", BTreeMap::new());
    let provider = fixture.activate(&mut runtime, "p", published(1));
    let consumer = fixture.activate(&mut runtime, "a", BTreeMap::new());
    let independent = fixture.activate(&mut runtime, "b", published(2));
    let stopped = runtime.retire(&provider).unwrap();
    assert_eq!(stopped.len(), 2);
    assert!(stopped.contains(&consumer));
    assert_eq!(runtime.status(&parent).unwrap().phase, Phase::Active);
    assert_eq!(runtime.status(&independent).unwrap().phase, Phase::Active);
}

#[test]
fn initialization_can_invoke_live_dependencies_without_publishing_partial_exports() {
    let fixture = Fixture::new();
    let mut runtime = Runtime::new(fixture.plan());
    let provider = fixture.activate(&mut runtime, "p", published("initialized state"));
    let consumer = runtime.begin(&fixture.id("a")).unwrap();
    let capability = runtime.bind(&consumer, "action").unwrap().pop().unwrap();
    let initialization = runtime.admit(&capability).unwrap();
    assert_eq!(*initialization.value(), "initialized state");
    assert_eq!(runtime.status(&consumer).unwrap().phase, Phase::Loading);
    runtime.retire(&consumer).unwrap();
    assert!(matches!(runtime.admit(&capability), Err(Error::Retired)));
    runtime.finish_cleanup(&consumer).unwrap();
    assert_eq!(runtime.status(&provider).unwrap().calls, 1);
    runtime.settle(&initialization, Ok(())).unwrap();
}

#[test]
fn resource_failure_delivers_the_full_stop_set_to_adapter() {
    let fixture = Fixture::new();
    let mut runtime = Runtime::new(fixture.plan());
    let provider = fixture.activate(&mut runtime, "p", published(1));
    let consumer = fixture.activate(&mut runtime, "a", BTreeMap::new());
    let resource = runtime
        .own_resource(&provider, "foreground subscription")
        .unwrap();
    let stopped = runtime
        .complete_resource(&resource, Err("close acknowledgement missing".into()))
        .unwrap();
    assert_eq!(stopped.len(), 2);
    assert!(stopped.contains(&provider));
    assert!(stopped.contains(&consumer));
    assert!(runtime.retire(&provider).unwrap().is_empty());
    assert!(matches!(
        runtime.finish_cleanup(&provider),
        Err(Error::CleanupFailed(_))
    ));
}

#[test]
fn alias_changes_keep_activations_but_effective_configuration_changes_retire_dependents() {
    let mut fixture = Fixture::new();
    let mut runtime = Runtime::new(fixture.plan());
    let provider = fixture.activate(&mut runtime, "p", published(1));
    let consumer = fixture.activate(&mut runtime, "a", BTreeMap::new());
    let p = fixture.input.instances.remove("p").unwrap();
    fixture.input.instances.insert("renamed".into(), p);
    for name in ["a", "b"] {
        fixture
            .input
            .instances
            .get_mut(name)
            .unwrap()
            .bindings
            .insert("action".into(), vec![target("renamed")]);
    }
    assert!(runtime.install(fixture.plan()).is_empty());
    assert_eq!(runtime.instance(&provider).unwrap().path, vec!["renamed"]);
    assert_eq!(runtime.status(&provider).unwrap().phase, Phase::Active);
    fixture
        .input
        .instances
        .get_mut("renamed")
        .unwrap()
        .configuration
        .values
        .insert("setting".into(), json!(2));
    let retired = runtime.install(fixture.plan());
    assert_eq!(retired.len(), 2);
    assert!(retired.contains(&provider));
    assert!(retired.contains(&consumer));
    assert_eq!(
        runtime.instance(&provider).unwrap().configuration,
        json!({})
    );
}

#[test]
fn consumer_configuration_changes_do_not_retire_shared_provider() {
    let mut fixture = Fixture::new();
    let mut runtime = Runtime::new(fixture.plan());
    let provider = fixture.activate(&mut runtime, "p", published(1));
    let consumer = fixture.activate(&mut runtime, "a", BTreeMap::new());
    let sibling = fixture.activate(&mut runtime, "b", BTreeMap::new());
    fixture
        .input
        .instances
        .get_mut("a")
        .unwrap()
        .configuration
        .values
        .insert("setting".into(), json!(2));
    assert_eq!(runtime.install(fixture.plan()), vec![consumer]);
    assert_eq!(runtime.status(&provider).unwrap().phase, Phase::Active);
    assert_eq!(runtime.status(&sibling).unwrap().phase, Phase::Active);
}

#[test]
fn required_imports_must_be_live_before_atomic_publication() {
    let fixture = Fixture::new();
    let mut runtime = Runtime::<usize>::new(fixture.plan());
    let consumer = runtime.begin(&fixture.id("a")).unwrap();
    assert!(matches!(
        runtime
            .publish(&consumer, BTreeMap::new())
            .unwrap_err()
            .error,
        Error::NotReady(_)
    ));
    fixture.activate(&mut runtime, "p", published(1));
    runtime.publish(&consumer, BTreeMap::new()).unwrap();
    assert_eq!(runtime.status(&consumer).unwrap().phase, Phase::Active);
}

#[test]
fn authority_tokens_cannot_cross_runtime_domains() {
    let fixture = Fixture::new();
    let mut first = Runtime::new(fixture.plan());
    let mut second = Runtime::new(fixture.plan());
    let provider = fixture.activate(&mut first, "p", published(1));
    let consumer = fixture.activate(&mut first, "a", BTreeMap::new());
    let capability = first.bind(&consumer, "action").unwrap().pop().unwrap();
    let execution = first.admit(&capability).unwrap();
    let resource = first.own_resource(&provider, "task").unwrap();
    fixture.activate(&mut second, "p", published(2));
    fixture.activate(&mut second, "a", BTreeMap::new());
    assert!(matches!(second.retire(&provider), Err(Error::Retired)));
    assert!(matches!(second.admit(&capability), Err(Error::Retired)));
    assert!(matches!(
        second.settle(&execution, Ok(())),
        Err(Error::Retired)
    ));
    assert!(matches!(
        second.complete_resource(&resource, Ok(())),
        Err(Error::Retired)
    ));
    assert_eq!(first.status(&provider).unwrap().calls, 1);
    assert_eq!(first.status(&provider).unwrap().resources, 1);
}

#[test]
fn remote_metadata_does_not_count_as_an_actual_registration() {
    let mut fixture = Fixture::new();
    let mut remote_contract = contract();
    remote_contract.transport = Transport::Remote;
    let mut remote_entry = entry(false);
    remote_entry.imports.get_mut("action").unwrap().contract = remote_contract.clone();
    remote_entry.imports.get_mut("action").unwrap().cardinality = Cardinality::One;
    fixture
        .packages
        .insert(
            reference("remote-consumer"),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Host, remote_entry)]),
                composition: None,
            },
        )
        .unwrap();
    let remote = RemoteCapability {
        deployment: "another-host".into(),
        instance: InstanceId(vec![Uuid::from_u128(99)]),
        export: "action".into(),
    };
    let mut consumer = node("remote-consumer", 4, false);
    consumer.bindings.insert(
        "action".into(),
        vec![Target::Remote {
            deployment: remote.deployment.clone(),
            instance: remote.instance.clone(),
            export: remote.export.clone(),
        }],
    );
    fixture.input.instances = BTreeMap::from([("remote".into(), consumer)]);
    let plan = Plan::prepare(
        &fixture.input,
        &fixture.packages,
        &BTreeMap::from([(
            remote,
            Export {
                contract: remote_contract,
                availability: Availability::Required,
            },
        )]),
    )
    .unwrap();
    let id = plan.instances().values().next().unwrap().identity.clone();
    let mut runtime = Runtime::<usize>::new(plan);
    let activation = runtime.begin(&id).unwrap();
    assert!(matches!(
        runtime
            .publish(&activation, BTreeMap::new())
            .unwrap_err()
            .error,
        Error::NotReady(_)
    ));
}

fn component_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.input.environment = Environment {
        location: ExecutionLocation::Ui,
        presentation: Some(crate::composition::PresentationTarget::Tui),
    };
    fixture.input.launch = Some(target("p"));
    fixture.packages = Packages::default();
    for (name, provider) in [("provider", true), ("consumer", false)] {
        let mut declaration = entry(provider);
        for export in declaration.exports.values_mut() {
            export.contract = component_contract();
        }
        for import in declaration.imports.values_mut() {
            import.contract = component_contract();
        }
        fixture
            .packages
            .insert(
                reference(name),
                Declaration {
                    entries: BTreeMap::from([(EntryTarget::Tui, declaration)]),
                    composition: None,
                },
            )
            .unwrap();
    }
    fixture
}
fn component_contract() -> Contract {
    Contract {
        name: "example.Component".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: json!({"type":"object"}),
            events: BTreeMap::new(),
            captures: BTreeMap::new(),
        },
    }
}
fn component_export() -> BTreeMap<String, Published<()>> {
    BTreeMap::from([(
        "action".into(),
        Published {
            contract: component_contract(),
            value: (),
        },
    )])
}

#[test]
fn mount_close_and_accepted_work_are_independent_cleanup_obligations() {
    let fixture = component_fixture();
    let mut runtime = Runtime::new(fixture.plan());
    let provider = fixture.activate(&mut runtime, "p", component_export());
    let a = runtime.mount_launch().unwrap();
    let b = runtime.mount_launch().unwrap();
    runtime.publish_mount(&a).unwrap();
    runtime.publish_mount(&b).unwrap();
    let accepted = runtime.admit_mount(&a).unwrap();
    runtime.retire_mount(&a).unwrap();
    assert!(runtime.admit_mount(&a).is_err());
    let other = runtime.admit_mount(&b).unwrap();
    runtime.settle(&other, Ok(())).unwrap();
    assert_eq!(runtime.status(&provider).unwrap().phase, Phase::Active);
    assert!(
        runtime
            .complete_mount(&a, Err("unknown native close".into()))
            .is_err()
    );
    runtime.retire(&provider).unwrap();
    assert!(runtime.admit_mount(&b).is_err());
    assert!(matches!(
        runtime.finish_cleanup(&provider),
        Err(Error::CleanupPending)
    ));
    runtime.complete_mount(&a, Ok(())).unwrap();
    runtime.complete_mount(&b, Ok(())).unwrap();
    assert!(matches!(
        runtime.finish_cleanup(&provider),
        Err(Error::CleanupPending)
    ));
    runtime.settle(&accepted, Ok(())).unwrap();
    runtime.finish_cleanup(&provider).unwrap();
    let replacement = fixture.activate(&mut runtime, "p", component_export());
    assert_ne!(provider, replacement);
    assert!(runtime.admit_mount(&a).is_err());
}

#[test]
fn owned_mount_children_close_before_parent_without_retiring_providers() {
    let mut fixture = component_fixture();
    // Root P explicitly imports an independent Q component. Ownership attaches
    // to the mounted child, not Q's reusable provider registration.
    fixture.input.instances.retain(|name, _| name == "p");
    fixture
        .input
        .instances
        .insert("q".into(), node("child", 4, true));
    fixture
        .input
        .instances
        .get_mut("p")
        .unwrap()
        .bindings
        .insert("action".into(), vec![target("q")]);
    let mut parent = entry(false);
    parent.imports.get_mut("action").unwrap().contract = component_contract();
    parent.exports.insert(
        "action".into(),
        crate::contract::Export {
            contract: component_contract(),
            availability: Availability::Required,
        },
    );
    let mut child = entry(true);
    child.exports.get_mut("action").unwrap().contract = component_contract();
    // Use fresh identities for immutable package declarations.
    fixture.input.instances.get_mut("p").unwrap().package = reference("parent").into();
    for (name, entry) in [("parent", parent), ("child", child)] {
        fixture
            .packages
            .insert(
                reference(name),
                Declaration {
                    entries: BTreeMap::from([(EntryTarget::Tui, entry)]),
                    composition: None,
                },
            )
            .unwrap();
    }
    let mut runtime = Runtime::new(fixture.plan());
    let q = fixture.activate(&mut runtime, "q", component_export());
    let p = fixture.activate(&mut runtime, "p", component_export());
    let root = runtime.mount_launch().unwrap();
    runtime.publish_mount(&root).unwrap();
    let capability = runtime.bind(&p, "action").unwrap().remove(0);
    let child = runtime.mount_component(&capability, Some(&root)).unwrap();
    runtime.publish_mount(&child).unwrap();
    assert_eq!(runtime.retire_mount(&root).unwrap().len(), 2);
    assert!(runtime.admit_mount(&child).is_err());
    assert!(matches!(
        runtime.complete_mount(&root, Ok(())),
        Err(Error::CleanupPending)
    ));
    runtime.complete_mount(&child, Ok(())).unwrap();
    runtime.complete_mount(&root, Ok(())).unwrap();
    assert_eq!(runtime.status(&p).unwrap().phase, Phase::Active);
    assert_eq!(runtime.status(&q).unwrap().phase, Phase::Active);
}

#[test]
fn launch_changes_retire_existing_roots_without_retiring_unchanged_providers() {
    let mut fixture = component_fixture();
    let mut runtime = Runtime::new(fixture.plan());
    let provider = fixture.activate(&mut runtime, "p", component_export());
    let root = runtime.mount_launch().unwrap();
    runtime.publish_mount(&root).unwrap();
    fixture.input.launch = None;
    assert!(runtime.install(fixture.plan()).is_empty());
    assert!(runtime.admit_mount(&root).is_err());
    assert_eq!(runtime.status(&provider).unwrap().phase, Phase::Active);
    runtime.complete_mount(&root, Ok(())).unwrap();
    assert!(runtime.mount_launch().is_err());
}

#[test]
fn recovery_root_remains_independent_of_failed_work_and_launch_selection() {
    use crate::composition::RootKind;
    let mut fixture = component_fixture();
    fixture
        .input
        .instances
        .insert("recovery".into(), node("provider", 4, true));
    fixture.input.recovery = Some(target("recovery"));
    let mut runtime = Runtime::new(fixture.plan());
    let primary = fixture.activate(&mut runtime, "p", component_export());
    let recovery = fixture.activate(&mut runtime, "recovery", component_export());
    let root = runtime.mount_launch().unwrap();
    runtime.publish_mount(&root).unwrap();
    let work = runtime.admit_mount(&root).unwrap();
    runtime
        .settle(&work, Err("original cleanup unknown".into()))
        .unwrap();
    assert!(runtime.admit_mount(&root).is_err());
    runtime.complete_mount(&root, Ok(())).unwrap();
    let fallback = runtime.mount_root(RootKind::Recovery).unwrap();
    runtime.publish_mount(&fallback).unwrap();
    let input = runtime.admit_mount(&fallback).unwrap();
    runtime.settle(&input, Ok(())).unwrap();
    assert_eq!(
        runtime.root_kind(&fallback).unwrap(),
        Some(RootKind::Recovery)
    );
    assert!(matches!(
        runtime.finish_cleanup(&primary),
        Err(Error::CleanupFailed(_))
    ));
    assert_eq!(runtime.status(&primary).unwrap().calls, 1);
    fixture.input.launch = None;
    runtime.install(fixture.plan());
    let input = runtime.admit_mount(&fallback).unwrap();
    runtime.settle(&input, Ok(())).unwrap();
    fixture.input.recovery = None;
    runtime.install(fixture.plan());
    assert!(runtime.admit_mount(&fallback).is_err());
    assert_eq!(runtime.status(&recovery).unwrap().phase, Phase::Active);
    assert!(matches!(
        runtime.finish_cleanup(&primary),
        Err(Error::CleanupFailed(_))
    ));
}

fn remote_plan(optional: bool) -> (Fixture, RemoteCapability, Contract, Plan) {
    let mut fixture = Fixture::new();
    let mut declared = contract();
    declared.transport = Transport::Remote;
    let mut declaration = entry(false);
    declaration.imports.get_mut("action").unwrap().contract = declared.clone();
    declaration.imports.get_mut("action").unwrap().cardinality = if optional {
        Cardinality::Optional
    } else {
        Cardinality::One
    };
    fixture
        .packages
        .insert(
            reference("remote-consumer"),
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Host, declaration)]),
                composition: None,
            },
        )
        .unwrap();
    // Deliberately share the local provider's stable ID; origin must distinguish it.
    let target = RemoteCapability {
        deployment: "host-a".into(),
        instance: InstanceId(vec![Uuid::from_u128(1)]),
        export: "action".into(),
    };
    for (alias, key) in [("a", 2), ("b", 3)] {
        let mut consumer = node("remote-consumer", key, false);
        consumer.bindings.insert(
            "action".into(),
            vec![Target::Remote {
                deployment: target.deployment.clone(),
                instance: target.instance.clone(),
                export: target.export.clone(),
            }],
        );
        fixture.input.instances.insert(alias.into(), consumer);
    }
    let observations = BTreeMap::from([(
        target.clone(),
        Export {
            contract: declared.clone(),
            availability: Availability::Required,
        },
    )]);
    let plan = Plan::prepare(&fixture.input, &fixture.packages, &observations).unwrap();
    (fixture, target, declared, plan)
}

#[test]
fn actual_remote_attachment_distinguishes_local_identity_and_fences_reconnect_and_old_handles() {
    let (_, target, declared, plan) = remote_plan(false);
    let mut runtime = Runtime::<usize>::new(plan);
    let local = runtime.begin(&target.instance).unwrap();
    runtime
        .publish(
            &local,
            BTreeMap::from([(
                "action".into(),
                Published {
                    contract: contract(),
                    value: 1,
                },
            )]),
        )
        .unwrap();
    let wire_epoch = Uuid::new_v4();
    let remote = runtime
        .attach_remote(
            target.clone(),
            wire_epoch,
            Published {
                contract: declared.clone(),
                value: 7,
            },
        )
        .unwrap();
    assert_ne!(local, remote);
    assert_eq!(local.instance(), remote.instance());
    assert!(local.remote_target().is_none());
    assert_eq!(remote.remote_target(), Some(&target));
    assert!(runtime.instance(&remote).is_err());
    let consumer = runtime
        .begin(&InstanceId(vec![Uuid::from_u128(2)]))
        .unwrap();
    runtime.publish(&consumer, BTreeMap::new()).unwrap();
    let capability = runtime.bind(&consumer, "action").unwrap().remove(0);
    assert_eq!(capability.provider(), &remote);
    assert_eq!(capability.registration_epoch(), wire_epoch);
    let work = runtime.admit(&capability).unwrap();
    assert_eq!(*work.value(), 7);
    let resource = runtime.own_resource(&remote, "actual connection").unwrap();
    let retired = runtime.retire(&remote).unwrap();
    assert!(retired.contains(&remote));
    assert!(retired.contains(&consumer));
    assert_eq!(runtime.status(&local).unwrap().phase, Phase::Active);
    assert!(runtime.admit(&capability).is_err());
    assert!(matches!(
        runtime.finish_cleanup(&remote),
        Err(Error::CleanupPending)
    ));
    assert!(
        runtime
            .attach_remote(
                target.clone(),
                wire_epoch,
                Published {
                    contract: declared.clone(),
                    value: 8
                }
            )
            .is_err()
    );
    runtime
        .settle(&work, Err("transport cleanup unknown".into()))
        .unwrap();
    assert!(matches!(
        runtime.finish_cleanup(&remote),
        Err(Error::CleanupFailed(_))
    ));
    runtime.complete_resource(&resource, Ok(())).unwrap();
    assert!(
        runtime
            .attach_remote(
                target.clone(),
                wire_epoch,
                Published {
                    contract: declared.clone(),
                    value: 8
                }
            )
            .is_err()
    );
    assert_eq!(*work.value(), 7);
    runtime.settle(&work, Ok(())).unwrap();
    runtime.finish_cleanup(&remote).unwrap();
    let replacement = runtime
        .attach_remote(
            target,
            wire_epoch,
            Published {
                contract: declared,
                value: 8,
            },
        )
        .unwrap();
    assert_ne!(replacement, remote);
    assert!(runtime.admit(&capability).is_err());
    runtime.finish_cleanup(&consumer).unwrap();
    let consumer2 = runtime.begin(consumer.instance()).unwrap();
    runtime.publish(&consumer2, BTreeMap::new()).unwrap();
    let new = runtime.bind(&consumer2, "action").unwrap().remove(0);
    assert_eq!(new.provider(), &replacement);
    let next = runtime.admit(&new).unwrap();
    assert_eq!(*next.value(), 8);
    runtime.settle(&next, Ok(())).unwrap();
    // Tokens belong to exactly one runtime, even if target and wire epoch agree.
    let (_, _, _, other_plan) = remote_plan(false);
    let mut other = Runtime::<usize>::new(other_plan);
    assert!(other.admit(&new).is_err());
    assert!(other.settle(&work, Ok(())).is_err());
}

#[test]
fn optional_remote_metadata_is_empty_and_actual_mismatch_does_not_become_ready() {
    let (_, target, declared, plan) = remote_plan(true);
    let mut runtime = Runtime::<usize>::new(plan);
    let consumer = runtime
        .begin(&InstanceId(vec![Uuid::from_u128(2)]))
        .unwrap();
    runtime.publish(&consumer, BTreeMap::new()).unwrap();
    assert!(runtime.bind(&consumer, "action").unwrap().is_empty());
    let mut incompatible = declared.clone();
    incompatible.version += 1;
    let rejected = runtime
        .attach_remote(
            target.clone(),
            Uuid::new_v4(),
            Published {
                contract: incompatible,
                value: 9,
            },
        )
        .unwrap_err();
    assert_eq!(rejected.batch["action"].value, 9);
    assert!(runtime.bind(&consumer, "action").unwrap().is_empty());
    assert!(
        runtime
            .attach_remote(
                target.clone(),
                Uuid::nil(),
                Published {
                    contract: declared.clone(),
                    value: 9
                }
            )
            .is_err()
    );
    assert!(
        runtime
            .attach_remote(
                target.clone(),
                Uuid::new_v4(),
                Published {
                    contract: contract(),
                    value: 9
                }
            )
            .is_err()
    );
    runtime
        .attach_remote(
            target,
            Uuid::new_v4(),
            Published {
                contract: declared,
                value: 9,
            },
        )
        .unwrap();
    let capability = runtime.bind(&consumer, "action").unwrap().remove(0);
    assert_eq!(*runtime.inspect(&capability).unwrap(), 9);
    assert!(runtime.mount_component(&capability, None).is_err());
}

#[test]
fn installing_unreferenced_input_retires_remote_without_rewriting_original_work() {
    let (mut fixture, target, declared, plan) = remote_plan(false);
    let mut runtime = Runtime::<usize>::new(plan);
    let remote = runtime
        .attach_remote(
            target,
            Uuid::new_v4(),
            Published {
                contract: declared,
                value: 41,
            },
        )
        .unwrap();
    let consumer = runtime
        .begin(&InstanceId(vec![Uuid::from_u128(2)]))
        .unwrap();
    runtime.publish(&consumer, BTreeMap::new()).unwrap();
    let capability = runtime.bind(&consumer, "action").unwrap().remove(0);
    let work = runtime.admit(&capability).unwrap();
    fixture.input.instances.remove("a");
    fixture.input.instances.remove("b");
    let next = fixture.plan();
    let retired = runtime.install(next);
    assert!(retired.contains(&remote));
    assert_eq!(*work.value(), 41);
    assert!(runtime.admit(&capability).is_err());
    assert!(matches!(
        runtime.finish_cleanup(&remote),
        Err(Error::CleanupPending)
    ));
    runtime.settle(&work, Ok(())).unwrap();
    runtime.finish_cleanup(&remote).unwrap();
}

#[test]
fn bound_tokens_do_not_own_values_but_accepted_execution_survives_retirement_and_handle_drop() {
    let fixture = Fixture::new();
    let mut runtime = Runtime::new(fixture.plan());
    let value = Arc::new(());
    let weak = Arc::downgrade(&value);
    let provider = fixture.activate(
        &mut runtime,
        "p",
        BTreeMap::from([(
            "action".into(),
            Published {
                contract: contract(),
                value,
            },
        )]),
    );
    let consumer = fixture.activate(&mut runtime, "a", BTreeMap::new());
    let token = runtime.bind(&consumer, "action").unwrap().remove(0);
    let old = token.clone();
    let execution = runtime.admit(&token).unwrap();
    drop(token);
    runtime.retire(&provider).unwrap();
    runtime.finish_cleanup(&consumer).unwrap();
    assert!(runtime.finish_cleanup(&provider).is_err());
    assert!(weak.upgrade().is_some());
    runtime.settle(&execution, Ok(())).unwrap();
    drop(execution);
    runtime.finish_cleanup(&provider).unwrap();
    assert!(
        weak.upgrade().is_none(),
        "old bound token is not a second value owner"
    );
    assert!(runtime.inspect(&old).is_err());
}
