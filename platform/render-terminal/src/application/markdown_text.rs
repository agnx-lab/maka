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

//! The original TUI CopyMessage formatter, as a bounded, effect-free query.
use maka_ui_runtime::{
    NativeCapability, NativeExport, NativeRegistry,
    actions::{NativeService, ServiceCompletion, ServiceFuture, ServiceRequest},
    markdown::{MarkdownTextInput, MarkdownTextResult},
};
use std::{collections::BTreeMap, sync::Arc};
struct Service;
fn plain(source: &str) -> Result<MarkdownTextResult, String> {
    let document = crate::markdown::prepared::Document::markdown(
        source,
        false,
        crate::markdown::Palette::default(),
    )
    .map_err(str::to_owned)?;
    Ok(MarkdownTextResult::Ready {
        text: document.text().to_owned(),
    })
}
impl NativeService for Service {
    fn call(&self, request: ServiceRequest) -> ServiceFuture {
        Box::pin(async move {
            let result = (|| {
                if request.method != "plain" {
                    return Err("unknown Markdown text method".into());
                }
                let input: MarkdownTextInput =
                    serde_json::from_value(request.input).map_err(|e| e.to_string())?;
                serde_json::to_value(plain(&input.source)?).map_err(|e| e.to_string())
            })();
            ServiceCompletion {
                result,
                cleanup: Ok(()),
            }
        })
    }
}
pub(super) fn register(registry: &mut NativeRegistry<dyn super::Widget>) {
    registry
        .register(
            &maka_ui_runtime::markdown::native_markdown_text().implementation,
            |_, _| {
                Ok(BTreeMap::from([(
                    "MarkdownText".into(),
                    NativeExport {
                        contract: maka_ui_runtime::markdown::text_contract(),
                        value: NativeCapability::Service(Arc::new(Service)),
                    },
                )]))
            },
        )
        .expect("unique terminal Markdown text formatter");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copy_reuses_native_markdown_semantics_for_complete_utf8() {
        let source =
            "# Heading\n\n**bold** [link](https://example.org)\n\n```rs\nlet x = 1;\n```\n";
        let MarkdownTextResult::Ready { text } = plain(source).unwrap();
        assert!(text.contains("Heading"));
        assert!(text.contains("bold link"));
        assert!(text.contains("let x = 1;"));
        assert!(!text.contains("**"));
        assert!(text.contains("https://example.org"));
        assert!(matches!(
            plain(&"中".repeat(21846)).unwrap(),
            MarkdownTextResult::Ready { .. }
        ));
        assert!(matches!(
            plain(&"a".repeat(65536)).unwrap(),
            MarkdownTextResult::Ready { .. }
        ));
    }
}
