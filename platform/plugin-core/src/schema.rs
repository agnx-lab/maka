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

//! Shared schema data. Runtime inputs and configuration values remain owned JSON.
use std::{borrow::Cow, sync::Arc};

use schemars::{JsonSchema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Immutable JSON schema storage shared by declarations, plans and live contracts.
/// Construction preserves JSON exactly; declaration admission still validates it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Schema(Arc<Value>);

impl From<Value> for Schema {
    fn from(value: Value) -> Self {
        Self(Arc::new(value))
    }
}
impl From<schemars::Schema> for Schema {
    fn from(value: schemars::Schema) -> Self {
        Self::from(Value::from(value))
    }
}
impl AsRef<Value> for Schema {
    fn as_ref(&self) -> &Value {
        &self.0
    }
}
impl std::ops::Deref for Schema {
    type Target = Value;
    fn deref(&self) -> &Value {
        self.as_ref()
    }
}
impl JsonSchema for Schema {
    fn schema_name() -> Cow<'static, str> {
        Value::schema_name()
    }
    fn schema_id() -> Cow<'static, str> {
        Value::schema_id()
    }
    fn inline_schema() -> bool {
        Value::inline_schema()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> schemars::Schema {
        Value::json_schema(generator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_projection_and_wire_representation_remain_json() {
        assert_eq!(schemars::schema_for!(Schema), schemars::schema_for!(Value));
        for value in [
            json!(null),
            json!(true),
            json!({"type":"object", "properties":{"name":{"type":"string"}}}),
        ] {
            let schema: Schema = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&schema).unwrap(), value);
            assert_eq!(schema, Schema::from(value));
        }
    }
}
