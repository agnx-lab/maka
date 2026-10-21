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

use crate::scoped::Directory as Dir;
use crate::{ReadExecutor, failed};
use cap_std::fs::{File, OpenOptions, OpenOptionsExt};
use maka_agent_facts::tools::ToolError;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

mod filter;
mod matches;

pub const GREP_NAME: &str = "Grep";
pub const GREP_DESCRIPTION: &str = "Search file contents with a ripgrep-compatible regular expression. Optional path selects a file or directory; glob filters files. Returns matches and complete. Results cover all matching lines in the admitted search scope. Hidden, binary and ignored files are excluded during directory traversal. No fuzzy fallback.";

pub fn grep_schema() -> Value {
    schemars::schema_for!(Input).into()
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    pattern: String,
    #[serde(default = "default_path")]
    path: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "String")]
    glob: Option<String>,
}
fn default_path() -> String {
    ".".into()
}
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    String::deserialize(d).map(Some)
}

impl ReadExecutor {
    pub async fn grep(
        &self,
        input: Value,
        cancellation: CancellationToken,
    ) -> Result<Value, ToolError> {
        let input: Input = serde_json::from_value(input).map_err(|e| failed(e.to_string()))?;
        if input.path.contains("://") {
            return Err(failed("Grep requires a filesystem path"));
        }
        let matcher = matches::Regex::new(&input.pattern)?;
        let authority = self.authority.clone();
        tokio::task::spawn_blocking(move || {
            let mut search = Search {
                matcher,
                cancellation,
                output: Vec::new(),
            };
            search.check()?;
            let requested = if input.path.is_empty() {
                "."
            } else {
                &input.path
            };
            let mut selected = None;
            let mut error = failed("Grep path is outside the admitted Read roots");
            for route in authority.read_routes(Path::new(requested))? {
                let relative = if route.relative.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    route.relative
                };
                match open(&route.root, &relative) {
                    Ok(file) => {
                        selected = Some((route.root, route.root_path, relative, file));
                        break;
                    }
                    Err(reason) => error = io_error(reason),
                }
            }
            let (root, root_path, relative, file) = selected.ok_or(error)?;
            let metadata = file.metadata().map_err(io_error)?;
            if metadata.is_file() {
                search.file(file, None)?;
            } else if metadata.is_dir() {
                let target = root.canonicalize(&relative).map_err(io_error)?;
                let display: PathBuf = root_path.join(&target).components().collect();
                let display = dunce::simplified(&display);
                let mut filters = filter::Filters::new(
                    input.glob.as_deref(),
                    dunce::simplified(authority.cwd()),
                    search.cancellation.clone(),
                )?;
                // Read ancestor rules only through the already admitted root.
                for ancestor in target
                    .ancestors()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                {
                    filters.push(&root, ancestor)?;
                }
                search.walk(&root, &target, &target, display, &mut filters)?;
            } else {
                return Err(failed("Grep supports only regular files and directories"));
            }
            search.check()?;
            Ok(json!({"matches":search.output,"complete":true}))
        })
        .await
        .map_err(|e| failed(format!("Grep worker failed: {e}")))?
    }
}

struct Search {
    matcher: matches::Regex,
    cancellation: CancellationToken,
    output: Vec<String>,
}
impl Search {
    fn check(&self) -> Result<(), ToolError> {
        if self.cancellation.is_cancelled() {
            return Err(failed("Grep cancelled"));
        }
        Ok(())
    }
    fn file(&mut self, file: File, label: Option<&Path>) -> Result<(), ToolError> {
        self.check()?;
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() {
            return Err(failed("Grep target changed to a nonregular file"));
        }
        let mut bytes = Vec::new();
        let mut file = file.take(metadata.len());
        let mut chunk = [0; 8192];
        while file.limit() != 0 {
            self.check()?;
            let count = file.read(&mut chunk).map_err(io_error)?;
            if count == 0 {
                return Err(failed("Grep source changed within its captured extent"));
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        let decoded = encoding_rs::Encoding::for_bom(&bytes)
            .map(|(encoding, _)| encoding.decode_with_bom_removal(&bytes).0);
        let bytes = decoded
            .as_ref()
            .map_or(bytes.as_slice(), |text| text.as_bytes());
        if bytes.contains(&0) {
            return Ok(());
        }
        self.matcher
            .search(bytes, label, &self.cancellation, &mut self.output)?;
        self.check()
    }
    fn walk(
        &mut self,
        root: &Dir,
        path: &Path,
        target: &Path,
        display: &Path,
        filters: &mut filter::Filters,
    ) -> Result<(), ToolError> {
        enum Work {
            Enter(PathBuf),
            Child(PathBuf, bool, bool),
            Leave(String),
        }
        let mut pending = vec![Work::Enter(path.to_owned())];
        let mut active = std::collections::BTreeSet::new();
        while let Some(work) = pending.pop() {
            self.check()?;
            match work {
                Work::Leave(identity) => {
                    filters.pop();
                    active.remove(&identity);
                }
                Work::Enter(path) => {
                    let identity = root
                        .open_dir(&path)
                        .and_then(|directory| directory.identity())
                        .map_err(io_error)?;
                    if !active.insert(identity.clone()) {
                        continue;
                    }
                    filters.push(root, &path)?;
                    pending.push(Work::Leave(identity));
                    let mut children = Vec::new();
                    for entry in root.read_dir(&path).map_err(io_error)? {
                        self.check()?;
                        let entry = entry.map_err(io_error)?;
                        let kind = entry.file_type().map_err(io_error)?;
                        if !kind.is_dir() && !kind.is_file() {
                            continue;
                        }
                        #[cfg(unix)]
                        let hidden = false;
                        #[cfg(windows)]
                        let hidden = {
                            use cap_std::fs::MetadataExt;
                            entry.metadata().map_err(io_error)?.file_attributes()
                                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN
                                != 0
                        };
                        children.push((entry.file_name(), kind.is_dir(), hidden));
                    }
                    children.sort_by(|a, b| a.0.cmp(&b.0));
                    for (name, directory, hidden) in children.into_iter().rev() {
                        pending.push(Work::Child(path.join(name), directory, hidden));
                    }
                }
                Work::Child(child, directory, hidden) => {
                    if let Err(error) = root.check_read(&child) {
                        if error.kind() == std::io::ErrorKind::PermissionDenied {
                            continue;
                        }
                        return Err(io_error(error));
                    }
                    let label = display.join(
                        child
                            .strip_prefix(target)
                            .map_err(|e| failed(e.to_string()))?,
                    );
                    if filters.excludes(&child, &label, directory, hidden) {
                        continue;
                    }
                    if directory {
                        pending.push(Work::Enter(child));
                    } else {
                        self.file(open(root, &child).map_err(io_error)?, Some(&label))?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn open(root: &Dir, path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    #[cfg(windows)]
    options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS);
    root.open_with(path, &options)
}
fn io_error(e: std::io::Error) -> ToolError {
    ToolError::Io {
        kind: e.kind(),
        message: format!("Grep filesystem error: {e}"),
    }
}
