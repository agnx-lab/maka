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

use super::{
    BundleError, BundleSummary, Inventory,
    format::{Blob, CHUNK, MAGIC, Record},
};
use crate::StoreError;
use sha2::{Digest, Sha256};
use std::{io, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

/// Decode a complete transfer for preview. Import must separately validate its
/// original canonical proofs and destination authority before publication.
pub async fn inspect<R: AsyncRead + Unpin>(input: R) -> Result<BundleSummary, BundleError> {
    let mut reader = Reader::new(input).await?;
    loop {
        match reader.next().await? {
            Record::Blob(_) => reader.copy_blob(&mut tokio::io::sink()).await?,
            Record::End { .. } => return Ok(reader.summary()?),
            _ => {}
        }
    }
}

fn validate_inventory(inventory: &Inventory) -> Result<(), StoreError> {
    if inventory.sessions.is_empty() {
        return Err(StoreError::PrefixTooLarge);
    }
    let mut digest = Sha256::new();
    let mut previous = None::<&str>;
    let mut root = false;
    for session in &inventory.sessions {
        crate::sessions::validate_id(&session.id)?;
        if session.revision == 0
            || session.revision > i64::MAX as u64
            || previous.is_some_and(|id| id >= session.id.as_str())
        {
            return Err(invalid(
                "bundle inventory is not unique, sorted and revisioned",
            ));
        }
        if previous.is_some() {
            digest.update(b"\n");
        }
        digest.update(session.id.as_bytes());
        previous = Some(&session.id);
        root |= session.id == inventory.root_session_id;
    }
    if !root || hex::encode(digest.finalize()) != inventory.subtree_digest {
        return Err(invalid("bundle inventory digest or root differs"));
    }
    Ok(())
}

pub(super) struct Reader<R> {
    input: R,
    digest: Sha256,
    bytes: u64,
    frames: u64,
    pending: Option<BlobRead>,
    finished: Option<String>,
    inventory: Option<Inventory>,
    fence: u64,
    previous_event: u64,
}

struct BlobRead {
    descriptor: Blob,
    remaining: u64,
    digest: Sha256,
}

impl<R: AsyncRead + Unpin> Reader<R> {
    pub async fn new(input: R) -> Result<Self, StoreError> {
        let mut reader = Self {
            input,
            digest: Sha256::new(),
            bytes: 0,
            frames: 0,
            pending: None,
            finished: None,
            inventory: None,
            fence: 0,
            previous_event: 0,
        };
        let mut magic = vec![0; MAGIC.len()];
        reader.read(&mut magic).await?;
        if magic != MAGIC {
            return Err(invalid("unsupported Session bundle"));
        }
        Ok(reader)
    }

    pub async fn next(&mut self) -> Result<Record, StoreError> {
        if self.finished.is_some() || self.pending.is_some() {
            return Err(invalid("bundle frame boundary violated"));
        }
        let before = format!("sha256:{}", hex::encode(self.digest.clone().finalize()));
        let mut length = [0; 4];
        self.read(&mut length).await?;
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 {
            return Err(StoreError::PrefixTooLarge);
        }
        // A declared length is framing, not evidence that any payload exists.
        // Grow only after consuming real bytes, never allocate the untrusted u32.
        let mut bytes = Vec::new();
        // Keep transport scratch off the enclosing async future's stack frame.
        let mut chunk = Vec::new();
        chunk.try_reserve_exact(CHUNK).map_err(io::Error::other)?;
        chunk.resize(CHUNK, 0);
        while bytes.len() < length {
            let count = (length - bytes.len()).min(CHUNK);
            self.read(&mut chunk[..count]).await?;
            bytes.try_reserve(count).map_err(io::Error::other)?;
            bytes.extend_from_slice(&chunk[..count]);
        }
        let record: Record = serde_json::from_slice(&bytes)?;
        if self.frames == 0 && !matches!(record, Record::Header { .. }) {
            return Err(invalid("bundle must start with its inventory"));
        }
        match &record {
            Record::Header {
                inventory,
                source_high_water,
            } => {
                if self.frames != 0 {
                    return Err(invalid("duplicate bundle header"));
                }
                validate_inventory(inventory)?;
                if *source_high_water > i64::MAX as u64 {
                    return Err(invalid("bundle source fence is out of range"));
                }
                self.inventory = Some(inventory.clone());
                self.fence = *source_high_water;
            }
            Record::Event { sequence, .. } => {
                if *sequence <= self.previous_event || *sequence > self.fence {
                    return Err(invalid(
                        "bundle events are not ordered within their source fence",
                    ));
                }
                self.previous_event = *sequence;
            }
            Record::Blob(blob) => {
                self.pending = Some(BlobRead {
                    descriptor: blob.clone(),
                    remaining: blob.bytes(),
                    digest: Sha256::new(),
                });
            }
            Record::End { digest, frames } => {
                if *frames != self.frames || *digest != before {
                    return Err(invalid("bundle footer does not match its bytes"));
                }
                if tokio::time::timeout(Duration::from_secs(30), self.input.read(&mut [0; 1]))
                    .await
                    .map_err(stalled)??
                    != 0
                {
                    return Err(invalid("trailing bytes after bundle footer"));
                }
                self.finished = Some(digest.clone());
            }
            _ => {}
        }
        self.frames = self
            .frames
            .checked_add(1)
            .ok_or(StoreError::PrefixTooLarge)?;
        Ok(record)
    }

    pub async fn copy_blob<W: AsyncWrite + Unpin>(
        &mut self,
        output: &mut W,
    ) -> Result<(), StoreError> {
        while let Some(bytes) = self.blob_chunk().await? {
            super::format::write_progress(output, &bytes).await?;
        }
        Ok(())
    }

    pub async fn blob_chunk(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        let remaining = self
            .pending
            .as_ref()
            .ok_or_else(|| invalid("no pending bundle payload"))?
            .remaining;
        if remaining == 0 {
            let pending = self.pending.take().expect("pending blob");
            if format!("sha256:{}", hex::encode(pending.digest.finalize()))
                != pending.descriptor.digest()
            {
                return Err(invalid("bundle payload digest mismatch"));
            }
            return Ok(None);
        }
        let mut bytes = vec![0; remaining.min(CHUNK as u64) as usize];
        self.read(&mut bytes).await?;
        let pending = self.pending.as_mut().expect("pending blob");
        pending.digest.update(&bytes);
        pending.remaining -= bytes.len() as u64;
        Ok(Some(bytes))
    }

    pub fn summary(self) -> Result<BundleSummary, StoreError> {
        Ok(BundleSummary {
            inventory: self
                .inventory
                .ok_or_else(|| invalid("bundle header missing"))?,
            bytes: self.bytes,
            digest: self
                .finished
                .ok_or_else(|| invalid("bundle footer missing"))?,
        })
    }

    async fn read(&mut self, bytes: &mut [u8]) -> Result<(), StoreError> {
        let mut offset = 0;
        while offset < bytes.len() {
            let count = tokio::time::timeout(
                Duration::from_secs(30),
                self.input.read(&mut bytes[offset..]),
            )
            .await
            .map_err(stalled)??;
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "bundle ended within a frame",
                )
                .into());
            }
            offset += count;
        }
        self.digest.update(&*bytes);
        self.bytes = self
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or(StoreError::PrefixTooLarge)?;
        Ok(())
    }
}

fn stalled(_: tokio::time::error::Elapsed) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "Session bundle transfer stalled")
}
fn invalid(message: &str) -> StoreError {
    StoreError::InvalidTransition(message.into())
}
