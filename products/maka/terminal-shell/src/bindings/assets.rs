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

use super::{Owner, execute};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use maka_agent_api::contract::ToolImageRead;
use maka_agent_facts::artifact::content_digest;
use nsh_engine::Assets;
use serde_json::json;
use std::sync::Arc;

pub(super) struct HostAssets(pub(super) Arc<Owner>);

impl Assets for HostAssets {
    fn read(&self, uri: &str) -> Result<Vec<u8>, String> {
        let Some(encoded) = uri.strip_prefix("asset:/tool-image/") else {
            return crate::ApplicationAssets.read(uri);
        };
        // This URI selects a canonical row, not a file or bearer capability.
        // The original Host authorizes every page with its original grant.
        if uri.len() > 2048 {
            return Err("Tool image source is too long".into());
        }
        let source: ToolImageRead = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(encoded)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        source.position.validate().map_err(str::to_owned)?;
        if source.session_id.is_empty()
            || source.session_id.len() > 128
            || source.offset != 0
            || source.through > 9_007_199_254_740_991
            || source.position.source_sequence > source.through
        {
            return Err("Invalid tool image source".into());
        }
        let owner = self.0.clone();
        // Image assets are read on NSH's image workers; bundled/font reads above
        // stay synchronous. Track the whole read with the launcher's other IO.
        let (bytes, digest) = self.0.runtime.block_on(execute(owner.clone(), async move {
            read_image(&owner, source).await
        }))?;
        // Hash a large image on its reader, without occupying a Host IO worker.
        if content_digest(&bytes) != digest {
            return Err("The original tool image digest does not match".into());
        }
        Ok(bytes)
    }
}

async fn read_image(owner: &Owner, source: ToolImageRead) -> Result<(Vec<u8>, String), String> {
    // Match the existing NSH decoder's encoded-byte ceiling, including valid
    // 50 MiB Host images. Image bytes never enter the JavaScript heap.
    const MAX_BYTES: u64 = 64 << 20;
    let mut bytes = Vec::new();
    let mut metadata: Option<(u64, String, String)> = None;
    loop {
        let offset = bytes.len() as u64;
        let page = owner
            .client
            .query(
                "agent",
                "toolImage",
                json!({
                    "sessionId":source.session_id, "through":source.through,
                    "position":source.position, "offset":offset,
                }),
            )
            .await?;
        let total = page["totalBytes"]
            .as_u64()
            .filter(|total| *total > 0 && *total <= MAX_BYTES)
            .ok_or("Tool image exceeds the terminal decoder's 64 MiB limit")?;
        let mime = page["mimeType"]
            .as_str()
            .ok_or("Missing tool image MIME type")?;
        let digest = page["digest"].as_str().ok_or("Missing tool image digest")?;
        let data = page["data"].as_str().ok_or("Missing tool image bytes")?;
        if page["kind"] != "ready"
            || page["through"].as_u64() != Some(source.through)
            || page["position"]["sourceSequence"].as_u64() != Some(source.position.source_sequence)
            || page["position"]["itemIndex"].as_u64() != Some(source.position.item_index)
            || page["offset"].as_u64() != Some(offset)
            || offset >= total
            || data.len() > 43692
            || !matches!(
                mime,
                "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/avif"
            )
            || !digest.strip_prefix("sha256:").is_some_and(|hash| {
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            || metadata.as_ref().is_some_and(|(length, kind, hash)| {
                *length != total || kind != mime || hash != digest
            })
        {
            return Err("The original tool image could not be verified".into());
        }
        if mime == "image/avif" {
            return Err("AVIF previews are not supported by this terminal renderer".into());
        }
        if metadata.is_none() {
            bytes
                .try_reserve_exact(total as usize)
                .map_err(|error| error.to_string())?;
            metadata = Some((total, mime.to_owned(), digest.to_owned()));
        }
        // Each page has its own padding: concatenating Base64 would corrupt
        // every image that crosses the Host's 32 KiB page boundary.
        let chunk = STANDARD.decode(data).map_err(|error| error.to_string())?;
        if chunk.len() as u64 != (total - offset).min(32 * 1024) {
            return Err("The original tool image page has an invalid length".into());
        }
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 == total {
            return Ok((bytes, digest.to_owned()));
        }
    }
}
