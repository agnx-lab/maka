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

use super::Error;
use maka_capabilities::filesystem::entries;
use maka_capabilities::filesystem::entries::Kind;
use maka_capabilities::filesystem::entries::Operation;
use maka_capabilities::filesystem::entries::Output;
use std::sync::Arc;

/// Publication operates through the embedding's pinned directory and owned workers.
#[derive(Clone)]
pub(super) struct Directory {
    root: Arc<Root>,
    prefix: String,
}
struct Root {
    directory: cap_std::fs::Dir,
    cancellation: tokio_util::sync::CancellationToken,
    resources: Option<Arc<maka_capabilities::call::Resources>>,
    // Every queued worker retains publication exclusion even after observer loss.
    _lock: Arc<super::io::PublicationLock>,
}
impl Directory {
    pub fn private(
        directory: cap_std::fs::Dir,
        cancellation: tokio_util::sync::CancellationToken,
        resources: Option<Arc<maka_capabilities::call::Resources>>,
        lock: Arc<super::io::PublicationLock>,
    ) -> Self {
        Self {
            root: Arc::new(Root {
                directory,
                cancellation,
                resources,
                _lock: lock,
            }),
            prefix: String::new(),
        }
    }
    fn path(&self, name: &str) -> String {
        match (self.prefix.is_empty(), name.is_empty()) {
            (true, _) => name.into(),
            (_, true) => self.prefix.clone(),
            _ => format!("{}/{name}", self.prefix),
        }
    }
    pub fn at(&self, name: &str) -> Self {
        Self {
            root: self.root.clone(),
            prefix: self.path(name),
        }
    }
    async fn invoke(&self, operation: Operation) -> Result<Output, Error> {
        if self.root.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut ticket = self
            .root
            .resources
            .as_ref()
            .map(|resources| resources.reserve().map_err(|_| Error::Cancelled))
            .transpose()?;
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(ticket) = &mut ticket {
                ticket.start();
            }
            let result = entries::execute(&root.directory, operation, &root.cancellation, None);
            drop(root);
            if let Some(ticket) = ticket {
                ticket.complete(Ok(()));
            }
            result
        })
        .await
        .map_err(|e| Error::OutcomeUnknown(e.to_string()))?
        .map_err(Into::into)
    }
    pub async fn open(&self, name: &str) -> Result<Self, Error> {
        let path = self.path(name);
        let output = self.invoke(Operation::Stat { path: path.clone() }).await?;
        if !matches!(
            output,
            Output::Stat(entries::Metadata {
                kind: Kind::Directory,
                ..
            })
        ) {
            return Err(Error::Invalid("Expected a Skill directory".into()));
        }
        Ok(Self {
            root: self.root.clone(),
            prefix: path,
        })
    }
    pub async fn child(&self, name: &str) -> Result<Self, Error> {
        match self.create_dir(name).await {
            Ok(()) => {}
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        self.open(name).await
    }
    pub async fn create_dir(&self, name: &str) -> Result<(), Error> {
        self.invoke(Operation::CreateDirectory {
            path: self.path(name),
        })
        .await?;
        Ok(())
    }
    pub async fn entries(&self) -> Result<Vec<entries::Entry>, Error> {
        let mut after = None;
        let mut entries = Vec::new();
        loop {
            let Output::List(page) = self
                .invoke(Operation::List(entries::ListFiles {
                    path: self.prefix.clone(),
                    after: after.clone(),
                    limit: 1024,
                }))
                .await?
            else {
                return Err(Error::Invalid("Invalid directory output".into()));
            };
            entries.extend(page.entries);
            match page.next_after {
                None => return Ok(entries),
                Some(next) if after.as_ref().is_none_or(|previous| &next > previous) => {
                    after = Some(next)
                }
                Some(_) => return Err(Error::Invalid("Directory page did not advance".into())),
            }
        }
    }
    pub async fn read(&self, name: &str) -> Result<(Vec<u8>, u32), Error> {
        let path = self.path(name);
        let Output::Stat(metadata) = self.invoke(Operation::Stat { path: path.clone() }).await?
        else {
            return Err(Error::Invalid("Invalid metadata output".into()));
        };
        if !matches!(metadata.kind, Kind::File) {
            return Err(Error::Invalid(
                "Skill resource is not a regular file".into(),
            ));
        }
        // Read the complete initial prefix through one opened file object.
        let Output::Read(page) = self
            .invoke(Operation::Read(entries::ReadFile {
                path,
                offset: 0,
                limit: usize::MAX,
            }))
            .await?
        else {
            return Err(Error::Invalid("Invalid file output".into()));
        };
        if page.next_offset.is_some() {
            return Err(Error::Invalid("Incomplete Skill resource capture".into()));
        }
        Ok((page.bytes, metadata.mode))
    }

    pub async fn write_new(&self, name: &str, bytes: &[u8], mode: u32) -> Result<(), Error> {
        let path = self.path(name);
        // Empty files still require a create-new operation.
        for (offset, chunk) in bytes
            .chunks(1024 * 1024)
            .enumerate()
            .chain(bytes.is_empty().then_some((0, &[][..])))
        {
            self.invoke(Operation::Write(entries::WriteFile {
                path: path.clone(),
                offset: (offset * 1024 * 1024) as u64,
                bytes: chunk.to_vec(),
                truncate: false,
                create_new: offset == 0,
                mode: (offset * 1024 * 1024 + chunk.len() == bytes.len()).then_some(mode),
            }))
            .await?;
        }
        Ok(())
    }
    pub async fn remove(&self, name: &str) -> Result<(), Error> {
        self.invoke(Operation::Remove {
            path: self.path(name),
        })
        .await?;
        Ok(())
    }
    pub async fn remove_tree(&self, name: &str) -> Result<(), Error> {
        let mut stack = vec![(self.clone(), name.to_owned(), false)];
        while let Some((parent, name, visited)) = stack.pop() {
            if visited {
                parent.remove(&name).await?;
                continue;
            }
            let directory = parent.open(&name).await?;
            stack.push((parent, name, true));
            for entry in directory.entries().await? {
                match entry.kind {
                    Kind::Directory => stack.push((directory.clone(), entry.name, false)),
                    _ => directory.remove(&entry.name).await?,
                }
            }
        }
        Ok(())
    }
    pub async fn sync(&self) -> Result<(), Error> {
        self.invoke(Operation::Sync {
            path: self.prefix.clone(),
        })
        .await?;
        Ok(())
    }
    pub async fn rename(&self, from: &str, target: &Self, to: &str) -> Result<(), Error> {
        if !Arc::ptr_eq(&self.root, &target.root) {
            return Err(Error::Invalid(
                "Publication crosses filesystem grants".into(),
            ));
        }
        self.invoke(Operation::Rename {
            from: self.path(from),
            to: target.path(to),
        })
        .await?;
        Ok(())
    }
}
impl From<entries::Error> for Error {
    fn from(error: entries::Error) -> Self {
        use entries::Error as File;
        match error {
            File::Invalid(message) => Self::Invalid(message),
            File::NotFound => Self::Io(std::io::ErrorKind::NotFound.into()),
            File::AlreadyExists => Self::Io(std::io::ErrorKind::AlreadyExists.into()),
            File::Retired | File::Cancelled => Self::Cancelled,
            File::Io(message) => Self::Io(std::io::Error::other(message)),
            File::OutcomeUnknown(message) => Self::OutcomeUnknown(message),
        }
    }
}
impl From<maka_agent_facts::tools::ToolError> for Error {
    fn from(error: maka_agent_facts::tools::ToolError) -> Self {
        use maka_agent_facts::tools::ToolError;
        match error {
            ToolError::Io { kind, message } => Self::Io(std::io::Error::new(kind, message)),
            ToolError::Failed(message) => Self::Invalid(message),
            error => Self::OutcomeUnknown(error.to_string()),
        }
    }
}
