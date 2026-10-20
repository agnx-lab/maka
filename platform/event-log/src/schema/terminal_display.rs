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

//! One-time conversion of the old text-only terminal record during migration 35.
use maka_agent_facts::terminal::{
    TerminalScreen, TerminalSize,
    display::{Alternate, Color, Display, Row, Run, Style},
};
use std::sync::Arc;
use unicode_width::UnicodeWidthChar;

pub(crate) async fn register(
    connection: &mut sqlx::SqliteConnection,
    install: bool,
) -> Result<(), crate::StoreError> {
    let mut locked = connection.lock_handle().await?;
    // SAFETY: SQLx's guard excludes its worker; this wrapper borrows, never owns,
    // the shared SQLite handle, and no statement or await escapes this scope.
    let borrowed = unsafe { rusqlite::Connection::from_handle(locked.as_raw_handle().as_ptr()) }?;
    if install {
        borrowed.create_scalar_function(
            "maka_migrate_terminal_display",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let input: String = context.get(0)?;
                convert(&input).map_err(rusqlite::Error::UserFunctionError)
            },
        )?;
    } else {
        borrowed.remove_function("maka_migrate_terminal_display", 1)?;
    }
    Ok(())
}

fn convert(input: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let old: TerminalScreen = serde_json::from_str(input)?;
    let mut display = Display::new(old.size);
    display.metadata.cursor = old.cursor;
    display.metadata.input = old.input;
    display.metadata.alternate_screen = old.alternate_screen;
    // Original colors, styled blanks and soft-wrap boundaries were discarded.
    display.metadata.truncated = true;
    display.screen = rows(&old.screen, old.size.cols())?;
    if display.screen.len() > usize::from(old.size.rows()) {
        return Err("old screen exceeds geometry".into());
    }
    display
        .screen
        .resize(usize::from(old.size.rows()), blank(old.size.cols()));
    display.scrollback = if old.scrollback.is_empty() {
        Vec::new()
    } else {
        rows(&old.scrollback, old.size.cols())?
    };
    if let Some(text) = old.last_alternate_screen {
        // The saved alternate may predate a resize. Its original dimensions
        // were lost, so infer geometry that preserves every known character.
        let cols = text
            .lines()
            .map(|line| line.chars().map(|c| c.width().unwrap_or(1)).sum::<usize>())
            .max()
            .unwrap_or(0)
            .max(usize::from(old.size.cols()));
        let cols = u16::try_from(cols)?;
        let rows = rows(&text, cols)?;
        let size = TerminalSize::new(cols, u16::try_from(rows.len())?)?;
        display.last_alternate_screen = Some(Arc::new(Alternate { size, rows }));
    }
    display.validate()?;
    Ok(serde_json::to_string(&display)?)
}
fn style(flags: u16) -> Style {
    Style {
        foreground: Color::Named(256),
        background: Color::Named(257),
        flags,
        underline: None,
    }
}
fn blank(cols: u16) -> Row {
    vec![Run {
        style: style(0),
        text: " ".repeat(usize::from(cols)),
        lengths: Vec::new(),
    }]
}

// Convert literal text to cells, never feed it through an ANSI interpreter.
fn rows(text: &str, cols: u16) -> Result<Vec<Row>, &'static str> {
    let cols = usize::from(cols);
    let mut output = Vec::new();
    for line in text.split('\n') {
        let mut cells: Vec<(String, u16)> = Vec::new();
        for c in line.chars() {
            let width = c.width().unwrap_or(1);
            if width == 0 {
                let index = cells.iter().rposition(|(_, flags)| *flags & 64 == 0);
                if let Some(index) = index {
                    if cells[index].0.chars().count() >= 33 {
                        return Err("old terminal combining sequence exceeds cell capacity");
                    }
                    cells[index].0.push(c);
                } else {
                    cells.push((format!(" {c}"), 0));
                }
                continue;
            }
            if cells.len() + width > cols {
                while cells.len() < cols {
                    cells.push((" ".into(), 1024));
                }
                cells.last_mut().unwrap().1 |= 16;
                output.push(pack(std::mem::take(&mut cells)));
            }
            cells.push((c.to_string(), if width == 2 { 32 } else { 0 }));
            if width == 2 {
                cells.push((" ".into(), 64));
            }
        }
        while cells.len() < cols {
            cells.push((" ".into(), 0));
        }
        output.push(pack(cells));
    }
    Ok(output)
}
fn pack(cells: Vec<(String, u16)>) -> Row {
    let mut row: Row = Vec::new();
    for (text, flags) in cells {
        let count = text.chars().count() as u8;
        if let Some(run) = row.last_mut()
            && run.style.flags == flags
        {
            if count != 1 && run.lengths.is_empty() {
                run.lengths.resize(run.text.chars().count(), 1);
            }
            if !run.lengths.is_empty() {
                run.lengths.push(count);
            }
            run.text.push_str(&text);
        } else {
            row.push(Run {
                style: style(flags),
                text,
                lengths: if count == 1 { Vec::new() } else { vec![count] },
            });
        }
    }
    row
}
