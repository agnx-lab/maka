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

//! Last successful public model catalogs. Payload validation and refresh lifetime
//! belong to the caller; this store owns their durable, atomic publication.

use crate::{ConfigError, ConfigurationStore, Result, TransactionMode, catalog};
use serde_json::Value;
use std::collections::BTreeMap;

type CachedCatalogRow = (String, String, Option<String>, Option<String>, i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PublicCatalogSource {
    ModelsDev,
    OpenRouter,
}

impl PublicCatalogSource {
    fn key(self) -> &'static str {
        match self {
            Self::ModelsDev => "models_dev",
            Self::OpenRouter => "openrouter",
        }
    }

    fn from_key(key: &str) -> Result<Self> {
        match key {
            "models_dev" => Ok(Self::ModelsDev),
            "openrouter" => Ok(Self::OpenRouter),
            _ => Err(ConfigError::Invalid("invalid public catalog source".into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublicCatalogRecord {
    pub body: Value,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Successful fetch or conditional revalidation time, in Unix milliseconds.
    pub checked_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublicCatalogSnapshot {
    pub catalog_revision: u64,
    pub sources: BTreeMap<PublicCatalogSource, PublicCatalogRecord>,
}

impl ConfigurationStore {
    pub async fn public_catalog(&self) -> Result<PublicCatalogSnapshot> {
        self.transaction(TransactionMode::Deferred, |tx| {
            Box::pin(async move {
                let revision: i64 = sqlx::query_scalar(
                    "SELECT revision FROM connection_catalog WHERE singleton = 1",
                )
                .fetch_one(&mut *tx)
                .await?;
                let rows: Vec<CachedCatalogRow> = sqlx::query_as(
                    "SELECT source, body, etag, last_modified, checked_at FROM public_catalog",
                )
                .fetch_all(&mut *tx)
                .await?;
                let mut sources = BTreeMap::new();
                for (source, body, etag, last_modified, checked_at) in rows {
                    sources.insert(
                        PublicCatalogSource::from_key(&source)?,
                        PublicCatalogRecord {
                            body: serde_json::from_str(&body)?,
                            etag,
                            last_modified,
                            checked_at,
                        },
                    );
                }
                Ok(PublicCatalogSnapshot {
                    catalog_revision: crate::database::unsigned(revision)?,
                    sources,
                })
            })
        })
        .await
    }

    /// Publish an already validated payload. A 304 response can retain the
    /// original body and update its HTTP metadata without invalidating cursors.
    pub async fn put_public_catalog(
        &self,
        source: PublicCatalogSource,
        record: PublicCatalogRecord,
    ) -> Result<u64> {
        self.transaction(TransactionMode::Immediate, move |tx| {
            Box::pin(async move {
                let previous: Option<String> =
                    sqlx::query_scalar("SELECT body FROM public_catalog WHERE source = ?")
                        .bind(source.key())
                        .fetch_optional(&mut *tx)
                        .await?;
                let previous: Option<Value> =
                    previous.as_deref().map(serde_json::from_str).transpose()?;
                let changed = previous.as_ref() != Some(&record.body);
                sqlx::query(
                    "INSERT INTO public_catalog(source, body, etag, last_modified, checked_at)
                     VALUES(?, ?, ?, ?, ?)
                     ON CONFLICT(source) DO UPDATE SET body = excluded.body,
                     etag = excluded.etag, last_modified = excluded.last_modified,
                     checked_at = excluded.checked_at",
                )
                .bind(source.key())
                .bind(serde_json::to_string(&record.body)?)
                .bind(record.etag)
                .bind(record.last_modified)
                .bind(record.checked_at)
                .execute(&mut *tx)
                .await?;
                let current = catalog::read(tx).await?;
                if changed {
                    catalog::advance(tx, current.revision, current.default_target.as_ref()).await
                } else {
                    Ok(current.revision)
                }
            })
        })
        .await
    }
}
