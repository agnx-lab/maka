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
use std::{fs, path::PathBuf, process::Command, time::SystemTime};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    private: PathBuf,
    identity: DirectoryIdentity,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repository");
        let private = temp.path().join("private");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&private).unwrap();
        git(&root, &["init", "--quiet"]);
        git(&root, &["config", "user.name", "Maka tests"]);
        git(&root, &["config", "user.email", "tests@maka.invalid"]);
        let (root, identity) = crate::directory::capture(&root).unwrap();
        Self {
            _temp: temp,
            root,
            private,
            identity,
        }
    }
    fn commit(&self) {
        git(&self.root, &["add", "."]);
        git(&self.root, &["commit", "--quiet", "-m", "base"]);
    }
    fn status(&self) -> GitStatus {
        status(&self.root, &self.identity, &self.private, None, cancel())
            .unwrap()
            .unwrap()
    }
    fn diff(&self, area: GitArea, name: &[u8]) -> GitDiff {
        diff(
            &self.root,
            &self.identity,
            &self.private,
            None,
            area,
            &WorkspacePath::from_bytes(name).unwrap(),
            cancel(),
        )
        .unwrap()
        .unwrap()
    }
}
fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn git(root: &Path, args: &[&str]) {
    git_stdout(root, args);
}
fn git_stdout(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn entries(status: GitStatus) -> Vec<(GitArea, Vec<u8>, GitChangeKind)> {
    status
        .entries
        .into_iter()
        .map(|e| (e.area, e.path.bytes().unwrap(), e.kind))
        .collect()
}

#[test]
fn staged_worktree_and_untracked_are_distinct_and_diff_literal_images() {
    let f = Fixture::new();
    fs::write(f.root.join("file.txt"), "original\n").unwrap();
    fs::write(f.root.join("deleted.txt"), "delete me\n").unwrap();
    f.commit();
    fs::write(f.root.join("file.txt"), "staged\n").unwrap();
    git(&f.root, &["add", "file.txt"]);
    fs::write(f.root.join("file.txt"), "working\n").unwrap();
    fs::remove_file(f.root.join("deleted.txt")).unwrap();
    fs::write(f.root.join("untracked.txt"), "new content\n").unwrap();
    assert_eq!(
        entries(f.status()),
        vec![
            (
                GitArea::Staged,
                b"file.txt".to_vec(),
                GitChangeKind::Modified
            ),
            (
                GitArea::Worktree,
                b"deleted.txt".to_vec(),
                GitChangeKind::Deleted
            ),
            (
                GitArea::Worktree,
                b"file.txt".to_vec(),
                GitChangeKind::Modified
            ),
            (
                GitArea::Untracked,
                b"untracked.txt".to_vec(),
                GitChangeKind::Added
            ),
        ]
    );
    let GitDiff::Text { patch } = f.diff(GitArea::Staged, b"file.txt") else {
        panic!("expected staged text")
    };
    assert!(
        patch.contains("@@ -1,1 +1,1 @@\n-original\n+staged\n"),
        "{patch}"
    );
    assert!(!patch.contains("working"));
    let GitDiff::Text { patch } = f.diff(GitArea::Worktree, b"file.txt") else {
        panic!("expected working text")
    };
    assert!(
        patch.contains("@@ -1,1 +1,1 @@\n-staged\n+working\n"),
        "{patch}"
    );
    assert_eq!(f.diff(GitArea::Untracked, b"file.txt"), GitDiff::Unchanged);
    let GitDiff::Text { patch } = f.diff(GitArea::Untracked, b"untracked.txt") else {
        panic!("expected untracked text")
    };
    assert!(
        patch.contains("--- /dev/null\n+++ \"b/untracked.txt\"\n@@ -1,0 +1,1 @@\n+new content\n"),
        "{patch}"
    );
}

#[test]
fn unborn_repository_uses_empty_head_and_prunes_ignored_dependency_tree() {
    let f = Fixture::new();
    fs::write(f.root.join(".gitignore"), "node_modules/\n").unwrap();
    fs::create_dir(f.root.join("node_modules")).unwrap();
    // If the preflight enters this ignored tree, this invalid control file fails it.
    fs::create_dir(f.root.join("node_modules/.gitignore")).unwrap();
    fs::write(f.root.join("new.txt"), "before commit\n").unwrap();
    git(&f.root, &["add", "new.txt"]);
    let result = f.status();
    assert_eq!(result.head, None);
    assert_eq!(
        entries(result),
        vec![
            (GitArea::Staged, b"new.txt".to_vec(), GitChangeKind::Added),
            (
                GitArea::Untracked,
                b".gitignore".to_vec(),
                GitChangeKind::Added
            ),
        ]
    );
    let GitDiff::Text { patch } = f.diff(GitArea::Staged, b"new.txt") else {
        panic!("expected unborn staged text")
    };
    assert!(patch.contains("+before commit\n"));
    // Ignoring a directory does not hide a file already tracked inside it.
    fs::remove_dir(f.root.join("node_modules/.gitignore")).unwrap();
    fs::write(f.root.join("node_modules/tracked"), "tracked before\n").unwrap();
    git(&f.root, &["add", "-f", "node_modules/tracked"]);
    f.commit();
    fs::write(f.root.join("node_modules/tracked"), "tracked after\n").unwrap();
    assert_eq!(
        entries(f.status()),
        vec![(
            GitArea::Worktree,
            b"node_modules/tracked".to_vec(),
            GitChangeKind::Modified
        )]
    );
}

struct ReadOnlySnapshot {
    index: Vec<u8>,
    index_modified: SystemTime,
    head: Vec<u8>,
    files: Vec<(String, Vec<u8>, SystemTime)>,
}
impl ReadOnlySnapshot {
    fn capture(f: &Fixture, names: &[&str]) -> Self {
        let index = f.root.join(".git/index");
        Self {
            index: fs::read(&index).unwrap(),
            index_modified: fs::metadata(&index).unwrap().modified().unwrap(),
            head: git_stdout(&f.root, &["rev-parse", "HEAD"]),
            files: names
                .iter()
                .map(|name| {
                    let path = f.root.join(name);
                    (
                        (*name).to_owned(),
                        fs::read(&path).unwrap(),
                        fs::metadata(&path).unwrap().modified().unwrap(),
                    )
                })
                .collect(),
        }
    }
    fn assert_unchanged(&self, f: &Fixture) {
        let index = f.root.join(".git/index");
        assert_eq!(fs::read(&index).unwrap(), self.index);
        assert_eq!(
            fs::metadata(&index).unwrap().modified().unwrap(),
            self.index_modified
        );
        assert_eq!(git_stdout(&f.root, &["rev-parse", "HEAD"]), self.head);
        for (name, bytes, modified) in &self.files {
            let path = f.root.join(name);
            assert_eq!(
                fs::read(&path).unwrap(),
                *bytes,
                "worktree bytes changed: {name}"
            );
            assert_eq!(
                fs::metadata(&path).unwrap().modified().unwrap(),
                *modified,
                "worktree mtime changed: {name}"
            );
        }
    }
}

fn assert_patch_reconstructs(
    diff: GitDiff,
    name: &str,
    before: Option<&[u8]>,
    expected: &[u8],
) -> String {
    let GitDiff::Text { patch } = diff else {
        panic!("expected complete text patch, got {diff:?}")
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("apply");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "--quiet"]);
    if let Some(before) = before {
        fs::write(root.join(name), before).unwrap();
    }
    let patch_file = temp.path().join("preview.patch");
    fs::write(&patch_file, &patch).unwrap();
    git(
        &root,
        &["apply", "--whitespace=nowarn", patch_file.to_str().unwrap()],
    );
    assert_eq!(fs::read(root.join(name)).unwrap(), expected);
    patch
}

#[test]
fn binary_previews_do_not_return_partial_text() {
    let f = Fixture::new();
    fs::write(f.root.join("binary"), [0, 255, 10]).unwrap();
    assert_eq!(f.diff(GitArea::Untracked, b"binary"), GitDiff::Binary);
    let mut large_binary = vec![b'x'; 1_100_000];
    large_binary[0] = 0;
    large_binary[1_099_999] = 255;
    fs::write(f.root.join("large-binary"), &large_binary).unwrap();
    assert_eq!(f.diff(GitArea::Untracked, b"large-binary"), GitDiff::Binary);
    assert_eq!(fs::read(f.root.join("large-binary")).unwrap(), large_binary);
}

#[test]
fn large_staged_and_worktree_images_keep_distinct_small_changes() {
    let f = Fixture::new();
    let prefix = "unchanged configuration: 零一二三四五六七八九\n".repeat(24_000);
    let head = format!("{prefix}selected = head\n");
    let staged = format!("{prefix}selected = staged\n");
    let working = format!("{prefix}selected = working\n");
    fs::write(f.root.join("config.txt"), &head).unwrap();
    f.commit();
    let equal_snapshot = ReadOnlySnapshot::capture(&f, &["config.txt"]);
    assert_eq!(f.diff(GitArea::Staged, b"config.txt"), GitDiff::Unchanged);
    assert_eq!(f.diff(GitArea::Worktree, b"config.txt"), GitDiff::Unchanged);
    equal_snapshot.assert_unchanged(&f);
    fs::write(f.root.join("config.txt"), &staged).unwrap();
    git(&f.root, &["add", "config.txt"]);
    fs::write(f.root.join("config.txt"), &working).unwrap();
    let snapshot = ReadOnlySnapshot::capture(&f, &["config.txt"]);
    assert_patch_reconstructs(
        f.diff(GitArea::Staged, b"config.txt"),
        "config.txt",
        Some(head.as_bytes()),
        staged.as_bytes(),
    );
    assert_patch_reconstructs(
        f.diff(GitArea::Worktree, b"config.txt"),
        "config.txt",
        Some(staged.as_bytes()),
        working.as_bytes(),
    );
    snapshot.assert_unchanged(&f);
}

#[test]
fn large_untracked_text_returns_complete_applicable_patch() {
    let f = Fixture::new();
    fs::write(f.root.join("anchor"), "committed anchor\n").unwrap();
    f.commit();
    let content = "configuration: 零一二三四五六七八九\n".repeat(30_000);
    fs::write(f.root.join("large.txt"), &content).unwrap();
    let snapshot = ReadOnlySnapshot::capture(&f, &["anchor", "large.txt"]);
    assert_patch_reconstructs(
        f.diff(GitArea::Untracked, b"large.txt"),
        "large.txt",
        None,
        content.as_bytes(),
    );
    snapshot.assert_unchanged(&f);
}

#[test]
fn json_escaping_does_not_discard_a_complete_text_patch() {
    let f = Fixture::new();
    fs::write(f.root.join("anchor"), "committed anchor\n").unwrap();
    f.commit();
    let content = "\"".repeat(262_154);
    fs::write(f.root.join("quotes"), &content).unwrap();
    let snapshot = ReadOnlySnapshot::capture(&f, &["anchor", "quotes"]);
    let result = f.diff(GitArea::Untracked, b"quotes");
    let serialized_bytes = serde_json::to_vec(&result).unwrap().len();
    let patch = assert_patch_reconstructs(result, "quotes", None, content.as_bytes());
    assert!(
        patch.len() < 512 * 1024,
        "raw patch fits the former output budget"
    );
    assert!(
        serialized_bytes > 512 * 1024,
        "JSON escaping alone crosses the former output budget"
    );
    snapshot.assert_unchanged(&f);
}

#[test]
fn many_changed_lines_return_complete_applicable_patch() {
    let f = Fixture::new();
    let before = "old\n".repeat(100_000);
    let after = "new\n".repeat(100_000);
    fs::write(f.root.join("many-lines"), &before).unwrap();
    f.commit();
    fs::write(f.root.join("many-lines"), &after).unwrap();
    let snapshot = ReadOnlySnapshot::capture(&f, &["many-lines"]);
    let patch = assert_patch_reconstructs(
        f.diff(GitArea::Worktree, b"many-lines"),
        "many-lines",
        Some(before.as_bytes()),
        after.as_bytes(),
    );
    assert!(
        patch.len() > 512 * 1024,
        "the full changed-line patch crosses the former output budget"
    );
    snapshot.assert_unchanged(&f);
}

#[test]
fn large_unselected_tracked_file_does_not_block_small_comparison() {
    let f = Fixture::new();
    fs::write(f.root.join("small"), "before\n").unwrap();
    fs::write(f.root.join("large"), "tracked prefix\n").unwrap();
    f.commit();
    fs::OpenOptions::new()
        .write(true)
        .open(f.root.join("large"))
        .unwrap()
        .set_len(50 * 1024 * 1024 + 1)
        .unwrap();
    fs::write(f.root.join("small"), "after\n").unwrap();
    let snapshot = ReadOnlySnapshot::capture(&f, &["small", "large"]);
    assert_patch_reconstructs(
        f.diff(GitArea::Worktree, b"small"),
        "small",
        Some(b"before\n"),
        b"after\n",
    );
    snapshot.assert_unchanged(&f);
}

#[test]
fn status_marks_the_bounded_serialized_prefix_and_preserves_actual_paths() {
    let f = Fixture::new();
    for i in 0..1800 {
        fs::write(f.root.join(format!("{i:04}-{}", "x".repeat(190))), "").unwrap();
    }
    let result = f.status();
    assert!(result.truncated);
    assert!(serde_json::to_vec(&result).unwrap().len() <= MAX_OUTPUT);
    assert_eq!(
        result.entries[0].path.bytes().unwrap(),
        format!("0000-{}", "x".repeat(190)).as_bytes()
    );
    let last = result.entries.last().unwrap().path.bytes().unwrap();
    assert!(
        f.root
            .join(gix::path::from_bstr(last.as_bstr()).as_ref())
            .is_file()
    );
}

#[test]
fn linked_and_managed_worktrees_use_original_metadata_without_private_root_escape() {
    let f = Fixture::new();
    fs::write(f.root.join("source.txt"), "source\n").unwrap();
    f.commit();
    let linked = f._temp.path().join("linked");
    git(
        &f.root,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked.to_str().unwrap(),
        ],
    );
    let (linked, identity) = crate::directory::capture(&linked).unwrap();
    fs::write(linked.join("source.txt"), "linked edit\n").unwrap();
    assert_eq!(
        entries(
            status(&linked, &identity, &f.private, None, cancel())
                .unwrap()
                .unwrap()
        ),
        vec![(
            GitArea::Worktree,
            b"source.txt".to_vec(),
            GitChangeKind::Modified
        )]
    );
    let manager = crate::worktree::Worktrees::open(&f.private.join("subagent-worktrees")).unwrap();
    let binding = manager.plan(&f.root, &"1".repeat(64), cancel()).unwrap();
    manager.ensure(&binding, &AtomicBool::new(false)).unwrap();
    let (root, identity) = crate::directory::capture(binding.directory()).unwrap();
    fs::write(root.join("source.txt"), "managed edit\n").unwrap();
    assert_eq!(
        entries(
            status(&root, &identity, &f.private, Some(&binding), cancel())
                .unwrap()
                .unwrap()
        ),
        vec![(
            GitArea::Worktree,
            b"source.txt".to_vec(),
            GitChangeKind::Modified
        )]
    );
    assert_eq!(
        status(&root, &identity, &f.private, None, cancel())
            .unwrap_err()
            .to_string(),
        "git_private"
    );
}

#[test]
fn filters_subdirectories_private_overlap_and_cancellation_fail_with_static_codes() {
    let f = Fixture::new();
    fs::write(f.root.join("tracked"), "safe\n").unwrap();
    f.commit();
    fs::create_dir(f.root.join("subdirectory")).unwrap();
    let (sub, identity) = crate::directory::capture(&f.root.join("subdirectory")).unwrap();
    assert_eq!(
        status(&sub, &identity, &f.private, None, cancel())
            .unwrap_err()
            .to_string(),
        "git_unsupported"
    );
    let nested_private = f.root.join("state");
    fs::create_dir(&nested_private).unwrap();
    assert_eq!(
        status(&f.root, &f.identity, &nested_private, None, cancel())
            .unwrap_err()
            .to_string(),
        "git_private"
    );
    let cancelled = Arc::new(AtomicBool::new(true));
    assert_eq!(
        status(&f.root, &f.identity, &f.private, None, cancelled)
            .unwrap_err()
            .to_string(),
        "git_cancelled"
    );
    let sentinel = f._temp.path().join("executed");
    git(
        &f.root,
        &[
            "config",
            "filter.malicious.clean",
            &format!("touch {}", sentinel.display()),
        ],
    );
    fs::write(f.root.join(".gitattributes"), "tracked filter=malicious\n").unwrap();
    assert_eq!(
        status(&f.root, &f.identity, &f.private, None, cancel())
            .unwrap_err()
            .to_string(),
        "git_unsupported"
    );
    assert!(!sentinel.exists());
}

#[cfg(unix)]
#[test]
fn raw_names_and_link_content_roundtrip_without_following_control_file_escapes() {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let f = Fixture::new();
    // APFS rejects ill-formed UTF-8 names before any Git/backend operation.
    #[cfg(target_os = "macos")]
    let name = b"raw-\\name";
    #[cfg(not(target_os = "macos"))]
    let name = b"raw-\xff\\name";
    fs::write(
        f.root.join(std::ffi::OsString::from_vec(name.to_vec())),
        "raw name\n",
    )
    .unwrap();
    assert_eq!(
        entries(f.status()),
        vec![(GitArea::Untracked, name.to_vec(), GitChangeKind::Added)]
    );
    assert!(matches!(
        f.diff(GitArea::Untracked, name),
        GitDiff::Text { .. }
    ));
    fs::write(f.private.join("secret"), "private sentinel").unwrap();
    symlink(f.private.join("secret"), f.root.join("link")).unwrap();
    let GitDiff::Text { patch } = f.diff(GitArea::Untracked, b"link") else {
        panic!("expected link literal")
    };
    assert!(!patch.contains("private sentinel"));
    symlink(f.private.join("secret"), f.root.join(".gitignore")).unwrap();
    assert_eq!(
        status(&f.root, &f.identity, &f.private, None, cancel())
            .unwrap_err()
            .to_string(),
        "git_unsupported"
    );
    fs::remove_file(f.root.join(".gitignore")).unwrap();
    fs::create_dir_all(f.root.join(".git/objects/info")).unwrap();
    fs::write(
        f.root.join(".git/objects/info/alternates"),
        f.private.to_string_lossy().as_bytes(),
    )
    .unwrap();
    assert_eq!(
        status(&f.root, &f.identity, &f.private, None, cancel())
            .unwrap_err()
            .to_string(),
        "git_unsupported"
    );
}

#[test]
fn no_repository_is_distinct_from_invalid_path_and_changed_directory_identity() {
    let f = Fixture::new();
    fs::remove_dir_all(f.root.join(".git")).unwrap();
    assert_eq!(
        status(&f.root, &f.identity, &f.private, None, cancel()).unwrap(),
        None
    );
    assert_eq!(
        WorkspacePath::from_bytes(b"../private")
            .unwrap_err()
            .to_string(),
        "workspace_path_invalid"
    );
    assert_eq!(
        WorkspacePath::from_bytes(b".git/config")
            .unwrap_err()
            .to_string(),
        "workspace_path_invalid"
    );
    let renamed = f._temp.path().join("renamed");
    fs::rename(&f.root, &renamed).unwrap();
    fs::create_dir(&f.root).unwrap();
    assert_eq!(
        status(&f.root, &f.identity, &f.private, None, cancel())
            .unwrap_err()
            .to_string(),
        "git_unavailable"
    );
}

#[test]
fn conflicted_index_is_visible_but_has_no_fabricated_two_way_diff() {
    let f = Fixture::new();
    fs::write(f.root.join("conflict"), "base\n").unwrap();
    f.commit();
    git(&f.root, &["checkout", "--quiet", "-b", "ours"]);
    fs::write(f.root.join("conflict"), "ours\n").unwrap();
    f.commit();
    git(&f.root, &["checkout", "--quiet", "-b", "theirs", "HEAD~1"]);
    fs::write(f.root.join("conflict"), "theirs\n").unwrap();
    f.commit();
    let output = Command::new("git")
        .current_dir(&f.root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .args(["merge", "--no-edit", "ours"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        entries(f.status()),
        vec![(
            GitArea::Worktree,
            b"conflict".to_vec(),
            GitChangeKind::Conflicted
        ),]
    );
    assert_eq!(f.diff(GitArea::Staged, b"conflict"), GitDiff::Unsupported);
    assert_eq!(f.diff(GitArea::Worktree, b"conflict"), GitDiff::Unsupported);
}

#[cfg(unix)]
#[test]
fn case_renamed_tracked_directory_cannot_hide_an_ignored_control_symlink() {
    use std::os::unix::fs::symlink;
    for (original, renamed, pattern) in
        [("DIR", "dir", "dir/\n"), ("Café", "cafe\u{301}", "café/\n")]
    {
        let f = Fixture::new();
        git(&f.root, &["config", "core.ignorecase", "true"]);
        git(&f.root, &["config", "core.precomposeunicode", "true"]);
        fs::create_dir(f.root.join(original)).unwrap();
        fs::write(f.root.join(original).join("tracked"), "tracked\n").unwrap();
        fs::write(
            f.root.join(original).join(".gitattributes"),
            vec![b'#'; MAX_CONTROL + 1],
        )
        .unwrap();
        f.commit();
        fs::write(f.root.join(".gitignore"), pattern).unwrap();
        f.commit();
        fs::remove_file(f.root.join(original).join(".gitattributes")).unwrap();
        fs::rename(f.root.join(original), f.root.join(renamed)).unwrap();
        fs::write(f.root.join(renamed).join("tracked"), "modified tracked\n").unwrap();
        let private_ignore = f.private.join("ignore");
        fs::write(&private_ignore, "private-ignore-sentinel\n").unwrap();
        symlink(&private_ignore, f.root.join(renamed).join(".gitignore")).unwrap();
        let result = status(&f.root, &f.identity, &f.private, None, cancel());
        fs::remove_file(f.root.join(renamed).join(".gitignore")).unwrap();
        let indexed_attributes = status(&f.root, &f.identity, &f.private, None, cancel());
        drop(f);
        assert_eq!(
            indexed_attributes.unwrap_err().to_string(),
            "git_too_large",
            "{original:?} -> {renamed:?}"
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "git_unsupported",
            "{original:?} -> {renamed:?}"
        );
    }
}

#[test]
fn metadata_budget_is_not_bypassed_by_an_objects_named_workspace_ancestor() {
    use std::io::Write;
    let mut f = Fixture::new();
    fs::write(f.root.join("tracked"), "safe\n").unwrap();
    f.commit();
    let parent = f._temp.path().join("objects");
    fs::create_dir(&parent).unwrap();
    let renamed = parent.join("repository");
    fs::rename(&f.root, &renamed).unwrap();
    f.root = renamed;
    // A valid comment keeps this a real readable Git configuration, not a parser failure.
    let mut config = fs::OpenOptions::new()
        .append(true)
        .open(f.root.join(".git/config"))
        .unwrap();
    config.write_all(b"\n#").unwrap();
    let chunk = [b'x'; 8192];
    for _ in 0..MAX_METADATA / chunk.len() {
        config.write_all(&chunk).unwrap();
    }
    drop(config);
    let result = status(&f.root, &f.identity, &f.private, None, cancel());
    drop(f);
    assert_eq!(result.unwrap_err().to_string(), "git_too_large");
}

#[cfg(unix)]
#[test]
fn split_index_reads_only_bounded_regular_shared_indexes() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    fs::write(f.root.join("tracked"), "safe\n").unwrap();
    f.commit();
    git(&f.root, &["update-index", "--split-index"]);
    assert_eq!(entries(f.status()), vec![]);
    let shared = fs::read_dir(f.root.join(".git"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .as_encoded_bytes()
                .starts_with(b"sharedindex.")
        })
        .unwrap();
    let bytes = fs::read(&shared).unwrap();
    let private_copy = f.private.join("sharedindex");
    fs::write(&private_copy, &bytes).unwrap();
    fs::remove_file(&shared).unwrap();
    symlink(&private_copy, &shared).unwrap();
    let result = status(&f.root, &f.identity, &f.private, None, cancel());
    fs::remove_file(&shared).unwrap();
    fs::write(&shared, &bytes).unwrap();
    #[cfg(target_os = "macos")]
    let uppercase_result = {
        let suffix = shared
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_prefix("sharedindex.")
            .unwrap();
        let uppercase = shared.with_file_name(format!("SHAREDINDEX.{suffix}"));
        fs::rename(&shared, &uppercase).unwrap();
        assert_eq!(entries(f.status()), vec![]);
        fs::remove_file(&uppercase).unwrap();
        symlink(&private_copy, &uppercase).unwrap();
        let result = status(&f.root, &f.identity, &f.private, None, cancel());
        fs::remove_file(&uppercase).unwrap();
        fs::write(&shared, &bytes).unwrap();
        result
    };
    fs::OpenOptions::new()
        .write(true)
        .open(&shared)
        .unwrap()
        .set_len(MAX_METADATA as u64 + 1)
        .unwrap();
    let oversized = status(&f.root, &f.identity, &f.private, None, cancel());
    drop(f);
    assert_eq!(result.unwrap_err().to_string(), "git_unsupported");
    assert_eq!(oversized.unwrap_err().to_string(), "git_too_large");
    #[cfg(target_os = "macos")]
    assert_eq!(uppercase_result.unwrap_err().to_string(), "git_unsupported");
}
