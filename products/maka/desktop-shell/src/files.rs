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

//! The `maka-files` script module: a text file or a folder the user picks,
//! and the files the user attaches to a message.
//!
//! A gpui-shell manifest can grant reads only under the application's own
//! directories, and scripts have no file dialog. `openText` shows the
//! platform's open panel and reads the one file the user chose, so choosing
//! the file is the permission, as with a browser's file input. Nothing else
//! on disk becomes readable. `chooseFolder` shows the panel for one folder
//! and answers its path only; the Host, not the script, reads the folder.
//!
//! `chooseFiles` shows the panel for any number of files and registers each
//! one under an opaque handle, with its size and content digest. The script
//! gets the handle, never the path, and has no way to register a path itself:
//! here too the choice in the panel is the permission. `maka-host` reads a
//! registered file when a mutation names its handle as an attachment (the
//! `file` of `agent.uploadAttachment`), and `release` forgets it.
//!
//! `open` and `reveal` hand a path in a task's folder to the system: open a
//! file with its default application, or show a file or folder in the file
//! manager, as the Files tool's menu offers. They read nothing and answer
//! nothing about the file. Off the main thread they check that the path,
//! with the links in its folders followed, is inside the task's folder;
//! `open` also refuses a link, a folder, and, unless the user agreed, a file
//! that runs a program when it is opened.
use gpui_shell::{
    HostArguments, HostError, HostModule, HostObject, HostResult, HostValue,
    gpui::{App, PathPromptOptions, SharedString},
};
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
};
use uuid::Uuid;

pub const NAME: &str = "maka-files";
/// Read limit when the script does not give one.
const DEFAULT_MAX_BYTES: u64 = 1 << 20;
/// Largest limit a script may ask for.
const MAX_BYTES: u64 = 16 << 20;
/// Largest file `chooseFiles` registers: the Host's limit for an attachment
/// (`agent.uploadAttachment`, 52428800 bytes).
const MAX_ATTACHMENT_BYTES: u64 = 50 << 20;
/// How much of a file is read at a time while its digest is computed.
const DIGEST_CHUNK: usize = 64 * 1024;

const DECLARATIONS: &str = r#"/**
 * Shows the open panel for one file and reads it as UTF-8 text. Resolves with
 * null when the user cancels. Rejects when the file is larger than `maxBytes`
 * (default 1 MiB, at most 16 MiB) or is not UTF-8.
 */
export function openText(options?: {
  prompt?: string;
  maxBytes?: number;
}): Promise<{ path: string; text: string } | null>;

/**
 * Shows the open panel for one folder. Resolves with its path, or null when
 * the user cancels.
 */
export function chooseFolder(options?: { prompt?: string }): Promise<string | null>;

/**
 * One file chosen with `chooseFiles`. A registered file has a `handle`, its
 * size and its `digest` (`sha256:` and the lowercase hex SHA-256 of its
 * content). A file over 50 MiB (52428800 bytes, the Host's limit for an
 * attachment) is not registered and has `tooLarge`; one that cannot be read
 * is not registered and has `error`.
 */
export interface ChosenFile {
  name: string;
  bytes?: number;
  handle?: string;
  digest?: string;
  tooLarge?: boolean;
  error?: string;
}

/**
 * Shows the open panel for any number of files and registers each one that
 * can be attached. `maka-host`'s `mutate` reads a registered file when an
 * attachment names it as `{ file: handle }`. Sizes and digests are read off
 * the main thread. Resolves with [] when the user cancels.
 */
export function chooseFiles(): Promise<ChosenFile[]>;

/** Forgets registered files. A handle that is not registered is ignored. */
export function release(handles: string[]): void;

/**
 * Opens the file at `path`, in the task's folder `root`, with the system's
 * default application. Both are absolute, without a `..` component. Rejects
 * when `path` does not exist, is a link, a folder or not a regular file, or
 * is outside `root` once the links in its folders are followed. A file that
 * can run a program when it is opened (one with an execute bit, or an
 * application, script, installer or location file by its extension) is
 * refused with a message that starts, after the function's name, with
 * `launchable:`, unless `options.launch` is true: the script passes that only
 * after the user agreed. Whether the application then opened it is not
 * reported.
 */
export function open(path: string, root: string, options?: { launch?: boolean }): Promise<void>;

/**
 * Shows the file or folder at `path`, in the task's folder `root`, in the
 * file manager. A link there is shown as itself, even one whose target is
 * gone; the links in its folders are followed, and it is refused when that
 * leads outside `root`.
 */
export function reveal(path: string, root: string): Promise<void>;
"#;

pub fn module() -> HostModule {
    HostModule::new(NAME)
        .declarations(DECLARATIONS)
        .async_function("openText", |arguments| {
            let (prompt, limit) = options(arguments)?;
            let chosen = gpui_shell::with_current_app(|cx| {
                cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt,
                })
            })
            .ok_or_else(|| HostError::new("openText must be called from the application"))?;
            Ok(async move {
                let paths = chosen
                    .await
                    .map_err(|_| HostError::new("the open panel closed without an answer"))?
                    .map_err(|error| HostError::new(format!("the open panel failed: {error}")))?;
                let Some(path) = paths.and_then(|paths| paths.into_iter().next()) else {
                    return Ok(HostValue::Null);
                };
                let text = read_text(&path, limit)?;
                Ok(HostObject::new()
                    .field("path", path.to_string_lossy().into_owned())
                    .field("text", text)
                    .into())
            })
        })
        .async_function("chooseFolder", |arguments| {
            let prompt = folder_options(arguments)?;
            let chosen = gpui_shell::with_current_app(|cx| {
                cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt,
                })
            })
            .ok_or_else(|| HostError::new("chooseFolder must be called from the application"))?;
            Ok(async move {
                let paths = chosen
                    .await
                    .map_err(|_| HostError::new("the open panel closed without an answer"))?
                    .map_err(|error| HostError::new(format!("the open panel failed: {error}")))?;
                Ok(match paths.and_then(|paths| paths.into_iter().next()) {
                    Some(path) => path.to_string_lossy().into_owned().into(),
                    None => HostValue::Null,
                })
            })
        })
        .async_function("chooseFiles", |_| {
            let chosen = gpui_shell::with_current_app(|cx| {
                cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: true,
                    prompt: None,
                })
            })
            .ok_or_else(|| HostError::new("chooseFiles must be called from the application"))?;
            Ok(async move {
                let paths = chosen
                    .await
                    .map_err(|_| HostError::new("the open panel closed without an answer"))?
                    .map_err(|error| HostError::new(format!("the open panel failed: {error}")))?;
                match paths {
                    Some(paths) => register(paths).await,
                    None => Ok(HostValue::Array(Vec::new())),
                }
            })
        })
        .function("release", |arguments| {
            let handles = arguments
                .get(0)
                .and_then(HostValue::as_array)
                .ok_or_else(|| HostError::new("release(handles) needs an array of handles"))?
                .iter()
                .map(|handle| {
                    handle
                        .as_str()
                        .ok_or_else(|| HostError::new("a handle must be a string"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut files = chosen_files();
            for handle in handles {
                files.remove(handle);
            }
            Ok(HostValue::Null)
        })
        .async_function("open", |arguments| {
            let path = absolute_argument(arguments, 0, "open", "path")?;
            let root = absolute_argument(arguments, 1, "open", "root")?;
            let launch = launch_option(arguments)?;
            hand_over(
                "open",
                move || openable(&path, &root, launch),
                |cx, path| cx.open_with_system(path),
            )
        })
        .async_function("reveal", |arguments| {
            let path = absolute_argument(arguments, 0, "reveal", "path")?;
            let root = absolute_argument(arguments, 1, "reveal", "root")?;
            hand_over(
                "reveal",
                move || revealable(&path, &root),
                |cx, path| cx.reveal_path(path),
            )
        })
}

/// `open` and `reveal` (`function`): in a task on the application, runs
/// `check` on a background thread (it reads the disk, which may be slow) and
/// hands the path it answers to `hand`. The returned future answers when
/// that is done.
fn hand_over<F>(
    function: &'static str,
    check: F,
    hand: fn(&App, &Path),
) -> Result<impl Future<Output = HostResult> + Send, HostError>
where
    F: FnOnce() -> Result<PathBuf, HostError> + Send + 'static,
{
    let (answer, answered) = tokio::sync::oneshot::channel();
    gpui_shell::with_current_app(|cx| {
        cx.spawn(async move |cx| {
            let checked = cx.background_executor().spawn(async move { check() }).await;
            let result = checked.map(|path| {
                cx.update(|cx| hand(cx, &path));
                HostValue::Null
            });
            let _ = answer.send(result);
        })
        .detach();
    })
    .ok_or_else(|| HostError::new(format!("{function} must be called from the application")))?;
    Ok(async move {
        answered
            .await
            .map_err(|_| HostError::new(format!("{function} stopped before it finished")))?
    })
}

/// Argument `index` of `open` or `reveal` (`function`), called `name` in
/// messages: an absolute path without a `..` component.
fn absolute_argument(
    arguments: &HostArguments,
    index: usize,
    function: &str,
    name: &str,
) -> Result<PathBuf, HostError> {
    let path = arguments
        .get(index)
        .and_then(HostValue::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| HostError::new(format!("{function}(path, root) needs a {name}")))?;
    if !path.is_absolute() {
        return Err(HostError::new(format!(
            "{function}: {} is not an absolute path",
            path.display()
        )));
    }
    if path
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(HostError::new(format!(
            "{function}: {} has a .. component",
            path.display()
        )));
    }
    Ok(path)
}

/// `open`'s options: whether the user agreed to open a file that can run a
/// program (`launch`).
fn launch_option(arguments: &HostArguments) -> Result<bool, HostError> {
    let Some(options) = arguments.get(2).filter(|value| !value.is_null()) else {
        return Ok(false);
    };
    if options.as_object().is_none() {
        return Err(HostError::new("open options must be an object"));
    }
    match options.get("launch") {
        None => Ok(false),
        Some(value) if value.is_null() => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| HostError::new("`launch` must be true or false")),
    }
}

/// The start of `open`'s message for a file that can run a program, which
/// the script asks the user about.
const LAUNCHABLE: &str = "launchable:";

/// Extensions of files that run code when the system opens them. Each
/// platform has its own list: a name that runs a program on one is a
/// document on another, and asking about every `.js` file on macOS would
/// teach the user to dismiss the question.
///
/// On macOS: applications and installers, which run their own scripts (app, pkg,
/// mpkg); shell scripts, which Terminal runs (command, tool, sh, bash, zsh,
/// csh, ksh, fish); Terminal settings, which can run a command (terminal);
/// Automator workflows and actions (workflow, action); AppleScript, which
/// runs when saved run-only or opened by another handler than Script Editor
/// (scpt, scptd, applescript); Java archives (jar); scripts an installed
/// launcher runs (py, rb, pl); and location files, which open a URL or
/// another file, an application among them (webloc, inetloc, fileloc). The
/// bundles among them are folders, which `open` refuses anyway.
#[cfg(target_os = "macos")]
const LAUNCHABLE_EXTENSIONS: &[&str] = &[
    "app",
    "pkg",
    "mpkg",
    "command",
    "tool",
    "sh",
    "bash",
    "zsh",
    "csh",
    "ksh",
    "fish",
    "terminal",
    "workflow",
    "action",
    "scpt",
    "scptd",
    "applescript",
    "jar",
    "py",
    "rb",
    "pl",
    "webloc",
    "inetloc",
    "fileloc",
];

/// On Windows, where the system opens a file through the shell's `open`
/// verb: programs and installers (exe, com, scr, pif, cpl, msi, msp, msix,
/// msixbundle, appx, appxbundle, application, appref-ms); command and
/// PowerShell scripts (bat, cmd, ps1, psm1); Windows Script Host scripts
/// (vbs, vbe, js, jse, wsf, wsh); HTML applications (hta); management
/// consoles (msc); registry files, which change settings when merged (reg);
/// shortcuts and location files, which open another file or a URL, a
/// program among them (lnk, url, scf); Java archives (jar); and scripts an
/// installed launcher runs (py, pyw).
#[cfg(windows)]
const LAUNCHABLE_EXTENSIONS: &[&str] = &[
    "exe",
    "com",
    "scr",
    "pif",
    "cpl",
    "msi",
    "msp",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "application",
    "appref-ms",
    "bat",
    "cmd",
    "ps1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "hta",
    "msc",
    "reg",
    "lnk",
    "url",
    "scf",
    "jar",
    "py",
    "pyw",
];

/// On Linux and the other Unix systems, where `xdg-open` picks the handler:
/// desktop entries, which name a command (desktop); shell scripts (sh,
/// bash, zsh, csh, ksh, fish); self-running installers and images (run,
/// appimage); Java archives (jar); and scripts an installed launcher runs
/// (py, rb, pl). A file with an execute bit is asked about whatever its
/// name.
#[cfg(not(any(target_os = "macos", windows)))]
const LAUNCHABLE_EXTENSIONS: &[&str] = &[
    "desktop", "sh", "bash", "zsh", "csh", "ksh", "fish", "run", "appimage", "jar", "py", "rb",
    "pl",
];

/// What `open` hands to the system for `path` in the folder `root`: the
/// path with every link resolved, for a regular file inside `root`. A link
/// as the last component is refused rather than followed, and so is a file
/// that can run a program unless `launch` says the user agreed. The system
/// opens by path, so a change after these checks is not seen; they refuse
/// what is there when the user asks.
fn openable(path: &Path, root: &Path, launch: bool) -> Result<PathBuf, HostError> {
    let metadata = entry(path)?;
    let kind = metadata.file_type();
    if kind.is_symlink() {
        return Err(HostError::new(format!(
            "open: {} is a link",
            path.display()
        )));
    }
    if kind.is_dir() {
        return Err(HostError::new(format!(
            "open: {} is a folder",
            path.display()
        )));
    }
    if !kind.is_file() {
        return Err(HostError::new(format!(
            "open: {} is not a regular file",
            path.display()
        )));
    }
    let resolved = inside("open", path, canonical(path)?, root)?;
    if !launch && launchable(&resolved, &metadata) {
        return Err(HostError::new(format!(
            "{LAUNCHABLE} {} can run a program",
            path.display()
        )));
    }
    Ok(resolved)
}

/// What `reveal` hands to the system for `path` in the folder `root`: the
/// path with the links in its folders resolved, inside `root`. A link as the
/// last component is kept, so the file manager shows the link itself.
fn revealable(path: &Path, root: &Path) -> Result<PathBuf, HostError> {
    entry(path)?;
    let resolved = match (path.parent(), path.file_name()) {
        (Some(folder), Some(name)) => canonical(folder)?.join(name),
        _ => canonical(path)?,
    };
    inside("reveal", path, resolved, root)
}

/// `resolved` (`path` with its links followed) when it is inside `root`, also
/// with its links followed.
fn inside(
    function: &str,
    path: &Path,
    resolved: PathBuf,
    root: &Path,
) -> Result<PathBuf, HostError> {
    if resolved.starts_with(canonical(root)?) {
        Ok(resolved)
    } else {
        Err(HostError::new(format!(
            "{function}: {} is outside {}",
            path.display(),
            root.display()
        )))
    }
}

/// `path` as an entry, not followed: a link is itself, even one whose
/// target is gone.
fn entry(path: &Path) -> Result<std::fs::Metadata, HostError> {
    std::fs::symlink_metadata(path)
        .map_err(|error| HostError::new(format!("{}: {error}", path.display())))
}

/// `path` with every link in it followed.
fn canonical(path: &Path) -> Result<PathBuf, HostError> {
    std::fs::canonicalize(path)
        .map_err(|error| HostError::new(format!("{}: {error}", path.display())))
}

/// Whether the regular file at `path` (`metadata`) can run a program when it
/// is opened: it has an execute bit, or an extension in
/// LAUNCHABLE_EXTENSIONS, in any case.
fn launchable(path: &Path, metadata: &std::fs::Metadata) -> bool {
    executable(metadata)
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                LAUNCHABLE_EXTENSIONS
                    .iter()
                    .any(|listed| listed.eq_ignore_ascii_case(extension))
            })
}

#[cfg(unix)]
fn executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o111 != 0
}

/// Without execute bits, only the extension tells.
#[cfg(not(unix))]
fn executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

/// A file the user chose to attach: where it is, and its size and digest
/// when it was chosen.
#[derive(Clone)]
pub(crate) struct Chosen {
    pub path: PathBuf,
    pub bytes: u64,
    pub digest: String,
}

/// Registered files by handle, for the whole process: `maka-host` reads them.
static CHOSEN: Mutex<BTreeMap<String, Chosen>> = Mutex::new(BTreeMap::new());

fn chosen_files() -> MutexGuard<'static, BTreeMap<String, Chosen>> {
    CHOSEN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The file registered under `handle`, unless it was never registered or
/// has been released.
pub(crate) fn chosen(handle: &str) -> Option<Chosen> {
    chosen_files().get(handle).cloned()
}

/// What `chooseFiles` found out about one file.
enum Described {
    Ready(Chosen),
    TooLarge(u64),
    Unreadable(String),
}

/// Registers the files at `paths` as `chooseFiles` does, and resolves with
/// what it answers (see `ChosenFile` in the declarations). Only paths the
/// user chose in the panel may be handed here. The files are read on a
/// thread of their own: the digest of 50 MiB takes a while, and the executor
/// driving the call is not held meanwhile.
pub(crate) fn register(paths: Vec<PathBuf>) -> impl Future<Output = HostResult> + Send + 'static {
    let (answer, answered) = tokio::sync::oneshot::channel();
    let started = std::thread::Builder::new()
        .name("maka-files-digest".into())
        .spawn(move || {
            let described: Vec<_> = paths
                .iter()
                .map(|path| (name_of(path), describe(path)))
                .collect();
            let _ = answer.send(described);
        });
    async move {
        started
            .map_err(|error| HostError::new(format!("cannot read the chosen files: {error}")))?;
        let described = answered
            .await
            .map_err(|_| HostError::new("reading the chosen files stopped before it finished"))?;
        let mut files = Vec::with_capacity(described.len());
        for (name, described) in described {
            let file = HostObject::new().field("name", name);
            files.push(match described {
                Described::Ready(chosen) => {
                    let handle = Uuid::new_v4().to_string();
                    let file = file
                        .field("handle", handle.as_str())
                        .field("bytes", chosen.bytes)
                        .field("digest", chosen.digest.as_str());
                    chosen_files().insert(handle, chosen);
                    file.into()
                }
                Described::TooLarge(bytes) => {
                    file.field("bytes", bytes).field("tooLarge", true).into()
                }
                Described::Unreadable(error) => file.field("error", error).into(),
            });
        }
        Ok(HostValue::Array(files))
    }
}

/// The name a file is attached under: its last path component.
fn name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// The size and digest of one file as it is now, unless it is over the
/// Host's limit or cannot be read.
fn describe(path: &Path) -> Described {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => return Described::Unreadable(error.to_string()),
    };
    if !metadata.is_file() {
        return Described::Unreadable("not a regular file".into());
    }
    if metadata.len() > MAX_ATTACHMENT_BYTES {
        return Described::TooLarge(metadata.len());
    }
    match digest(path, metadata.len()) {
        Ok(digest) => Described::Ready(Chosen {
            path: path.to_owned(),
            bytes: metadata.len(),
            digest,
        }),
        Err(error) => Described::Unreadable(error),
    }
}

/// `sha256:` and the lowercase hex SHA-256 of the file, as the Host computes
/// an upload's digest (`content_digest`), read a chunk at a time. A file
/// that does not hold `bytes` bytes by the end changed while it was read.
fn digest(path: &Path, bytes: u64) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; DIGEST_CHUNK];
    let mut read = 0;
    while read <= bytes {
        let count = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.to_string()),
        };
        hasher.update(&buffer[..count]);
        read += count as u64;
    }
    if read != bytes {
        return Err("the file changed while it was read".into());
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

fn folder_options(arguments: &HostArguments) -> Result<Option<SharedString>, HostError> {
    let Some(options) = arguments.get(0).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    if options.as_object().is_none() {
        return Err(HostError::new("chooseFolder options must be an object"));
    }
    prompt(options)
}

fn options(arguments: &HostArguments) -> Result<(Option<SharedString>, u64), HostError> {
    let Some(options) = arguments.get(0).filter(|value| !value.is_null()) else {
        return Ok((None, DEFAULT_MAX_BYTES));
    };
    if options.as_object().is_none() {
        return Err(HostError::new("openText options must be an object"));
    }
    let prompt = prompt(options)?;
    let limit = match options.get("maxBytes") {
        None => DEFAULT_MAX_BYTES,
        Some(value) if value.is_null() => DEFAULT_MAX_BYTES,
        Some(value) => match value.as_number() {
            Some(n) if n.fract() == 0.0 && n >= 1.0 && n <= MAX_BYTES as f64 => n as u64,
            _ => {
                return Err(HostError::new(format!(
                    "`maxBytes` must be a whole number from 1 to {MAX_BYTES}"
                )));
            }
        },
    };
    Ok((prompt, limit))
}

/// The panel's button label, when the options give one.
fn prompt(options: &HostValue) -> Result<Option<SharedString>, HostError> {
    Ok(match options.get("prompt") {
        None => None,
        Some(value) if value.is_null() => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or_else(|| HostError::new("`prompt` must be a string"))?
                .to_owned()
                .into(),
        ),
    })
}

/// Reads at most `limit` bytes; a longer file is refused rather than cut.
fn read_text(path: &Path, limit: u64) -> Result<String, HostError> {
    let name = path.display();
    let file =
        std::fs::File::open(path).map_err(|error| HostError::new(format!("{name}: {error}")))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| HostError::new(format!("{name}: {error}")))?;
    if bytes.len() as u64 > limit {
        return Err(HostError::new(format!(
            "{name} is larger than {limit} bytes"
        )));
    }
    String::from_utf8(bytes).map_err(|_| HostError::new(format!("{name} is not UTF-8 text")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: Vec<HostValue>) -> HostArguments {
        HostArguments::new(values)
    }

    #[test]
    fn options_default_and_bounds() {
        assert_eq!(
            options(&arguments(vec![])).unwrap(),
            (None, DEFAULT_MAX_BYTES)
        );
        let given = HostObject::new()
            .field("prompt", "Import")
            .field("maxBytes", 4096.0)
            .into();
        let (prompt, limit) = options(&arguments(vec![given])).unwrap();
        assert_eq!(prompt.as_deref(), Some("Import"));
        assert_eq!(limit, 4096);
        for bad in [0.0, 1.5, (MAX_BYTES + 1) as f64] {
            let value = HostObject::new().field("maxBytes", bad).into();
            assert!(options(&arguments(vec![value])).is_err(), "{bad}");
        }
        assert!(options(&arguments(vec![HostValue::from("x")])).is_err());
    }

    #[test]
    fn folder_options_take_a_prompt_only_from_an_object() {
        assert_eq!(folder_options(&arguments(vec![])).unwrap(), None);
        assert_eq!(
            folder_options(&arguments(vec![HostValue::Null])).unwrap(),
            None
        );
        let given = HostObject::new().field("prompt", "Choose").into();
        assert_eq!(
            folder_options(&arguments(vec![given])).unwrap().as_deref(),
            Some("Choose")
        );
        let bad = HostObject::new().field("prompt", 1.0).into();
        assert!(folder_options(&arguments(vec![bad])).is_err());
        assert!(folder_options(&arguments(vec![HostValue::from("x")])).is_err());
    }

    #[test]
    fn read_text_refuses_large_and_binary_files() {
        let directory = std::env::temp_dir().join(format!("maka-files-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let text = directory.join("a.json");
        std::fs::write(&text, "{\"é\":1}").unwrap();
        // Eight bytes: `é` takes two.
        assert_eq!(read_text(&text, 8).unwrap(), "{\"é\":1}");
        assert!(
            read_text(&text, 7)
                .unwrap_err()
                .message()
                .contains("larger than 7")
        );
        let binary = directory.join("b.bin");
        std::fs::write(&binary, [0xff, 0xfe]).unwrap();
        assert!(
            read_text(&binary, 16)
                .unwrap_err()
                .message()
                .contains("not UTF-8")
        );
        assert!(read_text(&directory.join("missing"), 16).is_err());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn declarations_name_exactly_the_registered_functions() {
        let module = module();
        module.validate().unwrap();
        assert_eq!(
            module.function_names(),
            [
                "chooseFiles",
                "chooseFolder",
                "open",
                "openText",
                "release",
                "reveal"
            ]
        );
        assert!(!module.is_async("release"));
        assert!(module.is_async("open"));
        assert!(module.is_async("reveal"));
    }

    #[test]
    fn open_and_reveal_check_their_arguments_before_the_application() {
        let directory = std::env::temp_dir().join(format!("maka-files-open-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("a.txt");
        std::fs::write(&file, "a").unwrap();
        let root = directory.to_string_lossy().into_owned();
        let begin = |function: &str, values: Vec<HostValue>| match module()
            .begin(function, &arguments(values))
        {
            Ok(_) => panic!("{function} began"),
            Err(error) => error.message().to_owned(),
        };
        let paths = |path: &str, root: &str| vec![HostValue::from(path), HostValue::from(root)];
        for function in ["open", "reveal"] {
            assert!(begin(function, paths("a.txt", &root)).contains("not an absolute path"));
            let up = format!("{}/../a.txt", directory.join("sub").display());
            assert!(begin(function, paths(&up, &root)).contains(".. component"));
            assert!(
                begin(function, paths(&file.to_string_lossy(), "repo"))
                    .contains("not an absolute path")
            );
            assert!(
                begin(function, vec![HostValue::from(&*file.to_string_lossy())])
                    .contains("needs a root")
            );
            assert!(begin(function, vec![]).contains("needs a path"));
            // Arguments that pass are handed to the application, which a test
            // does not have; what is on disk is checked in its task.
            assert!(
                begin(function, paths(&file.to_string_lossy(), &root))
                    .contains("must be called from the application")
            );
        }
        let mut with = paths(&file.to_string_lossy(), &root);
        with.push(HostObject::new().field("launch", "yes").into());
        assert!(begin("open", with).contains("`launch` must be true or false"));
        let mut with = paths(&file.to_string_lossy(), &root);
        with.push(HostObject::new().field("launch", true).into());
        assert!(begin("open", with).contains("must be called from the application"));
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn open_takes_a_regular_file_inside_the_folder_only() {
        let directory = std::env::temp_dir().join(format!("maka-files-inside-{}", Uuid::new_v4()));
        let root = directory.join("repo");
        let outside = directory.join("outside");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let file = root.join("src/a.txt");
        std::fs::write(&file, "a").unwrap();
        std::fs::write(outside.join("b.txt"), "b").unwrap();
        let refusal = |result: Result<PathBuf, HostError>| result.unwrap_err().message().to_owned();

        let opened = openable(&file, &root, false).unwrap();
        assert_eq!(opened, std::fs::canonicalize(&file).unwrap());
        assert_eq!(revealable(&file, &root).unwrap(), opened);
        assert!(refusal(openable(&root.join("src"), &root, false)).contains("is a folder"));
        assert!(refusal(openable(&root.join("missing"), &root, false)).contains("missing"));
        assert!(refusal(revealable(&root.join("missing"), &root)).contains("missing"));
        assert!(refusal(openable(&outside.join("b.txt"), &root, false)).contains("is outside"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            // A link as the last component: not opened, though its target is
            // in the folder; shown as itself, even when its target is gone.
            let link = root.join("src/link.txt");
            symlink(&file, &link).unwrap();
            assert!(refusal(openable(&link, &root, false)).contains("is a link"));
            assert!(refusal(openable(&link, &root, true)).contains("is a link"));
            let gone = root.join("src/gone.txt");
            symlink(root.join("src/missing"), &gone).unwrap();
            assert_eq!(
                revealable(&gone, &root).unwrap(),
                std::fs::canonicalize(root.join("src"))
                    .unwrap()
                    .join("gone.txt")
            );
            // A folder in the path that links out of the folder: refused by
            // both, though the path names a file under the folder.
            symlink(&outside, root.join("escape")).unwrap();
            let escaped = root.join("escape/b.txt");
            assert!(refusal(openable(&escaped, &root, false)).contains("is outside"));
            assert!(refusal(revealable(&escaped, &root)).contains("is outside"));
            // A folder link that stays inside is followed.
            symlink(root.join("src"), root.join("source")).unwrap();
            assert_eq!(
                openable(&root.join("source/a.txt"), &root, false).unwrap(),
                opened
            );
        }
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn open_asks_before_a_file_that_can_run_a_program() {
        let directory = std::env::temp_dir().join(format!("maka-files-launch-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let refusal = |result: Result<PathBuf, HostError>| result.unwrap_err().message().to_owned();
        #[cfg(target_os = "macos")]
        let names = ["run.command", "Setup.PKG", "tool.sh", "page.webloc"];
        #[cfg(windows)]
        let names = ["Setup.EXE", "run.bat", "tool.ps1", "link.lnk"];
        #[cfg(not(any(target_os = "macos", windows)))]
        let names = ["app.desktop", "tool.sh", "Tool.JAR", "page.py"];
        for name in names {
            let file = directory.join(name);
            std::fs::write(&file, "").unwrap();
            assert!(
                refusal(openable(&file, &directory, false)).starts_with(LAUNCHABLE),
                "{name}"
            );
            assert!(openable(&file, &directory, true).is_ok(), "{name}");
        }
        let plain = directory.join("notes.txt");
        std::fs::write(&plain, "").unwrap();
        assert!(openable(&plain, &directory, false).is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o744)).unwrap();
            assert!(refusal(openable(&plain, &directory, false)).starts_with(LAUNCHABLE));
            assert!(openable(&plain, &directory, true).is_ok());
        }
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn chosen_files_are_registered_unless_too_large_or_unreadable() {
        let directory = std::env::temp_dir().join(format!("maka-files-chosen-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let small = directory.join("abc.txt");
        std::fs::write(&small, "abc").unwrap();
        // Sparse: one byte over the limit without writing 50 MiB.
        let large = directory.join("large.bin");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(MAX_ATTACHMENT_BYTES + 1)
            .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let answered = runtime
            .block_on(register(vec![
                small.clone(),
                large,
                directory.clone(),
                directory.join("missing"),
            ]))
            .unwrap();
        let files = answered.as_array().unwrap();
        assert_eq!(files.len(), 4);

        assert_eq!(files[0].get("name"), Some(&HostValue::from("abc.txt")));
        assert_eq!(files[0].get("bytes"), Some(&HostValue::Number(3.0)));
        let handle = files[0].get("handle").and_then(HostValue::as_str).unwrap();
        let registered = chosen(handle).unwrap();
        assert_eq!(registered.path, small);
        assert_eq!(registered.bytes, 3);
        assert_eq!(
            files[0].get("digest").and_then(HostValue::as_str),
            Some(registered.digest.as_str())
        );

        assert_eq!(files[1].get("name"), Some(&HostValue::from("large.bin")));
        assert_eq!(files[1].get("tooLarge"), Some(&HostValue::Bool(true)));
        assert_eq!(
            files[1].get("bytes"),
            Some(&HostValue::Number((MAX_ATTACHMENT_BYTES + 1) as f64))
        );
        assert!(files[1].get("handle").is_none());
        assert_eq!(
            files[2].get("error"),
            Some(&HostValue::from("not a regular file"))
        );
        assert!(files[3].get("error").is_some() && files[3].get("handle").is_none());

        // Released, a handle names nothing; an unknown one is ignored.
        let release = |handles: Vec<HostValue>| {
            module().call("release", &HostArguments::new([HostValue::Array(handles)]))
        };
        release(vec![HostValue::from(handle), HostValue::from("unknown")]).unwrap();
        assert!(chosen(handle).is_none());
        assert!(release(vec![HostValue::Number(1.0)]).is_err());
        assert!(module().call("release", &arguments(vec![])).is_err());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_file_that_changes_while_it_is_read_is_refused() {
        let directory = std::env::temp_dir().join(format!("maka-files-digest-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("a.txt");
        std::fs::write(&path, "abcd").unwrap();
        assert_eq!(
            digest(&path, 3).unwrap_err(),
            "the file changed while it was read"
        );
        assert_eq!(
            digest(&path, 5).unwrap_err(),
            "the file changed while it was read"
        );
        assert!(digest(&path, 4).unwrap().starts_with("sha256:"));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
