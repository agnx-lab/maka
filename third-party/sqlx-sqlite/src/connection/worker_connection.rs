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

use super::{execute, ConnectionState};
use crate::{Sqlite, SqliteQueryResult, SqliteRow, SqliteStatement, SqliteTypeInfo};
use futures_core::{future::BoxFuture, stream::BoxStream};
use futures_util::{stream, FutureExt, StreamExt};
use libsqlite3_sys::sqlite3;
use sqlx_core::{
    error::Error,
    executor::{Execute, Executor},
    sql_str::SqlStr,
    Either,
};
use std::{fmt, future, ptr::NonNull};

/// Exclusive, callback-local access to an existing SQLite worker connection.
///
/// Queries use the same execution engine and bounded prepared statement cache as
/// ordinary SQLx queries. Their futures and streams are immediately ready: they
/// perform SQLite work on the calling worker and never enqueue another command.
/// This is not a separate connection or transaction owner. A callback must not
/// control transactions or wait for other work on this connection.
///
/// Instances and borrowed streams cannot outlive the worker callback. Owned rows
/// have the same lifetime guarantees as rows returned by ordinary SQLx queries.
pub struct SqliteWorkerConnection<'a> {
    pub(super) conn: &'a mut ConnectionState,
}

impl fmt::Debug for SqliteWorkerConnection<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SqliteWorkerConnection").finish_non_exhaustive()
    }
}

impl SqliteWorkerConnection<'_> {
    /// Borrow the original connection handle exclusively for this callback.
    ///
    /// The same unsafe obligations as `LockedSqliteHandle::as_raw_handle` apply:
    /// no handle-backed values may escape, no closing or transaction control,
    /// and no invalidation of SQLx-owned statements or hooks. Temporary raw
    /// statements/wrappers must be dropped before using the SQLx executor again.
    pub fn as_raw_handle(&mut self) -> NonNull<sqlite3> {
        self.conn.handle.as_non_null_ptr()
    }
}

impl<'c> Executor<'c> for &'c mut SqliteWorkerConnection<'_> {
    type Database = Sqlite;

    fn fetch_many<'e, 'q, E>(
        self,
        mut query: E,
    ) -> BoxStream<'e, Result<Either<SqliteQueryResult, SqliteRow>, Error>>
    where
        'c: 'e,
        E: Execute<'q, Sqlite> + 'q,
        'q: 'e,
    {
        let arguments = match query.take_arguments().map_err(Error::Encode) {
            Ok(arguments) => arguments,
            Err(error) => return stream::once(future::ready(Err(error))).boxed(),
        };
        let persistent = query.persistent() && arguments.is_some();
        match execute::iter(self.conn, query.sql(), arguments, persistent) {
            Ok(mut iter) => {
                let mut finished = false;
                stream::iter(std::iter::from_fn(move || {
                    if finished { return None; }
                    let result = iter.next();
                    finished = result.as_ref().is_none_or(Result::is_err);
                    result
                })).boxed()
            },
            Err(error) => stream::once(future::ready(Err(error))).boxed(),
        }
    }

    fn fetch_optional<'e, 'q, E>(
        self,
        mut query: E,
    ) -> BoxFuture<'e, Result<Option<SqliteRow>, Error>>
    where
        'c: 'e,
        E: Execute<'q, Sqlite> + 'q,
        'q: 'e,
    {
        Box::pin(async move {
            let arguments = query.take_arguments().map_err(Error::Encode)?;
            let persistent = query.persistent() && arguments.is_some();
            let iter = execute::iter(self.conn, query.sql(), arguments, persistent)?;
            for result in iter {
                if let Either::Right(row) = result? {
                    return Ok(Some(row));
                }
            }
            Ok(None)
        })
    }

    fn prepare_with<'e>(
        self,
        sql: SqlStr,
        _parameters: &[SqliteTypeInfo],
    ) -> BoxFuture<'e, Result<SqliteStatement, Error>>
    where
        'c: 'e,
    {
        future::ready(super::worker::prepare(self.conn, sql)).boxed()
    }

    #[doc(hidden)]
    #[cfg(feature = "offline")]
    fn describe<'e>(
        self,
        sql: SqlStr,
    ) -> BoxFuture<'e, Result<sqlx_core::describe::Describe<Sqlite>, Error>>
    where
        'c: 'e,
    {
        future::ready(super::describe::describe(self.conn, sql)).boxed()
    }
}
