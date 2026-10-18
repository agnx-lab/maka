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

use maka_skills::{
    Catalog, DiscoveredSkill, DiscoverySnapshot, HostCapabilities, Preference, Preferences,
    SkillLocation, parse,
};
use maka_skills::{SkillScope, SkillSource};
use std::{collections::BTreeMap, path::Path};

fn skill(id: &str, name: &str, body: &str) -> DiscoveredSkill {
    DiscoveredSkill {
        location: SkillLocation {
            reference: format!("project:maka:{id}"),
            id: id.into(),
            path: Path::new("/fixture/skills").join(id),
            discovery_root: "/fixture".into(),
            scope: SkillScope::Project,
            source: SkillSource::Maka,
            precedence: 0,
        },
        document: parse(&format!(
            "---\nname: '{}'\ndescription: work carefully\n---\n{body}",
            name.replace('\'', "''")
        ))
        .unwrap(),
        content_sha256: format!("sha256:{}", "0".repeat(64)),
        shadowed_by: None,
        resources: Default::default(),
    }
}

#[test]
fn execution_fingerprint_covers_frozen_content_and_resolution_inputs() {
    let mut discovery = DiscoverySnapshot {
        inventory: vec![skill("review", "Review", "old instructions")],
        rejected: Vec::new(),
        diagnostics: Vec::new(),
    };
    let mut preferences = Preferences::Available(BTreeMap::new());
    let mut host = HostCapabilities {
        tools: ["Read".into(), "Shell".into()].into(),
        capabilities: ["network".into(), "files".into()].into(),
    };
    let digest =
        |discovery: &DiscoverySnapshot, preferences: &Preferences, host: &HostCapabilities| {
            Catalog {
                discovery,
                preferences,
                host,
            }
            .fingerprint()
            .unwrap()
        };
    let original = digest(&discovery, &preferences, &host);
    host.tools = ["Shell".into(), "Read".into()].into();
    host.capabilities = ["files".into(), "network".into()].into();
    assert_eq!(digest(&discovery, &preferences, &host), original);
    assert_ne!(
        digest(&discovery, &Preferences::Unavailable, &host),
        original
    );

    // Discovery computes this digest over the complete SKILL.md bytes.
    discovery.inventory[0].document.body = "new instructions".into();
    discovery.inventory[0].content_sha256 =
        maka_agent_facts::artifact::content_digest(b"new SKILL.md bytes");
    let content = digest(&discovery, &preferences, &host);
    assert_ne!(
        content, original,
        "unchanged tool schemas cannot hide changed instructions"
    );
    discovery.inventory[0].location.path = "/moved/review".into();
    let moved = digest(&discovery, &preferences, &host);
    assert_ne!(
        moved, content,
        "relative references depend on the discovered path"
    );
    preferences = Preferences::Available(BTreeMap::from([(
        "project:maka:review".into(),
        Preference {
            enabled: false,
            pinned: false,
        },
    )]));
    let disabled = digest(&discovery, &preferences, &host);
    assert_ne!(disabled, moved);
    host.capabilities.remove("network");
    assert_ne!(digest(&discovery, &preferences, &host), disabled);
}

#[test]
fn captured_language_changes_only_builtin_skill_wrappers() {
    use maka_agent_facts::language::Language;
    use maka_skills::PreparedInvocation;
    let original_body = "User skill: preserve 中文 / 繁體 text and <raw> identifiers.";
    let user_message = "Review my OriginalCamelCase diff: \u{1f980}";
    let discovery = DiscoverySnapshot {
        inventory: vec![skill("review", "Original review 名稱", original_body)],
        rejected: vec![],
        diagnostics: vec![],
    };
    let preferences = Preferences::Available(BTreeMap::new());
    let host = HostCapabilities::default();
    let catalog = Catalog {
        discovery: &discovery,
        preferences: &preferences,
        host: &host,
    };
    let mut receipts = None;
    for (language, safety_word, no_task) in [
        (Language::En, "permission", "no additional task text"),
        (Language::ZhCn, "权限", "未提供额外的任务文本"),
        (Language::ZhTw, "權限", "未提供額外的任務文字"),
    ] {
        let PreparedInvocation::Ready {
            text,
            result,
            required_tools,
        } = catalog.prepare_invocation(user_message, &["review".into()], language)
        else {
            panic!("eligible explicit skill should load")
        };
        assert!(text.contains(safety_word));
        assert!(text.contains(original_body));
        assert!(text.contains(user_message));
        assert_eq!(result.loaded[0].id, "review");
        assert_eq!(result.loaded[0].name, "Original review 名稱");
        assert!(required_tools.is_empty());
        let value = serde_json::to_value(result).unwrap();
        if let Some(original) = &receipts {
            assert_eq!(&value, original);
        } else {
            receipts = Some(value);
        }
        let PreparedInvocation::Ready { text, .. } =
            catalog.prepare_invocation("/skill:review", &[], language)
        else {
            panic!("explicit token should load")
        };
        assert!(text.contains(no_task));
        assert!(text.contains(original_body));
        assert!(matches!(
            catalog.prepare_invocation(user_message, &[], language),
            PreparedInvocation::Passthrough
        ));
    }
}

#[test]
fn empty_skill_is_loaded_with_identity_and_original_empty_body() {
    let discovery = DiscoverySnapshot {
        inventory: vec![
            skill("empty", "Empty", ""),
            skill("nonempty", "Nonempty", "literal body 中文"),
        ],
        rejected: vec![],
        diagnostics: vec![],
    };
    let preferences = Preferences::Available(BTreeMap::new());
    let host = HostCapabilities::default();
    let catalog = Catalog {
        discovery: &discovery,
        preferences: &preferences,
        host: &host,
    };
    for (name, body) in [("empty", ""), ("nonempty", "literal body 中文")] {
        let loaded = catalog.load(name).unwrap();
        assert_eq!(loaded.skill.location.id, name);
        assert_eq!(loaded.instructions, body);
        assert_eq!(
            loaded.relative_path,
            Path::new("skills").join(name).join("SKILL.md")
        );
    }
    assert!(catalog.load("missing").is_err());
}
