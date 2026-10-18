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

use super::{MAX_PAGE_CHARS, PREFIX, ReadError, ReadInput, ReadRequest, Start, URL_SAFE_NO_PAD};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::num::NonZeroUsize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadPage {
    pub content: String,
    pub offset: usize,
    pub returned_lines: usize,
    pub total_lines: usize,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial_line: bool,
    pub next: Option<ReadInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<super::ReadMetadata>,
}

#[derive(Clone, Copy, Default)]
struct Position {
    byte: usize,
    utf16: usize,
    line: usize,
    line_start: usize,
    before_newline: bool,
}

/// One text scan, retaining only candidates for the requested bounded page.
/// The caller must feed the entire source before finishing, including its tail.
pub struct TextPageScan<'a> {
    request: &'a ReadRequest,
    budget: usize,
    digest: Sha256,
    position: Position,
    start: Option<Position>,
    end: Option<Position>,
    content: String,
    candidates: Vec<Position>,
    full: bool,
}

impl ReadRequest {
    pub fn scan_with_budget(&self, budget: usize) -> TextPageScan<'_> {
        TextPageScan {
            request: self,
            budget,
            digest: Sha256::new(),
            position: Position::default(),
            start: None,
            end: None,
            content: String::new(),
            candidates: Vec::new(),
            full: false,
        }
    }

    pub fn page(&self, text: &str) -> Result<ReadPage, ReadError> {
        self.page_with_budget(text, MAX_PAGE_CHARS)
    }

    /// Budget is the serialized page's UTF-16 length, including continuation.
    /// Callers adding execution metadata reserve its serialized size here.
    pub fn page_with_budget(&self, text: &str, budget: usize) -> Result<ReadPage, ReadError> {
        let mut scan = self.scan_with_budget(budget);
        scan.push(text);
        scan.finish()
    }
}

impl TextPageScan<'_> {
    fn select_start(&mut self) {
        if self.start.is_some() {
            return;
        }
        let matches = match self.request.start {
            Start::Line(line) => {
                self.position.line == line && self.position.byte == self.position.line_start
            }
            Start::Continuation { position, .. } => self.position.utf16 == position,
        };
        if matches {
            self.start = Some(self.position);
            self.candidates.push(self.position);
        }
    }

    pub fn push(&mut self, text: &str) {
        self.digest.update(text.as_bytes());
        for character in text.chars() {
            self.select_start();
            self.position.before_newline = character == '\n';
            if let Some(last) = self.candidates.last_mut()
                && last.byte == self.position.byte
            {
                last.before_newline = self.position.before_newline;
            }
            if let Some(start) = self.start {
                if self.end.is_none()
                    && character == '\n'
                    && self.request.limit.is_some_and(|limit| {
                        self.position.line + 1 == start.line.saturating_add(limit.get())
                    })
                {
                    self.end = Some(self.position);
                }
                if self.end.is_none() && !self.full {
                    if self.position.utf16 - start.utf16 + character.len_utf16() <= self.budget {
                        self.content.push(character);
                    } else {
                        self.full = true;
                    }
                }
            }
            self.position.byte += character.len_utf8();
            self.position.utf16 += character.len_utf16();
            if character == '\n' {
                self.position.line += 1;
                self.position.line_start = self.position.byte;
            }
            self.position.before_newline = false;
            if self.start.is_some() && self.end.is_none() && !self.full {
                self.candidates.push(self.position);
            }
        }
    }

    pub fn finish(mut self) -> Result<ReadPage, ReadError> {
        self.select_start();
        let digest = hex::encode(self.digest.finalize());
        let digest = &digest[..32];
        if let Start::Continuation {
            digest: expected, ..
        } = &self.request.start
        {
            if digest != expected {
                return Err(ReadError::ContentChanged);
            }
            if self.start.is_none() {
                return Err(ReadError::InvalidPosition);
            }
        }
        let start = self.start.unwrap_or(self.position);
        if self.candidates.is_empty() {
            self.candidates.push(start);
        }
        let offset = match self.request.start {
            Start::Line(offset) => offset,
            Start::Continuation { .. } => start.line,
        };
        let total_lines = self.position.line + 1;
        let last_line = offset
            .saturating_add(self.request.limit.map_or(total_lines, NonZeroUsize::get))
            .min(total_lines);
        let end = self.end.unwrap_or(self.position);
        let make_page = |stop: Position| {
            let complete = stop.byte >= end.byte;
            let next_line = offset.max(stop.line);
            let boundary = stop.before_newline && stop.line == next_line;
            let partial = !complete && !boundary;
            let returned_lines = if offset >= total_lines {
                0
            } else if complete {
                last_line - offset
            } else {
                next_line - offset + usize::from(!partial)
            };
            ReadPage {
                metadata: None,
                content: self.content[..stop.byte.saturating_sub(start.byte)].into(),
                offset,
                returned_lines,
                total_lines,
                partial_line: partial || (offset < total_lines && start.byte > start.line_start),
                next: (!complete).then(|| ReadInput {
                    path: format!(
                        "{PREFIX}{}?at={}&sha={digest}",
                        URL_SAFE_NO_PAD.encode(&self.request.path),
                        stop.utf16 + usize::from(!partial)
                    ),
                    offset: None,
                    limit: self.request.limit.map(|limit| {
                        NonZeroUsize::new(limit.get() - returned_lines)
                            .expect("incomplete line range")
                    }),
                }),
            }
        };
        let fits = |page: &ReadPage| {
            serde_json::to_string(page)
                .expect("ReadPage contains only JSON fields")
                .encode_utf16()
                .count()
                <= self.budget
        };
        if end.utf16.saturating_sub(start.utf16) <= self.budget {
            let page = make_page(end);
            if fits(&page) {
                return Ok(page);
            }
        }
        let (mut low, mut high) = (0, self.candidates.len() - 1);
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            if fits(&make_page(self.candidates[middle])) {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        if low == 0 {
            return Err(ReadError::AddressTooLong);
        }
        let mut stop = self.candidates[low];
        if let Some(relative) = self.content[..stop.byte - start.byte]
            .rfind('\n')
            .filter(|relative| *relative > 0 && !stop.before_newline)
        {
            stop = *self
                .candidates
                .iter()
                .find(|candidate| candidate.byte == start.byte + relative)
                .expect("retained scalar boundary");
        }
        let page = make_page(stop);
        if !fits(&page) {
            return Err(ReadError::AddressTooLong);
        }
        Ok(page)
    }
}

#[cfg(test)]
fn content_digest(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))[..32].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bounded_pages_preserve_line_ranges_unicode_and_reject_changed_content() {
        for text in [
            String::new(),
            "\n\n".into(),
            "一\r\n二\n".into(),
            format!(
                "\n{}\n{}\nend\n",
                "中😀\"\\\t".repeat(2_000),
                "short\n".repeat(800)
            ),
        ] {
            for (offset, limit) in [(0, None), (1, Some(2)), (999_999, Some(3))] {
                let mut input = ReadInput {
                    path: "maka://runtime/attachments/报告".into(),
                    offset: Some(offset),
                    limit: limit.and_then(NonZeroUsize::new),
                };
                let expected = text
                    .split('\n')
                    .skip(offset)
                    .take(limit.unwrap_or(usize::MAX))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut actual = String::new();
                let mut consumed = text
                    .split_inclusive('\n')
                    .take(offset)
                    .map(|line| line.encode_utf16().count())
                    .sum::<usize>();
                let mut count = 0;
                loop {
                    let page = input
                        .resolve()
                        .unwrap()
                        .page_with_budget(&text, 512)
                        .unwrap();
                    for chunk_bytes in [1, 7, 8192] {
                        let request = input.resolve().unwrap();
                        let mut scan = request.scan_with_budget(512);
                        let mut start = 0;
                        while start < text.len() {
                            let end =
                                text.ceil_char_boundary((start + chunk_bytes).min(text.len()));
                            scan.push(&text[start..end]);
                            start = end;
                        }
                        assert_eq!(scan.finish().unwrap(), page);
                    }
                    assert!(serde_json::to_string(&page).unwrap().encode_utf16().count() <= 512);
                    actual.push_str(&page.content);
                    consumed += page.content.encode_utf16().count();
                    count += 1;
                    assert!(count < 1_000, "every continuation must progress");
                    let Some(next) = page.next else { break };
                    assert!(!page.content.is_empty());
                    let Start::Continuation { position, .. } = next.resolve().unwrap().start else {
                        panic!("expected continuation")
                    };
                    assert!((consumed..=consumed + 1).contains(&position));
                    if position > consumed {
                        actual.push('\n');
                        consumed += 1;
                    }
                    assert_eq!(
                        next.resolve().unwrap().page(&(text.clone() + "changed")),
                        Err(ReadError::ContentChanged)
                    );
                    input = next;
                }
                assert!(
                    actual == expected,
                    "line range offset={offset} limit={limit:?} did not reconstruct the source"
                );
            }
        }
        let text = "😀".repeat(8_000);
        let input: ReadInput = serde_json::from_value(json!({"path":"file"})).unwrap();
        let page = input.resolve().unwrap().page(&text).unwrap();
        assert!(page.partial_line);
        let mut request = page.next.unwrap().resolve().unwrap();
        for invalid in [1, text.encode_utf16().count() + 1] {
            request.start = Start::Continuation {
                position: invalid,
                digest: content_digest(&text),
            };
            assert_eq!(request.page(&text), Err(ReadError::InvalidPosition));
        }
        let input: ReadInput =
            serde_json::from_value(json!({"path":"a".repeat(MAX_PAGE_CHARS)})).unwrap();
        assert_eq!(
            input.resolve().unwrap().page(&text),
            Err(ReadError::AddressTooLong)
        );
    }

    #[test]
    fn every_scalar_cut_preserves_literal_bom_nul_and_line_metadata() {
        let text = "\u{feff}零\0😀\r\n尾\n";
        let input: ReadInput = serde_json::from_value(json!({"path":"file"})).unwrap();
        let request = input.resolve().unwrap();
        for cut in text
            .char_indices()
            .map(|(byte, _)| byte)
            .chain([text.len()])
        {
            let mut scan = request.scan_with_budget(512);
            scan.push(&text[..cut]);
            scan.push(&text[cut..]);
            assert_eq!(
                serde_json::to_value(scan.finish().unwrap()).unwrap(),
                json!({
                    "content":"\u{feff}零\0😀\r\n尾\n","offset":0,"returnedLines":3,"totalLines":3,"next":null
                })
            );
        }
        let request = ReadRequest {
            path: "file".into(),
            start: Start::Continuation {
                position: 999,
                digest: "0".repeat(32),
            },
            limit: None,
        };
        let mut scan = request.scan_with_budget(512);
        scan.push(text);
        assert_eq!(scan.finish(), Err(ReadError::ContentChanged));
    }
}
