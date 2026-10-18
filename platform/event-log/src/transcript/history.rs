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
    read::{prepared, row_position, sql_number, sql_position},
    tool_metadata::read as tool_metadata,
};
use crate::{EventLog, StoreError, sequence_number};
use maka_agent_facts::history::RowPosition;
use maka_agent_ports::session::history::Chunk;
use maka_agent_ports::session::history::Cursor;
use maka_agent_ports::session::history::Page;
use maka_agent_ports::session::history::Position;
use maka_agent_ports::session::history::Role;
use maka_agent_ports::session::history::{HistoryField, HistoryView};
use sqlx::{Connection, Row, SqliteConnection};

// One body projection for conversation windows and exact source reads.
pub(super) const BODY: &str = "COALESCE(CASE json_extract(payload, '$.type')
                    WHEN 'user' THEN (SELECT group_concat(line, char(10)) FROM (
                        SELECT COALESCE(json_extract(payload, '$.displayText'), json_extract(payload, '$.text'), '') AS line
                            WHERE COALESCE(json_extract(payload, '$.displayText'), json_extract(payload, '$.text'), '') != ''
                        UNION ALL
                        SELECT '↳ ' || json_extract(value, '$.name') || CASE
                            WHEN json_type(value, '$.bytes') IS NOT 'integer' THEN ''
                            WHEN json_extract(value, '$.bytes') < 1024 THEN ' · ' || json_extract(value, '$.bytes') || ' B'
                            WHEN json_extract(value, '$.bytes') < 1048576 THEN printf(' · %.1f KiB', json_extract(value, '$.bytes') / 1024.0)
                            ELSE printf(' · %.1f MiB', json_extract(value, '$.bytes') / 1048576.0) END
                            FROM json_each(payload, '$.attachments') WHERE json_type(value, '$.name') = 'text'
                        UNION ALL
                        SELECT '▱ ' || json_extract(value, '$.path')
                            FROM json_each(payload, '$.directoryReferences') WHERE json_type(value, '$.path') = 'text'))
                    WHEN 'assistant' THEN json_extract(payload, '$.text')
                    WHEN 'model_interruption' THEN json_extract(payload, '$.message')
                    WHEN 'turn_state' THEN CASE WHEN json_extract(payload, '$.status') = 'failed' THEN
                        COALESCE(NULLIF(json_extract(payload, '$.failureMessage'), ''), NULLIF(json_extract(payload, '$.errorClass'), ''), 'failed') END
                    WHEN 'tool_call' THEN COALESCE(json_extract(payload, '$.intent'), json_extract(payload, '$.args.intent'), '')
                    WHEN 'tool_result' THEN CASE json_extract(payload, '$.content.kind')
                        WHEN 'text' THEN json_extract(payload, '$.content.text')
                        WHEN 'json' THEN (SELECT group_concat(atom, char(10)) FROM json_tree(payload, '$.content.value') WHERE type = 'text')
                        WHEN 'operation' THEN (SELECT group_concat(atom, char(10)) FROM json_tree(payload, '$.content.reply.receipt.outcome.value') WHERE type = 'text')
                        ELSE '' END
                    ELSE '' END, '')";

impl EventLog {
    /// Exact ascending UTF-8 windows over immutable derived text. The byte and
    /// fragment budgets apply to the whole window, never to a silently clipped
    /// message. Before reads a prefix ending exclusively at its cursor; After
    /// starts inclusively. `max_json_bytes` bounds the complete serialized page,
    /// including metadata and empty projection boundaries. An indivisible first
    /// fragment that exceeds it returns an error. Cursor positions grant no access.
    #[allow(clippy::too_many_arguments)]
    pub async fn history_text(
        &self,
        session: &str,
        through: u64,
        position: Position,
        include_thinking: bool,
        max_bytes: usize,
        max_chunks: usize,
        max_json_bytes: usize,
    ) -> Result<Page, StoreError> {
        self.history_view(
            session,
            through,
            position,
            include_thinking,
            HistoryView::Text,
            max_bytes,
            Some(max_chunks),
            max_json_bytes,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn history_view(
        &self,
        session: &str,
        through: u64,
        position: Position,
        include_thinking: bool,
        view: HistoryView,
        max_bytes: usize,
        max_chunks: Option<usize>,
        max_json_bytes: usize,
    ) -> Result<Page, StoreError> {
        self.validate_root()?;
        if !(4..=128 * 1024).contains(&max_bytes)
            || max_chunks == Some(0)
            || !(1024..=1024 * 1024).contains(&max_json_bytes)
        {
            return Err(invalid("invalid history window limits"));
        }
        crate::sessions::validate_id(session)?;
        let fence = through;
        let through_sql = sql_number(fence)?;
        // A serialized chunk occupies at least one byte. The delivery budget
        // therefore bounds rows even when a caller needs no item-count limit.
        let max_chunks = max_chunks.unwrap_or(max_json_bytes).min(max_json_bytes);
        let session = session.to_owned();
        self.connection.run(move |connection| Box::pin(async move {
            let mut tx = connection.begin().await?;
            prepared(&mut tx, &session, fence).await?;
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM session_control WHERE id = ?)"
            ).bind(&session).fetch_one(&mut *tx).await?;
            if !exists { return Err(invalid("Session no longer exists")); }
            let target = if let Position::Row { position, field } = position {
                if position.validate().is_err() || position.source_sequence > fence
                    || (!include_thinking && field == HistoryField::Thinking) {
                    return Err(invalid("history row exceeds its fence"));
                }
                materialize(&mut tx, &session, position, view, through_sql).await?;
                // Header lookup requires the actual field; a tool Body cannot
                // impersonate a Thinking field that was never recorded.
                header(&mut tx, &session, position, field, view, through_sql).await?;
                Some((position, field))
            } else { None };
            let anchor = match position {
                Position::Before { cursor } | Position::After { cursor } => {
                    if cursor.position.source_sequence > fence || (!include_thinking && cursor.field == HistoryField::Thinking) {
                        return Err(invalid("history cursor exceeds its fence"));
                    }
                    sql_number(cursor.offset)?;
                    materialize(&mut tx, &session, cursor.position, view, through_sql).await?;
                    let row = header(&mut tx, &session, cursor.position, cursor.field, view, through_sql).await?;
                    if cursor.offset > row.total {
                        return Err(invalid("history offset exceeds message"));
                    }
                    let byte: Vec<u8> = if cursor.offset == row.total { Vec::new() } else { sqlx::query_scalar(
                        "SELECT substr(body, ?4, 1) FROM transcript_text WHERE session_id = ?1 AND source_sequence = ?2 AND item_index = ?3 AND field = ?5"
                    ).bind(&session).bind(sql_position(cursor.position)?.0).bind(sql_position(cursor.position)?.1).bind(sql_number(cursor.offset)? + 1).bind(field_name(cursor.field))
                        .fetch_one(&mut *tx).await? };
                    if byte.first().is_some_and(|byte| byte & 0xc0 == 0x80) {
                        return Err(invalid("history offset splits UTF-8"));
                    }
                    Some((cursor, row))
                }
                _ => None,
            };
            let (reverse, boundary, inclusive) = match position {
                Position::Start => (false, None, true),
                Position::Tail => (true, None, true),
                Position::Row { position, .. } => (false, Some(position), true),
                Position::Before { cursor } => (true, Some(cursor.position), cursor.offset > 0 || (include_thinking && cursor.field == HistoryField::Body)),
                Position::After { cursor } => (false, Some(cursor.position), cursor.offset < anchor.as_ref().unwrap().1.total || cursor.field == HistoryField::Thinking),
                Position::Event { sequence } => {
                    if sequence == 0 || sequence > through { return Err(invalid("history event exceeds its fence")); }
                    (false, Some(RowPosition { source_sequence: sequence, item_index: 0 }), true)
                }
            };
            let query = match (reverse, boundary.is_some(), inclusive) {
                (true, false, _) => "SELECT source_sequence,item_index FROM transcript_rows WHERE session_id=?1 AND source_sequence<=?2 ORDER BY source_sequence DESC,item_index DESC LIMIT ?3",
                (false, false, _) => "SELECT source_sequence,item_index FROM transcript_rows WHERE session_id=?1 AND source_sequence<=?2 ORDER BY source_sequence,item_index LIMIT ?3",
                (true, true, true) => "SELECT source_sequence,item_index FROM transcript_rows WHERE session_id=?1 AND (source_sequence,item_index)<=(?3,?4) ORDER BY source_sequence DESC,item_index DESC LIMIT ?2",
                (true, true, false) => "SELECT source_sequence,item_index FROM transcript_rows WHERE session_id=?1 AND (source_sequence,item_index)<(?3,?4) ORDER BY source_sequence DESC,item_index DESC LIMIT ?2",
                (false, true, true) => "SELECT source_sequence,item_index FROM transcript_rows WHERE session_id=?1 AND source_sequence<=?2 AND (source_sequence,item_index)>=(?4,?5) ORDER BY source_sequence,item_index LIMIT ?3",
                (false, true, false) => "SELECT source_sequence,item_index FROM transcript_rows WHERE session_id=?1 AND source_sequence<=?2 AND (source_sequence,item_index)>(?4,?5) ORDER BY source_sequence,item_index LIMIT ?3",
            };
            let mut query = sqlx::query_as::<_,(i64,i64)>(sqlx::AssertSqlSafe(query)).bind(&session);
            // A validated reverse anchor already implies the raw fence. Keep the
            // cursor pair as the sole upper bound so SQLite seeks directly to it.
            if !(reverse && boundary.is_some()) { query=query.bind(through_sql); }
            query=query.bind((max_chunks + 1) as i64);
            if let Some(boundary)=boundary { let (source,item)=sql_position(boundary)?; query=query.bind(source).bind(item); }
            let rows = query.fetch_all(&mut *tx).await?;
            let mut chunks = Vec::new();
            let mut chunks_json_bytes = 0;
            let mut bytes = 0;
            let mut projected = 0;
            let mut left = None;
            let mut right = None;
            'rows: for locator in rows {
                if chunks.len() == max_chunks || bytes == max_bytes || (projected >= 1024 * 1024 && left.is_some()) {
                    break;
                }
                let locator = row_position(locator.0, locator.1)?;
                // Materialize only actually visited rows, never a long message's
                // following rows. One oversized first row still progresses.
                projected += materialize(&mut tx, &session, locator, view, through_sql).await?;
                let has_thinking: bool = include_thinking && sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM transcript_text WHERE session_id = ?1 AND source_sequence = ?2 AND item_index = ?3 AND field = 'thinking')"
                ).bind(&session).bind(sql_position(locator)?.0).bind(sql_position(locator)?.1).fetch_one(&mut *tx).await?;
                let mut fields = vec![HistoryField::Body];
                if include_thinking && has_thinking { fields.insert(0, HistoryField::Thinking); }
                if reverse { fields.reverse(); }
                for field in fields {
                    if chunks.len() == max_chunks || bytes == max_bytes { break 'rows; }
                    if target == Some((locator, HistoryField::Body)) && field == HistoryField::Thinking {
                        continue;
                    }
                    let row = header(&mut tx, &session, locator, field, view, through_sql).await?;
                    let exact_target = target == Some((locator, field));
                    if let Some((cursor, _)) = &anchor && cursor.position == locator {
                        let before = field == HistoryField::Thinking && cursor.field == HistoryField::Body;
                        let after = field == HistoryField::Body && cursor.field == HistoryField::Thinking;
                        if (reverse && after) || (!reverse && before) { continue; }
                        if field == cursor.field && ((reverse && cursor.offset == 0) || (!reverse && cursor.offset == row.total)) { continue; }
                    }
                    let cursor_offset = anchor.as_ref().filter(|(cursor, _)| cursor.position == locator && cursor.field == field)
                        .map(|(cursor, _)| cursor.offset);
                    let (mut start, end) = if reverse {
                        let end = cursor_offset.unwrap_or(row.total);
                        (end.saturating_sub((max_bytes - bytes) as u64), end)
                    } else {
                        let start = cursor_offset.unwrap_or(0);
                        (start, row.total.min(start + (max_bytes - bytes) as u64))
                    };
                    let fragment: Vec<u8> = if row.total == 0 { Vec::new() } else { sqlx::query_scalar(
                        "SELECT substr(body, ?4, ?5) FROM transcript_text WHERE session_id = ?1 AND source_sequence = ?2 AND item_index = ?3 AND field = ?6"
                    ).bind(&session).bind(sql_position(locator)?.0).bind(sql_position(locator)?.1).bind(sql_number(start)? + 1)
                        .bind(sql_number(end - start)?).bind(field_name(field)).fetch_one(&mut *tx).await? };
                    let text = if reverse {
                        let skip = fragment.iter().take_while(|byte| **byte & 0xc0 == 0x80).count();
                        start += skip as u64;
                        std::str::from_utf8(&fragment[skip..]).map_err(|_| invalid("invalid history UTF-8"))?
                    } else {
                        match std::str::from_utf8(&fragment) {
                            Ok(text) => text,
                            Err(error) if error.error_len().is_none() =>
                                std::str::from_utf8(&fragment[..error.valid_up_to()]).map_err(|_| invalid("invalid history UTF-8"))?,
                            Err(_) => return Err(invalid("invalid history UTF-8")),
                        }
                    };
                    if text.is_empty() && row.total > 0 { break 'rows; }
                    let end = start + text.len() as u64;
                    let beginning = Cursor { field, position: locator, offset: start };
                    let ending = (Cursor { field, position: locator, offset: end }, row.total);
                    let candidate_left = if reverse { Some(beginning) } else { left.or(Some(beginning)) };
                    let candidate_right = if reverse { right.or(Some(ending)) } else { Some(ending) };
                    let emit = (!row.hidden || exact_target) && (exact_target || row.total != 0
                        || matches!(row.role.as_str(), "tool_call" | "tool_result")
                        || (view == HistoryView::Conversation && row.role == "context_compacted"));
                    if emit {
                    let role = match row.role.as_str() {
                        "user" => Role::User, "assistant" => Role::Assistant,
                        "tool_call" => Role::ToolCall, "tool_result" => Role::ToolResult,
                        "context_compacted" => Role::Compaction,
                        "turn_state" | "model_interruption" => Role::Failure,
                        _ => return Err(invalid("invalid history text role")),
                    };
                    let attachments = if start == 0 && role == Role::User {
                        let encoded: Option<String> = sqlx::query_scalar(
                            "SELECT json_extract(payload, '$.attachments') FROM transcript_rows WHERE session_id = ?1 AND source_sequence = ?2 AND item_index = ?3"
                        ).bind(&session).bind(sql_position(locator)?.0).bind(sql_position(locator)?.1).fetch_one(&mut *tx).await?;
                        encoded.map(|value| serde_json::from_str(&value)).transpose()?.unwrap_or_default()
                    } else { Vec::new() };
                    let tool = if matches!(role, Role::ToolCall | Role::ToolResult) {
                        let Some(tool) = tool_metadata(&mut tx, &session, locator, through_sql, max_json_bytes).await? else {
                            if left.is_none() { return Err(invalid("history fragment exceeds JSON window budget")); }
                            break 'rows;
                        };
                        Some(tool)
                    } else { None };
                    let timing = super::timing::read(&mut tx, &session, &row.turn, through_sql).await?;
                    let compaction = if role == Role::Compaction {
                        let duration: i64 = sqlx::query_scalar(
                            "SELECT json_extract(payload, '$.durationMs') FROM transcript_rows WHERE session_id = ?1 AND source_sequence = ?2 AND item_index = ?3"
                        ).bind(&session).bind(sql_position(locator)?.0).bind(sql_position(locator)?.1).fetch_one(&mut *tx).await?;
                        Some(maka_agent_ports::session::history::Compaction { duration_ms: sequence_number(duration)? })
                    } else { None };
                    let model = if role == Role::Assistant {
                        super::model::resolve(&mut tx, &session, locator, sql_number(through)?).await?
                    } else { None };
                    let chunk = Chunk { field, timing, compaction, origin: row.origin, model,
                        message_id: row.message, turn_id: row.turn, timestamp: row.timestamp, role,
                        position: locator, offset: start, total_bytes: row.total, text: text.to_owned(), attachments, tool,
                    };
                    chunks_json_bytes += serde_json::to_vec(&chunk)?.len();
                    chunks.push(chunk);
                    }
                    // Bound the complete canonical delivery, including attachment
                    // descriptors and JSON escaping. A rejected candidate does not
                    // advance the source boundary or silently discard any metadata.
                    let (before, after) = boundaries(&mut tx, &session, through_sql, include_thinking, candidate_left, candidate_right).await?;
                    // The empty envelope includes []; each chunk adds its exact
                    // encoded bytes, plus commas between elements. Cursor changes
                    // still count even when this boundary emits no chunk.
                    let envelope = Page::Ready { through, chunks: Vec::new(), before, after };
                    let fits = serde_json::to_vec(&envelope)?.len() + chunks_json_bytes
                        + chunks.len().saturating_sub(1) <= max_json_bytes;
                    if !fits {
                        if emit { chunks.pop(); }
                        if left.is_none() { return Err(invalid("history fragment exceeds JSON window budget")); }
                        break 'rows;
                    }
                    bytes += text.len();
                    left = candidate_left;
                    right = candidate_right;
                    if (!reverse && end < row.total) || (reverse && start > 0) { break 'rows; }
                }
            }
            if reverse { chunks.reverse(); }
            if left.is_none() {
                if let Some((cursor, row)) = anchor {
                    left = Some(cursor); right = Some((cursor, row.total));
                } else if matches!(position, Position::Event { .. }) {
                    let last: Option<(i64,i64)> = sqlx::query_as(
                        "SELECT source_sequence,item_index FROM transcript_rows WHERE session_id = ?1 AND source_sequence <= ?2 ORDER BY source_sequence DESC,item_index DESC LIMIT 1"
                    ).bind(&session).bind(through_sql).fetch_optional(&mut *tx).await?;
                    if let Some(locator) = last {
                        let locator = row_position(locator.0, locator.1)?;
                        materialize(&mut tx, &session, locator, view, through_sql).await?;
                        let row = header(&mut tx, &session, locator, HistoryField::Body, view, through_sql).await?;
                        let cursor = Cursor { field: HistoryField::Body, position: locator, offset: row.total };
                        left = Some(cursor); right = Some((cursor, row.total));
                    }
                }
            }
            let (before, after) = boundaries(&mut tx, &session, through_sql, include_thinking, left, right).await?;
            tx.commit().await?;
            Ok(Page::Ready { through, chunks, before, after })
        })).await
    }
}

async fn boundaries(
    tx: &mut SqliteConnection,
    session: &str,
    through: i64,
    include_thinking: bool,
    left: Option<Cursor>,
    right: Option<(Cursor, u64)>,
) -> Result<(Option<Cursor>, Option<Cursor>), StoreError> {
    let before = match left {
        Some(cursor)
            if cursor.offset > 0
                || neighbor(tx, session, through, cursor, include_thinking, true).await? =>
        {
            Some(cursor)
        }
        _ => None,
    };
    let after = match right {
        Some((cursor, total))
            if cursor.offset < total
                || neighbor(tx, session, through, cursor, include_thinking, false).await? =>
        {
            Some(cursor)
        }
        _ => None,
    };
    Ok((before, after))
}

struct Header {
    origin: Option<maka_agent_facts::message::MessageOrigin>,
    message: String,
    turn: String,
    timestamp: u64,
    role: String,
    hidden: bool,
    total: u64,
}
async fn header(
    tx: &mut SqliteConnection,
    session: &str,
    locator: RowPosition,
    field: HistoryField,
    view: HistoryView,
    through: i64,
) -> Result<Header, StoreError> {
    if view == HistoryView::Conversation
        && field == HistoryField::Body
        && let Some(row) = conversation_tool(tx, session, locator, through).await?
    {
        return Ok(row);
    }
    let row = sqlx::query(
        "SELECT source.message_id, source.turn_id, cached.timestamp, cached.role, length(cached.body),
            CASE WHEN cached.role = 'user' THEN source.payload -> '$.origin' END
            FROM transcript_rows source JOIN transcript_text cached USING(session_id, source_sequence, item_index)
            WHERE source.session_id = ?1 AND source.source_sequence = ?2 AND item_index = ?3 AND cached.field = ?4"
    ).bind(session).bind(sql_position(locator)?.0).bind(sql_position(locator)?.1).bind(field_name(field)).fetch_one(tx).await?;
    Ok(Header {
        origin: row
            .try_get::<Option<String>, _>(5)?
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        message: row.try_get(0)?,
        turn: row.try_get(1)?,
        timestamp: sequence_number(row.try_get(2)?)?,
        role: row.try_get(3)?,
        hidden: false,
        total: sequence_number(row.try_get(4)?)?,
    })
}
async fn conversation_tool(
    tx: &mut SqliteConnection,
    session: &str,
    locator: RowPosition,
    through: i64,
) -> Result<Option<Header>, StoreError> {
    let row = sqlx::query(
        "SELECT row.message_id, row.turn_id, json_extract(row.payload, '$.ts'),
            json_extract(row.payload, '$.type'), json_extract(row.payload, '$.origin'),
            COALESCE(json_extract(row.payload, '$.toolName'), json_extract(call.payload, '$.toolName')),
            COALESCE(json_extract(row.payload, '$.isError'), 0)
        FROM transcript_rows row LEFT JOIN transcript_rows call
            ON call.session_id = row.session_id AND call.turn_id = row.turn_id
            AND call.message_id = json_extract(row.payload, '$.toolUseId')
            AND json_extract(call.payload, '$.type') = 'tool_call'
        WHERE row.session_id = ?1 AND row.source_sequence = ?2 AND row.item_index = ?3 AND row.source_sequence <= ?4
            AND json_extract(row.payload, '$.type') IN ('tool_call', 'tool_result')"
    ).bind(session).bind(sql_position(locator)?.0).bind(sql_position(locator)?.1).bind(through).fetch_optional(tx).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let origin: Option<String> = row.try_get(4)?;
    let name: Option<String> = row.try_get(5)?;
    let error: bool = row.try_get(6)?;
    let internal = !error
        && (matches!(
            (origin.as_deref(), name.as_deref()),
            (Some("provider"), Some("wait" | "tool_search"))
                | (Some("code_mode"), Some("code_cell" | "tool_search"))
        ));
    Ok(Some(Header {
        origin: None,
        message: row.try_get(0)?,
        turn: row.try_get(1)?,
        timestamp: sequence_number(row.try_get(2)?)?,
        role: row.try_get(3)?,
        hidden: internal,
        total: 0,
    }))
}

async fn neighbor(
    tx: &mut SqliteConnection,
    session: &str,
    through: i64,
    cursor: Cursor,
    include_thinking: bool,
    before: bool,
) -> Result<bool, StoreError> {
    if include_thinking {
        if !before && cursor.field == HistoryField::Thinking {
            return Ok(true);
        }
        if before && cursor.field == HistoryField::Body {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM transcript_text WHERE session_id = ?1 AND source_sequence = ?2 AND item_index = ?3 AND field = 'thinking')"
            ).bind(session).bind(sql_position(cursor.position)?.0).bind(sql_position(cursor.position)?.1).fetch_one(&mut *tx).await?;
            if exists {
                return Ok(true);
            }
        }
    }
    let query = if before {
        "SELECT EXISTS(SELECT 1 FROM transcript_rows WHERE session_id = ?1 AND source_sequence <= ?2 AND (source_sequence,item_index) < (?3,?4))"
    } else {
        "SELECT EXISTS(SELECT 1 FROM transcript_rows WHERE session_id = ?1 AND source_sequence <= ?2 AND (source_sequence,item_index) > (?3,?4))"
    };
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(query))
        .bind(session)
        .bind(through)
        .bind(sql_position(cursor.position)?.0)
        .bind(sql_position(cursor.position)?.1)
        .fetch_one(tx)
        .await?)
}
async fn materialize(
    tx: &mut SqliteConnection,
    session: &str,
    locator: RowPosition,
    view: HistoryView,
    through: i64,
) -> Result<usize, StoreError> {
    if view == HistoryView::Conversation
        && conversation_tool(tx, session, locator, through)
            .await?
            .is_some()
    {
        return Ok(0);
    }
    let source: Option<(i64, bool)> = sqlx::query_as(
        "SELECT total_bytes, EXISTS(SELECT 1 FROM transcript_text cached WHERE cached.session_id = source.session_id AND cached.source_sequence = source.source_sequence AND cached.item_index = source.item_index AND cached.field = 'body')
            FROM transcript_rows source WHERE session_id = ?1 AND source_sequence = ?2 AND item_index = ?3"
    ).bind(session).bind(sql_position(locator)?.0).bind(sql_position(locator)?.1).fetch_optional(&mut *tx).await?;
    let Some((bytes, cached)) = source else {
        return Err(invalid("history row was not found"));
    };
    if cached {
        return Ok(0);
    }
    let query = format!(
        "INSERT INTO transcript_text
            SELECT session_id, source_sequence, item_index, 'body', json_extract(payload, '$.ts'), json_extract(payload, '$.type'),
                CAST({BODY} AS BLOB)
            FROM transcript_rows WHERE session_id = ?1 AND source_sequence = ?2 AND item_index = ?3
            UNION ALL
            SELECT session_id, source_sequence, item_index, 'thinking', json_extract(payload, '$.ts'), 'assistant',
                CAST(json_extract(payload, '$.thinking.text') AS BLOB)
            FROM transcript_rows WHERE session_id = ?1 AND source_sequence = ?2 AND item_index = ?3
                AND json_extract(payload, '$.type') = 'assistant' AND json_type(payload, '$.thinking.text') = 'text'"
    );
    sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
        .bind(session)
        .bind(sql_position(locator)?.0)
        .bind(sql_position(locator)?.1)
        .execute(tx)
        .await?;
    usize::try_from(sequence_number(bytes)?)
        .map_err(|_| invalid("history source exceeds address space"))
}
fn invalid(message: &str) -> StoreError {
    StoreError::InvalidTransition(message.into())
}

fn field_name(field: HistoryField) -> &'static str {
    match field {
        HistoryField::Body => "body",
        HistoryField::Thinking => "thinking",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn conversation_retains_trace_parents_and_hides_transport_helpers() {
        let mut db = SqliteConnection::connect(":memory:").await.unwrap();
        sqlx::query("CREATE TABLE transcript_rows (session_id TEXT, source_sequence INTEGER, item_index INTEGER, message_id TEXT, turn_id TEXT, payload BLOB)").execute(&mut db).await.unwrap();
        let rows = [
            json!({"type":"tool_call","origin":"provider","toolName":"exec"}),
            json!({"type":"tool_call","origin":"code_mode","toolName":"code_cell"}),
            json!({"type":"tool_call","origin":"imported","toolName":"exec"}),
            json!({"type":"tool_call","origin":"provider","toolName":"cua_repl"}),
            json!({"type":"tool_call","origin":"host_sdk","toolName":"cua_operation","parentToolCallId":"4"}),
            json!({"type":"tool_result","origin":"provider","toolUseId":"4","isError":false}),
            json!({"type":"tool_result","origin":"provider","toolUseId":"1","isError":true}),
        ];
        for (i, mut value) in rows.into_iter().enumerate() {
            value["ts"] = json!(1);
            sqlx::query("INSERT INTO transcript_rows VALUES ('a', ?, 0, ?, 'turn', ?)")
                .bind((i + 1) as i64)
                .bind((i + 1).to_string())
                .bind(serde_json::to_vec(&value).unwrap())
                .execute(&mut db)
                .await
                .unwrap();
        }
        for (locator, visible) in [
            (1, true),
            (2, false),
            (3, true),
            (4, true),
            (5, true),
            (6, true),
            (7, true),
        ] {
            let row = conversation_tool(
                &mut db,
                "a",
                RowPosition {
                    source_sequence: locator,
                    item_index: 0,
                },
                7,
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(!row.hidden, visible, "locator {locator}");
            assert_eq!(row.total, 0);
        }
        // A parent without a child at the requested fence remains visible.
        assert_eq!(
            conversation_tool(
                &mut db,
                "a",
                RowPosition {
                    source_sequence: 4,
                    item_index: 0
                },
                4
            )
            .await
            .unwrap()
            .unwrap()
            .role,
            "tool_call"
        );
    }
}
