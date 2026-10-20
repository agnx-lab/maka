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

//! A bounded presentation of CommonMark/GFM, independent of Host content.

use ::markdown::{ParseOptions, mdast::Node};
use nsh_core::ui::prelude::*;

const MAX_BLOCKS: usize = 48;
const MAX_RUNS: usize = 2048;

/// Inline content; unsupported constructs retain their complete source spelling.
#[derive(Clone, Debug, PartialEq, JsObject)]
pub struct MarkdownRun {
    pub key: String,
    pub text: String,
    pub strong: bool,
    pub emphasis: bool,
    pub strike: bool,
    pub code: bool,
}

/// Rendering supported by the terminal application's rich text primitives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PropEnum)]
pub enum MarkdownBlockKind {
    Paragraph,
    Heading,
    Code,
    Literal,
    Rule,
}

/// A block's key uses its starting source offset, so appending text preserves it.
#[derive(Clone, Debug, PartialEq, JsObject)]
pub struct MarkdownBlock {
    pub key: String,
    pub kind: MarkdownBlockKind,
    pub indent: u32,
    pub quote: u32,
    pub marker: String,
    pub runs: Vec<MarkdownRun>,
    pub text: String,
    pub level: u32,
    pub language: String,
}

/// Stateless, local Markdown presentation; it reads no files and follows no URLs.
#[class]
pub struct Markdown;

#[class]
impl Markdown {
    /// Parse on the launcher's existing blocking pool, away from the UI thread.
    pub async fn parse(this: Instance<Self>, source: String) -> Result<Vec<MarkdownBlock>, String> {
        drop(this);
        tokio::task::spawn_blocking(move || parse_source(&source))
            .await
            .map_err(|error| format!("Markdown worker failed: {error}"))?
    }
}

/// The runtime and declaration generator share this one native export.
pub fn registry() -> Registry {
    let mut registry = Registry::ui_set("maka/markdown");
    registry.value("markdown", || Instance::new(Markdown));
    registry
}

fn parse_source(source: &str) -> Result<Vec<MarkdownBlock>, String> {
    let root =
        ::markdown::to_mdast(source, &ParseOptions::gfm()).map_err(|error| error.to_string())?;
    let mut output = Presentation {
        source,
        blocks: Vec::new(),
        runs: 0,
        finished: false,
    };
    output.visit(&root, Context::default());
    Ok(output.blocks.into_iter().map(|(_, block)| block).collect())
}

#[derive(Clone, Copy, Default)]
struct Context {
    indent: u32,
    quote: u32,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Marks {
    strong: bool,
    emphasis: bool,
    strike: bool,
    code: bool,
}

struct Presentation<'a> {
    source: &'a str,
    blocks: Vec<(usize, MarkdownBlock)>,
    runs: usize,
    finished: bool,
}

impl Presentation<'_> {
    fn visit(&mut self, node: &Node, context: Context) {
        if self.finished {
            return;
        }
        match node {
            Node::Root(root) => {
                for child in &root.children {
                    self.visit(child, context);
                }
            }
            Node::Blockquote(quote) => {
                for child in &quote.children {
                    self.visit(
                        child,
                        Context {
                            quote: context.quote + 1,
                            ..context
                        },
                    );
                }
            }
            Node::List(list) => {
                for (index, child) in list.children.iter().enumerate() {
                    let Node::ListItem(item) = child else {
                        unreachable!("Markdown list children are list items");
                    };
                    let mut marker = if list.ordered {
                        format!("{}.", u64::from(list.start.unwrap_or(1)) + index as u64)
                    } else {
                        "•".into()
                    };
                    if let Some(checked) = item.checked {
                        marker.push_str(if checked { " [x]" } else { " [ ]" });
                    }
                    // An empty item, or one beginning with a nested list, still
                    // has its own marker before its descendants.
                    let first = self.blocks.len();
                    if item.children.is_empty() || begins_with_list(item.children.first()) {
                        self.push(
                            self.block(
                                child,
                                MarkdownBlockKind::Paragraph,
                                Context {
                                    indent: context.indent + 1,
                                    ..context
                                },
                            ),
                            child,
                        );
                    }
                    for child in &item.children {
                        self.visit(
                            child,
                            Context {
                                indent: context.indent + 1,
                                ..context
                            },
                        );
                        if self.finished {
                            break;
                        }
                    }
                    // Even if a later child exhausted the budget, this item's
                    // retained first block still owns its marker. The literal
                    // tail itself already contains the original source marker.
                    if !self.finished || first + 1 < self.blocks.len() {
                        if let Some((_, block)) = self.blocks.get_mut(first) {
                            block.marker = marker;
                        }
                    }
                    if self.finished {
                        break;
                    }
                }
            }
            Node::Paragraph(paragraph) => {
                self.phrasing(
                    node,
                    &paragraph.children,
                    MarkdownBlockKind::Paragraph,
                    0,
                    context,
                );
            }
            Node::Heading(heading) => {
                self.phrasing(
                    node,
                    &heading.children,
                    MarkdownBlockKind::Heading,
                    u32::from(heading.depth),
                    context,
                );
            }
            Node::Code(code) => {
                let mut block = self.block(node, MarkdownBlockKind::Code, context);
                block.text.clone_from(&code.value);
                block.language = code.lang.clone().unwrap_or_default();
                self.push(block, node);
            }
            Node::ThematicBreak(_) => {
                let mut block = self.block(node, MarkdownBlockKind::Rule, context);
                block.text = raw(self.source, node).into();
                self.push(block, node);
            }
            // Tables, HTML, images, links, footnotes and definitions have no
            // terminal activation or lossy "text content" representation.
            _ => {
                let mut block = self.block(node, MarkdownBlockKind::Literal, context);
                block.text = raw(self.source, node).into();
                self.push(block, node);
            }
        }
    }

    fn block(&self, node: &Node, kind: MarkdownBlockKind, context: Context) -> MarkdownBlock {
        MarkdownBlock {
            key: format!("{}:{}", kind.js_name(), start(node)),
            kind,
            indent: context.indent,
            quote: context.quote,
            marker: String::new(),
            runs: Vec::new(),
            text: String::new(),
            level: 0,
            language: String::new(),
        }
    }

    fn phrasing(
        &mut self,
        node: &Node,
        children: &[Node],
        kind: MarkdownBlockKind,
        level: u32,
        context: Context,
    ) {
        let mut block = self.block(node, kind, context);
        for child in children {
            if !inline(
                self.source,
                child,
                Marks::default(),
                &mut block.runs,
                MAX_RUNS - self.runs,
            ) {
                self.tail(start(node));
                return;
            }
        }
        block.level = level;
        self.push(block, node);
    }

    fn push(&mut self, block: MarkdownBlock, node: &Node) {
        if self.blocks.len() == MAX_BLOCKS {
            self.tail(start(node));
            return;
        }
        self.runs += block.runs.len();
        self.blocks.push((start(node), block));
    }

    fn tail(&mut self, mut offset: usize) {
        // Reserve the final slot for a complete literal suffix, including the
        // last displayed block if all presentation slots were already used.
        if self.blocks.len() == MAX_BLOCKS {
            offset = self.blocks.pop().expect("presentation slots are full").0;
        }
        // Container markers before a leaf's AST offset belong to this suffix.
        let offset = self.source[..offset]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        self.blocks.push((
            offset,
            MarkdownBlock {
                key: format!("literal:{offset}"),
                kind: MarkdownBlockKind::Literal,
                indent: 0,
                quote: 0,
                marker: String::new(),
                runs: Vec::new(),
                text: self.source[offset..].into(),
                level: 0,
                language: String::new(),
            },
        ));
        self.finished = true;
    }
}

fn begins_with_list(node: Option<&Node>) -> bool {
    match node {
        Some(Node::List(_)) => true,
        Some(Node::Blockquote(quote)) => begins_with_list(quote.children.first()),
        _ => false,
    }
}

fn inline(
    source: &str,
    node: &Node,
    marks: Marks,
    runs: &mut Vec<MarkdownRun>,
    limit: usize,
) -> bool {
    let children = match node {
        Node::Strong(strong) => Some((
            &strong.children,
            Marks {
                strong: true,
                ..marks
            },
        )),
        Node::Emphasis(emphasis) => Some((
            &emphasis.children,
            Marks {
                emphasis: true,
                ..marks
            },
        )),
        Node::Delete(delete) => Some((
            &delete.children,
            Marks {
                strike: true,
                ..marks
            },
        )),
        _ => None,
    };
    if let Some((children, marks)) = children {
        return children
            .iter()
            .all(|child| inline(source, child, marks, runs, limit));
    }
    let (text, marks) = match node {
        Node::Text(text) => (&*text.value, marks),
        Node::InlineCode(code) => (
            &*code.value,
            Marks {
                code: true,
                ..marks
            },
        ),
        Node::Break(_) => ("\n", marks),
        _ => (raw(source, node), marks),
    };
    if text.is_empty() {
        return true;
    }
    if let Some(previous) = runs.last_mut() {
        if previous.strong == marks.strong
            && previous.emphasis == marks.emphasis
            && previous.strike == marks.strike
            && previous.code == marks.code
        {
            previous.text.push_str(text);
            return true;
        }
    }
    if runs.len() == limit {
        return false;
    }
    runs.push(MarkdownRun {
        key: format!("inline:{}", start(node)),
        text: text.into(),
        strong: marks.strong,
        emphasis: marks.emphasis,
        strike: marks.strike,
        code: marks.code,
    });
    true
}

fn start(node: &Node) -> usize {
    node.position()
        .expect("Markdown parser assigns source positions")
        .start
        .offset
}

fn raw<'a>(source: &'a str, node: &Node) -> &'a str {
    let position = node
        .position()
        .expect("Markdown parser assigns source positions");
    &source[position.start.offset..position.end.offset]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_blocks_keep_structure_and_nested_inline_marks() {
        let blocks = parse_source(
            "# Heading\n\nPlain **bold and *both***, ~~gone~~ and `code`.\n\n> quote\n\n3. first\n\n   second\n\n- [x] done\n\n```rust\nlet answer = 42;\n```\n\n---",
        )
        .unwrap();
        assert_eq!(blocks[0].kind, MarkdownBlockKind::Heading);
        assert_eq!(blocks[0].level, 1);
        assert_eq!(blocks[0].runs[0].text, "Heading");
        assert!(
            blocks[1]
                .runs
                .iter()
                .any(|run| run.strong && run.emphasis && run.text == "both")
        );
        assert!(
            blocks[1]
                .runs
                .iter()
                .any(|run| run.strike && run.text == "gone")
        );
        assert!(
            blocks[1]
                .runs
                .iter()
                .any(|run| run.code && run.text == "code")
        );
        assert_eq!(blocks[2].quote, 1);
        assert_eq!(blocks[3].marker, "3.");
        assert_eq!(blocks[4].marker, "");
        assert_eq!(blocks[5].marker, "• [x]");
        assert_eq!(blocks[6].kind, MarkdownBlockKind::Code);
        assert_eq!(blocks[6].language, "rust");
        assert_eq!(blocks[6].text, "let answer = 42;");
        assert_eq!(blocks[7].kind, MarkdownBlockKind::Rule);
        assert_eq!(blocks[7].text, "---");
    }

    #[test]
    fn unicode_offsets_and_append_preserve_block_and_run_identity() {
        let first = parse_source("你好 **粗体** 👩🏽‍💻 e\u{301}").unwrap();
        let next = parse_source("你好 **粗体** 👩🏽‍💻 e\u{301} more").unwrap();
        assert_eq!(first[0].key, next[0].key);
        assert_eq!(first[0].runs[0].key, next[0].runs[0].key);
        assert_eq!(first[0].runs[1].text, "粗体");
        assert_eq!(first[0].runs[2].text, " 👩🏽‍💻 e\u{301}");
        assert_eq!(next[0].runs[1].key, first[0].runs[1].key);
    }

    #[test]
    fn unsupported_constructs_keep_complete_source_without_activation() {
        let source = "[label](https://example.test/target) ![alt](image.png) [^note]\n\n| left | right |\n| --- | --- |\n| 一 | 二 |\n\n<div>HTML</div>\n\n[id]: https://example.test/definition\n\n[^note]: footnote\n\n$E = mc^2$ \\(x\\) \\[y\\]";
        let blocks = parse_source(source).unwrap();
        assert_eq!(
            blocks[0].runs[0].text,
            "[label](https://example.test/target) ![alt](image.png) [^note]"
        );
        for original in [
            "| left | right |\n| --- | --- |\n| 一 | 二 |",
            "<div>HTML</div>",
            "[id]: https://example.test/definition",
            "[^note]: footnote",
        ] {
            assert!(blocks.iter().any(|block| block.kind == MarkdownBlockKind::Literal && block.text == original));
        }
        assert_eq!(blocks.last().unwrap().runs[0].text, "$E = mc^2$ (x) [y]");
    }

    #[test]
    fn block_budget_keeps_the_entire_remaining_source_literal() {
        let source = (1..=60)
            .map(|number| format!("段落 {number} 👩🏽‍💻"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let blocks = parse_source(&source).unwrap();
        assert_eq!(blocks.len(), 48);
        let tail = blocks.last().unwrap();
        assert_eq!(tail.kind, MarkdownBlockKind::Literal);
        assert_eq!(tail.text, source[source.find("段落 48 ").unwrap()..]);
        assert!(tail.text.ends_with("段落 60 👩🏽‍💻"));
    }

    #[test]
    fn inline_budget_keeps_the_complete_block_and_tail_literal() {
        let rich = "**bold** plain ".repeat(1100);
        let source = format!("Earlier\n\n{rich}\n\nLast ![image](retained.png)");
        let blocks = parse_source(&source).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].runs[0].text, "Earlier");
        assert_eq!(blocks[1].kind, MarkdownBlockKind::Literal);
        assert_eq!(blocks[1].text, source["Earlier\n\n".len()..]);
        assert!(blocks[1].text.ends_with("![image](retained.png)"));
    }

    #[test]
    fn empty_list_items_keep_their_marker() {
        let blocks = parse_source("-\n- filled").unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].marker, "•");
        assert!(blocks[0].runs.is_empty());
        assert_eq!(blocks[1].marker, "•");
        assert_eq!(blocks[1].runs[0].text, "filled");
    }

    #[test]
    fn nested_list_through_quote_keeps_both_item_markers() {
        let blocks = parse_source("- > 3. nested").unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].marker, "•");
        assert!(blocks[0].runs.is_empty());
        assert_eq!(blocks[1].marker, "3.");
        assert_eq!(blocks[1].quote, 1);
        assert_eq!(blocks[1].runs[0].text, "nested");
    }

    #[test]
    fn budget_in_a_later_item_paragraph_keeps_its_first_marker() {
        let source = format!(
            "- first\n\n{}",
            (2..=50)
                .map(|number| format!("  paragraph {number}"))
                .collect::<Vec<_>>()
                .join("\n\n")
        );
        let blocks = parse_source(&source).unwrap();
        assert_eq!(blocks[0].marker, "•");
        assert_eq!(blocks[0].runs[0].text, "first");
        assert_eq!(
            blocks.last().unwrap().text,
            source[source.find("  paragraph 48").unwrap()..]
        );
    }
}
