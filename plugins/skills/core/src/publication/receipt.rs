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

//! Immutable request identities and original outcomes, retained after journal collection.
use super::{Error, directory::Directory};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub id: uuid::Uuid,
    pub fingerprint: String,
    /// Reader authority and view identity, independent of editable form fields.
    pub binding: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Destination {
    Skill { reference: String },
    Source { id: String },
    Installed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Applied { destination: Destination },
    Conflict,
    Rejected { reason: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub operation: Operation,
    pub outcome: Outcome,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Publication {
    pub operation: Operation,
    pub destination: Destination,
}
impl Operation {
    pub fn validate(&self) -> Result<(), Error> {
        if self.id.is_nil() || !digest(&self.fingerprint) || !digest(&self.binding) {
            return Err(Error::Invalid("Invalid Skill operation identity".into()));
        }
        Ok(())
    }
}
pub(crate) fn digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub(super) async fn reserve(
    requests: &Directory,
    receipts: &Directory,
    operation: &Operation,
) -> Result<Option<Outcome>, Error> {
    operation.validate()?;
    let name = format!("{}.json", operation.id);
    match requests.read(&name).await {
        Ok((bytes, _)) => {
            let previous: Operation = serde_json::from_slice(&bytes)?;
            if previous != *operation {
                return Err(Error::Invalid(
                    "Skill operation identity was reused with different input".into(),
                ));
            }
        }
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            atomic(requests, &name, &serde_json::to_vec(operation)?).await?;
        }
        Err(error) => return Err(error),
    }
    read(receipts, operation).await
}
pub(super) async fn read(
    receipts: &Directory,
    operation: &Operation,
) -> Result<Option<Outcome>, Error> {
    operation.validate()?;
    let name = format!("{}.json", operation.id);
    match receipts.read(&name).await {
        Ok((bytes, _)) => decode(&bytes, operation).map(Some),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
/// Read only an original immutable receipt through an explicitly captured view.
/// This neither reserves an operation nor recovers a publication intent.
pub async fn read_view(
    files: &maka_capabilities::filesystem::ReadDirectory,
    operation: &Operation,
) -> Result<Option<Outcome>, Error> {
    operation.validate()?;
    let operation = operation.clone();
    files
        .with_reader(move |reader| {
            let path = format!("{}.json", operation.id);
            match crate::discovery::source::read_view(
                &reader,
                std::path::Path::new(&path),
                &tokio_util::sync::CancellationToken::new(),
            ) {
                Ok(Some(bytes)) => decode(&bytes, &operation).map(Some),
                Ok(None) => Ok(None),
                Err(_) => Err(Error::Invalid("Skill receipt could not be read".into())),
            }
        })
        .await
        .map_err(Error::from)?
}
fn decode(bytes: &[u8], operation: &Operation) -> Result<Outcome, Error> {
    let record: Record = serde_json::from_slice(bytes)?;
    if record.operation != *operation {
        return Err(Error::Invalid(
            "Skill operation receipt does not match its input".into(),
        ));
    }
    Ok(record.outcome)
}
pub(super) async fn complete(
    receipts: &Directory,
    operation: &Operation,
    outcome: Outcome,
) -> Result<(), Error> {
    operation.validate()?;
    let name = format!("{}.json", operation.id);
    match receipts.read(&name).await {
        Ok((bytes, _)) => {
            let previous: Record = serde_json::from_slice(&bytes)?;
            if previous.operation != *operation || previous.outcome != outcome {
                return Err(Error::Invalid(
                    "Skill operation already has a different original outcome".into(),
                ));
            }
            return Ok(());
        }
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let bytes = serde_json::to_vec(&Record {
        operation: operation.clone(),
        outcome,
    })?;
    atomic(receipts, &name, &bytes).await
}
async fn atomic(directory: &Directory, name: &str, bytes: &[u8]) -> Result<(), Error> {
    let pending = format!("{}.pending", uuid::Uuid::new_v4());
    directory.write_new(&pending, bytes, 0o600).await?;
    directory.rename(&pending, directory, name).await?;
    directory.sync().await
}
