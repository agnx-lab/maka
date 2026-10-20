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

use cap_fs_ext::MetadataExt;
use cap_std::fs::File;
use maka_agent_facts::tools::ToolError;
use maka_agent_facts::tools::ToolExecutor;
use maka_fs_tools::{MutationExecutor, WriteCoordinator, WriteScope};
use serde_json::{Value, json};
use std::{fs, sync::Arc};
use tokio_util::sync::CancellationToken;

fn executor(root: &std::path::Path) -> MutationExecutor {
    MutationExecutor::new(
        root,
        WriteScope::Restricted {
            roots: vec![root.to_owned()],
        },
        Arc::new(WriteCoordinator::default()),
    )
    .unwrap()
}
async fn edit(executor: &MutationExecutor, input: Value) -> Result<Value, ToolError> {
    executor
        .invoke("Edit".into(), input, CancellationToken::new())
        .await
}

#[tokio::test]
async fn exact_literal_replacement_preserves_inode_and_reports_original_line_span() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    #[cfg(windows)]
    let root = std::path::PathBuf::from(root.to_str().unwrap().strip_prefix(r"\\?\").unwrap());
    let executor = executor(&root);
    let path = root.join("text");
    fs::write(&path, "零\n一\n二\n三\n").unwrap();
    fs::hard_link(&path, root.join("alias")).unwrap();
    let original = File::from_std(fs::File::open(&path).unwrap())
        .metadata()
        .unwrap();
    let result = edit(
        &executor,
        json!({"path":"text","old_string":"一\n二\n","new_string":"$&$1\\n"}),
    )
    .await
    .unwrap();
    assert_eq!(
        result,
        json!({"ok":true,"path":path,"replacements":1,"matchedVia":"exact","startLine":2,"endLine":3})
    );
    assert_eq!(
        fs::read_to_string(root.join("alias")).unwrap(),
        "零\n$&$1\\n三\n"
    );
    let after = File::from_std(fs::File::open(&path).unwrap())
        .metadata()
        .unwrap();
    assert_eq!(
        (after.dev(), after.ino(), after.permissions()),
        (original.dev(), original.ino(), original.permissions())
    );
    fs::write(&path, "aaa").unwrap();
    // The second overlapping "aa" is not a second non-overlapping occurrence.
    edit(
        &executor,
        json!({"path":"text","old_string":"aa","new_string":""}),
    )
    .await
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"a");
    fs::write(&path, "a\nb").unwrap();
    assert_eq!(
        edit(
            &executor,
            json!({"path":"text","old_string":"\n","new_string":"X"})
        )
        .await
        .unwrap()["endLine"],
        1
    );
    assert_eq!(fs::read(&path).unwrap(), b"aXb");
}

#[tokio::test]
async fn invalid_ambiguous_and_missing_edits_leave_bytes_intact() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let executor = executor(&root);
    let path = root.join("text");
    let cases = [
        (
            b"one one".to_vec(),
            json!({"path":"text","old_string":"one","new_string":"two"}),
        ),
        (
            b"one".to_vec(),
            json!({"path":"text","old_string":"","new_string":"two"}),
        ),
        (
            b"one".to_vec(),
            json!({"path":"text","old_string":"one","new_string":"one"}),
        ),
        (
            b"  one".to_vec(),
            json!({"path":"text","old_string":"one  ","new_string":"two"}),
        ),
        (
            vec![0xff, b'x'],
            json!({"path":"text","old_string":"x","new_string":"two"}),
        ),
        (
            vec![b'x'; 1024 * 1024 + 1],
            json!({"path":"text","old_string":"x","new_string":"two"}),
        ),
        (
            b"one".to_vec(),
            json!({"path":"text","old_string":"one","new_string":"two","extra":true}),
        ),
        (
            b"one".to_vec(),
            json!({"path":"text","old_string":"x".repeat(1024 * 1024 + 1),"new_string":"two"}),
        ),
    ];
    for (source, input) in cases {
        fs::write(&path, &source).unwrap();
        assert!(matches!(
            edit(&executor, input).await,
            Err(ToolError::Failed(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), source);
    }
    assert!(
        edit(
            &executor,
            json!({"path":"missing","old_string":"one","new_string":"two"})
        )
        .await
        .is_err()
    );
    assert!(!root.join("missing").exists());
    fs::create_dir(root.join("dir")).unwrap();
    assert!(
        edit(
            &executor,
            json!({"path":"dir","old_string":"one","new_string":"two"})
        )
        .await
        .is_err()
    );
    let denied = MutationExecutor::new(
        &root,
        WriteScope::Disabled,
        Arc::new(WriteCoordinator::default()),
    )
    .unwrap();
    assert!(
        edit(
            &denied,
            json!({"path":"text","old_string":"one","new_string":"two"})
        )
        .await
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), b"one");
}

#[tokio::test]
async fn exact_edit_large_utf8_source_preserves_bytes_inode_and_mode() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    #[cfg(windows)]
    let root = std::path::PathBuf::from(root.to_str().unwrap().strip_prefix(r"\\?\").unwrap());
    let executor = executor(&root);
    let prefix = "unchanged configuration value: 零一二三四五六七八九\n".repeat(20_000);
    let path = root.join("text");
    fs::write(&path, format!("{prefix}target = old\ntrailer\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    }
    fs::hard_link(&path, root.join("alias")).unwrap();
    let before = File::from_std(fs::File::open(&path).unwrap())
        .metadata()
        .unwrap();
    assert_eq!(
        edit(
            &executor,
            json!({"path":"text","old_string":"target = old","new_string":"target = 新"})
        )
        .await
        .unwrap(),
        json!({"ok":true,"path":path,"replacements":1,"matchedVia":"exact","startLine":20_001,"endLine":20_001})
    );
    assert_eq!(
        fs::read(root.join("alias")).unwrap(),
        format!("{prefix}target = 新\ntrailer\n").as_bytes()
    );
    let after = File::from_std(fs::File::open(&path).unwrap())
        .metadata()
        .unwrap();
    assert_eq!(
        (before.dev(), before.ino(), before.permissions()),
        (after.dev(), after.ino(), after.permissions())
    );
}

#[tokio::test]
async fn exact_edit_accepts_large_old_and_new_strings_and_output_growth() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    #[cfg(windows)]
    let root = std::path::PathBuf::from(root.to_str().unwrap().strip_prefix(r"\\?\").unwrap());
    let executor = executor(&root);
    let path = root.join("text");
    let large = "configuration value: 零一二三四五六七八九; ".repeat(24_000);
    for (source, old, new, expected) in [
        (
            format!("before:{large}:after"),
            large.clone(),
            "small".into(),
            "before:small:after".into(),
        ),
        (
            "before:small:after".into(),
            "small".into(),
            large.clone(),
            format!("before:{large}:after"),
        ),
        (
            format!("{}target", "p".repeat(600_000)),
            "target".into(),
            "新".repeat(200_000),
            format!("{}{}", "p".repeat(600_000), "新".repeat(200_000)),
        ),
    ] {
        fs::write(&path, source).unwrap();
        assert_eq!(
            edit(
                &executor,
                json!({"path":"text","old_string":old,"new_string":new})
            )
            .await
            .unwrap(),
            json!({"ok":true,"path":path,"replacements":1,"matchedVia":"exact","startLine":1,"endLine":1})
        );
        assert_eq!(fs::read(&path).unwrap(), expected.as_bytes());
    }
}

#[tokio::test]
async fn fuzzy_edit_accepts_large_utf16_source_and_many_lines() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    #[cfg(windows)]
    let root = std::path::PathBuf::from(root.to_str().unwrap().strip_prefix(r"\\?\").unwrap());
    let executor = executor(&root);
    let path = root.join("text");
    for (line, count) in [
        (
            "unchanged configuration value: 零一二三四五六七八九\n",
            30_000,
        ),
        ("padding\n", 50_001),
    ] {
        let prefix = line.repeat(count);
        fs::write(&path, format!("{prefix}  target value  \ntrailer\n")).unwrap();
        assert_eq!(
            edit(
                &executor,
                json!({"path":"text","old_string":"target value\n","new_string":"updated 新\n"})
            )
            .await
            .unwrap(),
            json!({"ok":true,"path":path,"replacements":1,"matchedVia":"line-trimmed","startLine":count+1,"endLine":count+1})
        );
        assert_eq!(
            fs::read(&path).unwrap(),
            format!("{prefix}updated 新\ntrailer\n").as_bytes()
        );
    }
}
