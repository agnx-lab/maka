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

//! Worker callback lifecycle on the actual OwnedConnection runtime and lease.
use super::*;
use std::{
    fs::File,
    sync::{atomic::AtomicUsize, mpsc as blocking},
    time::Duration,
};

async fn owner(directory: &std::path::Path) -> Arc<OwnedConnection> {
    let lease = Arc::new(crate::root::FileLease::acquire(&directory.join("writer.lock")).unwrap());
    Arc::new(OwnedConnection::open(
        SqliteConnectOptions::new().filename(directory.join("callback.sqlite")).create_if_missing(true),
        ConnectionAuthority::Writer { lease, root: None },
        |connection| Box::pin(async move {
            sqlx::raw_sql("PRAGMA journal_mode=WAL; CREATE TABLE facts(value INTEGER); INSERT INTO facts VALUES(1);").execute(connection).await?;
            Ok(())
        }),
    ).await.unwrap())
}

async fn count(owner: &OwnedConnection) -> i64 {
    owner
        .run(|connection| {
            Box::pin(async move {
                Ok(sqlx::query_scalar("SELECT count(*) FROM facts")
                    .fetch_one(connection)
                    .await?)
            })
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn callback_yields_owner_read_admission_and_survives_external_waiter_drop() {
    let directory = tempfile::tempdir().unwrap();
    let owner = owner(directory.path()).await;
    let (entered, reached) = oneshot::channel();
    let (release, gate) = blocking::channel();
    let writer = owner.clone();
    let waiter = tokio::spawn(async move {
        writer
            .run(move |connection| {
                Box::pin(async move {
                    let mut tx = connection.begin().await?;
                    tx.run_on_worker(move |handle| -> Result<(), StoreError> {
                        // SAFETY: exclusive callback-local non-owning use, no retained handles.
                        let borrowed =
                            unsafe { rusqlite::Connection::from_handle(handle.as_ptr()) }?;
                        borrowed.execute("INSERT INTO facts VALUES(2)", [])?;
                        entered
                            .send(std::thread::current().name().unwrap_or("").to_owned())
                            .unwrap();
                        gate.recv_timeout(Duration::from_secs(10)).unwrap();
                        drop(borrowed);
                        Ok(())
                    })
                    .await??;
                    tx.commit().await?;
                    Ok(())
                })
            })
            .await
    });
    let thread = tokio::time::timeout(Duration::from_secs(5), reached)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(thread, "runtime-store-owner");
    assert!(!thread.is_empty());
    // This real independent read is admitted while the writer callback is gated.
    let read = tokio::time::timeout(
        Duration::from_secs(5),
        owner.read(|connection| {
            Box::pin(async move {
                Ok(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM facts")
                    .fetch_one(connection)
                    .await?)
            })
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(read, 1, "uncommitted callback writes remain invisible");
    assert!(crate::root::FileLease::acquire(&directory.path().join("writer.lock")).is_err());
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    release.send(()).unwrap();
    assert_eq!(count(&owner).await, 2, "accepted callback still commits");
    owner.shutdown().await.unwrap();
    drop(crate::root::FileLease::acquire(&directory.path().join("writer.lock")).unwrap());
}

#[tokio::test]
async fn callback_domain_error_uses_sqlx_rollback_before_next_job() {
    let directory = tempfile::tempdir().unwrap();
    let owner = owner(directory.path()).await;
    let result = owner
        .run::<()>(|connection| {
            Box::pin(async move {
                let mut tx = connection.begin().await?;
                tx.run_on_worker(move |handle| -> Result<(), StoreError> {
                    // SAFETY: exclusive synchronous borrow, all statements/wrapper scoped here.
                    let borrowed = unsafe { rusqlite::Connection::from_handle(handle.as_ptr()) }?;
                    borrowed.execute("INSERT INTO facts VALUES(2)", [])?;
                    Err(StoreError::InvalidTransition(
                        "callback fixture error".into(),
                    ))
                })
                .await??;
                tx.commit().await?;
                Ok(())
            })
        })
        .await;
    assert!(matches!(result, Err(StoreError::InvalidTransition(_))));
    assert_eq!(count(&owner).await, 1);
    owner.shutdown().await.unwrap();
}

#[tokio::test]
async fn dropped_internal_callback_future_and_transaction_queue_rollback_in_order() {
    let directory = tempfile::tempdir().unwrap();
    let owner = owner(directory.path()).await;
    let (entered, reached) = oneshot::channel();
    let (dropped, after_drop) = oneshot::channel();
    let (release, gate) = blocking::channel();
    let writer = owner.clone();
    let waiter = tokio::spawn(async move {
        writer
            .run(move |connection| {
                Box::pin(async move {
                    let mut tx = connection.begin().await?;
                    let mut callback =
                        Box::pin(tx.run_on_worker(move |handle| -> Result<(), StoreError> {
                            // SAFETY: only this worker callback can access the borrowed handle.
                            let borrowed =
                                unsafe { rusqlite::Connection::from_handle(handle.as_ptr()) }?;
                            borrowed.execute("INSERT INTO facts VALUES(2)", [])?;
                            entered.send(()).unwrap();
                            gate.recv_timeout(Duration::from_secs(10)).unwrap();
                            Ok(())
                        }));
                    assert!(futures_util::poll!(&mut callback).is_pending());
                    drop(callback);
                    drop(tx); // existing start_rollback command follows the accepted Call
                    dropped.send(()).unwrap();
                    Ok(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM facts")
                        .fetch_one(connection)
                        .await?)
                })
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), reached)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), after_drop)
        .await
        .unwrap()
        .unwrap();
    release.send(()).unwrap();
    assert_eq!(waiter.await.unwrap().unwrap(), 1);
    assert_eq!(count(&owner).await, 1);
    owner.shutdown().await.unwrap();
}

struct PanicOnDrop;
impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        panic!("callback capture destruction unwound");
    }
}

#[tokio::test]
async fn callback_body_and_capture_drop_panics_close_after_rollback_and_reader_drain() {
    for drop_panic in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let owner = owner(directory.path()).await;
        let (reader_entered, reader_reached) = oneshot::channel();
        let (reader_release, reader_gate) = oneshot::channel();
        let reader = owner.clone();
        let reading = tokio::spawn(async move {
            reader
                .read(move |connection| {
                    Box::pin(async move {
                        let tx = connection.begin().await?;
                        reader_entered.send(()).unwrap();
                        reader_gate.await.unwrap();
                        tx.commit().await?;
                        Ok(())
                    })
                })
                .await
        });
        reader_reached.await.unwrap();
        let (entered, reached) = oneshot::channel();
        let (release, gate) = blocking::channel();
        let writer = owner.clone();
        let captured = if drop_panic { Some(PanicOnDrop) } else { None };
        let waiter = tokio::spawn(async move {
            writer
                .run::<()>(move |connection| {
                    Box::pin(async move {
                        let mut tx = connection.begin().await?;
                        tx.run_on_worker(move |handle| -> Result<(), StoreError> {
                            let _captured = captured;
                            // SAFETY: callback-local non-owning wrapper drops during unwind.
                            let borrowed =
                                unsafe { rusqlite::Connection::from_handle(handle.as_ptr()) }?;
                            borrowed.execute("INSERT INTO facts VALUES(2)", [])?;
                            entered.send(()).unwrap();
                            gate.recv_timeout(Duration::from_secs(10)).unwrap();
                            if !drop_panic {
                                panic!("callback body unwound");
                            }
                            Ok(())
                        })
                        .await??;
                        tx.commit().await?;
                        Ok(())
                    })
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(5), reached)
            .await
            .unwrap()
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let queued = owner.run(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        });
        tokio::pin!(queued);
        assert!(futures_util::poll!(&mut queued).is_pending());
        release.send(()).unwrap();
        assert!(matches!(
            waiter.await.unwrap(),
            Err(StoreError::OperationUnknown)
        ));
        // Callback outcome was observed, but actual reader drain/close is still pending.
        let mut closed = owner.close_receipt();
        assert!(futures_util::poll!(&mut closed).is_pending());
        assert!(crate::root::FileLease::acquire(&directory.path().join("writer.lock")).is_err());
        reader_release.send(()).unwrap();
        reading.await.unwrap().unwrap();
        closed.await.unwrap();
        assert!(queued.await.is_err());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "unwound writer is never reused"
        );
        let db = rusqlite::Connection::open(directory.path().join("callback.sqlite")).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM facts", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(db);
        drop(crate::root::FileLease::acquire(&directory.path().join("writer.lock")).unwrap());
        owner.shutdown().await.unwrap();
    }
}

#[test]
fn dropped_caller_runtime_keeps_callback_commit_and_real_close_owned() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let owner = runtime.block_on(owner(directory.path()));
    let closed = owner.close_receipt();
    let (entered, reached) = blocking::channel();
    let (release, gate) = blocking::channel();
    runtime.spawn(async move {
        owner
            .run(move |connection| {
                Box::pin(async move {
                    let mut tx = connection.begin().await?;
                    tx.run_on_worker(move |handle| -> Result<(), StoreError> {
                        // SAFETY: callback owns the exclusive synchronous borrow only.
                        let borrowed =
                            unsafe { rusqlite::Connection::from_handle(handle.as_ptr()) }?;
                        borrowed.execute("INSERT INTO facts VALUES(2)", [])?;
                        entered.send(()).unwrap();
                        gate.recv_timeout(Duration::from_secs(10)).unwrap();
                        Ok(())
                    })
                    .await??;
                    tx.commit().await?;
                    Ok(())
                })
            })
            .await
            .unwrap();
    });
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while reached.try_recv().is_err() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    });
    drop(runtime);
    assert!(crate::root::FileLease::acquire(&directory.path().join("writer.lock")).is_err());
    release.send(()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        closed.await.unwrap();
    });
    drop(crate::root::FileLease::acquire(&directory.path().join("writer.lock")).unwrap());
    let db = rusqlite::Connection::open(directory.path().join("callback.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT sum(value) FROM facts", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
}

struct ReaderCloseGate {
    entered: Option<oneshot::Sender<()>>,
    release: blocking::Receiver<()>,
}

impl Drop for ReaderCloseGate {
    fn drop(&mut self) {
        // This destructor runs when SQLite destroys the actual reader hook.
        let _ = self.entered.take().unwrap().send(());
        self.release.recv().unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn writer_panic_rejects_accepted_reader_writer_work_before_join() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("callback.sqlite");
    let lock = directory.path().join("writer.lock");
    let owner = owner(directory.path()).await;
    let initial_capacity = owner.jobs.capacity();
    let contender = File::open(&lock).unwrap();
    let (writer_entered, writer_active) = oneshot::channel();
    let (panic_now, panic_gate) = oneshot::channel();
    let writer_owner = owner.clone();
    let panicking_writer = tokio::spawn(async move {
        writer_owner
            .run::<()>(move |_| {
                Box::pin(async move {
                    writer_entered.send(()).unwrap();
                    panic_gate.await.unwrap();
                    panic!("injected original accepted writer operation panic");
                })
            })
            .await
    });
    writer_active.await.unwrap();

    let (accepted, reader_queued) = oneshot::channel();
    let (close_entered, closing) = oneshot::channel();
    let (close_release, close_gate) = blocking::channel();
    let executed = Arc::new(AtomicUsize::new(0));
    let writer_observed = Arc::new(AtomicUsize::new(0));
    let read_owner = owner.clone();
    let writer = owner.writer();
    let reader_observed = writer_observed.clone();
    let reader = tokio::spawn(async move {
        read_owner
            .read::<()>(move |connection| {
                Box::pin(async move {
                    let original: i64 = sqlx::query_scalar("SELECT value FROM facts")
                        .fetch_one(&mut *connection)
                        .await?;
                    assert_eq!(original, 1);
                    let close = ReaderCloseGate {
                        entered: Some(close_entered),
                        release: close_gate,
                    };
                    connection
                        .lock_handle()
                        .await?
                        .set_progress_handler(1, move || {
                            let _retained = &close;
                            true
                        });
                    let mut pending = Box::pin(writer.run::<()>(move |connection| {
                        Box::pin(async move {
                            reader_observed.fetch_add(1, Ordering::SeqCst);
                            let value: i64 = sqlx::query_scalar("SELECT value FROM facts")
                                .fetch_one(connection)
                                .await?;
                            assert_eq!(value, 1);
                            Ok(())
                        })
                    }));
                    // No other writer can enqueue: the original writer is held
                    // above and this test owns all ingress. One poll must accept
                    // this command then wait for its reply, not its send capacity.
                    assert_eq!(writer.jobs.capacity(), initial_capacity);
                    assert!(futures_util::poll!(pending.as_mut()).is_pending());
                    assert_eq!(writer.jobs.capacity(), initial_capacity - 1);
                    accepted.send(()).unwrap();
                    pending.await
                })
            })
            .await
    });
    reader_queued.await.unwrap();
    // A separate accepted canonical-write stand-in must also be discarded,
    // rather than executed merely to let the reader finish during shutdown.
    let queued_executed = executed.clone();
    let mut canonical = Box::pin(owner.run::<()>(move |connection| {
        Box::pin(async move {
            queued_executed.fetch_add(1, Ordering::SeqCst);
            sqlx::query("INSERT INTO facts VALUES(2)")
                .execute(connection)
                .await?;
            Ok(())
        })
    }));
    assert!(futures_util::poll!(canonical.as_mut()).is_pending());
    assert_eq!(owner.jobs.capacity(), initial_capacity - 2);
    assert_eq!(writer_observed.load(Ordering::SeqCst), 0);
    assert_eq!(executed.load(Ordering::SeqCst), 0);
    assert!(matches!(
        contender.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    // An external sender can reserve capacity before closure and publish later.
    let reserved = owner.jobs.clone().reserve_owned().await.unwrap();
    panic_now.send(()).unwrap();
    assert!(matches!(
        panicking_writer.await.unwrap(),
        Err(StoreError::OperationUnknown)
    ));
    owner.jobs.closed().await;
    assert!(owner.close_receipt().now_or_never().is_none());
    assert!(matches!(
        contender.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    let (reply, rejected) = oneshot::channel::<()>();
    let late_executed = executed.clone();
    let sender = reserved.send(Command::Run(Box::new(move |_, _| {
        Box::pin(async move {
            late_executed.fetch_add(1, Ordering::SeqCst);
            reply.send(()).unwrap();
        })
    })));
    assert!(rejected.await.is_err());
    assert_eq!(executed.load(Ordering::SeqCst), 0);

    let closing_owner = owner.clone();
    let shutdown = tokio::spawn(async move { closing_owner.shutdown().await });
    // Closing a reader must reject its queued writer dependency first.
    closing.await.unwrap();
    assert_eq!(executed.load(Ordering::SeqCst), 0);
    assert!(!shutdown.is_finished());
    assert!(owner.close_receipt().now_or_never().is_none());
    assert!(matches!(
        contender.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    close_release.send(()).unwrap();
    assert!(matches!(
        reader.await.unwrap(),
        Err(StoreError::OperationUnknown)
    ));
    assert!(matches!(canonical.await, Err(StoreError::OperationUnknown)));
    assert_eq!(writer_observed.load(Ordering::SeqCst), 0);
    shutdown.await.unwrap().unwrap();
    owner.close_receipt().await.unwrap();
    contender.try_lock().unwrap();
    contender.unlock().unwrap();

    let lease = Arc::new(crate::root::FileLease::acquire(&lock).unwrap());
    let reopened = OwnedConnection::open(
        SqliteConnectOptions::new().filename(&database),
        ConnectionAuthority::Writer { lease, root: None },
        |_| Box::pin(async { Ok(()) }),
    )
    .await
    .unwrap();
    let facts: Vec<i64> = reopened
        .run(|connection| {
            Box::pin(async move {
                Ok(sqlx::query_scalar("SELECT value FROM facts ORDER BY rowid")
                    .fetch_all(connection)
                    .await?)
            })
        })
        .await
        .unwrap();
    assert_eq!(facts, [1]);
    assert_eq!(executed.load(Ordering::SeqCst), 0);
    reopened.shutdown().await.unwrap();
    drop(sender);
}

#[tokio::test]
async fn worker_queries_reuse_async_statement_and_preserve_owned_rows_and_bindings() {
    use futures_util::TryStreamExt;
    use sqlx::Row;
    const SQL: &str = "SELECT ?1 AS number, ?2 AS text, ?3 AS bytes, ?4 AS nullable";
    let directory = tempfile::tempdir().unwrap();
    let owner = owner(directory.path()).await;
    let row = owner
        .run(|connection| {
            Box::pin(async move {
                let warm = sqlx::query(SQL)
                    .bind(7i64)
                    .bind("warm")
                    .bind(vec![1u8])
                    .bind(Some("present"))
                    .fetch_one(&mut *connection)
                    .await?;
                assert_eq!(warm.try_get::<i64, _>(0)?, 7);
                worker_query(connection, |worker| {
                    Box::pin(async move {
                        // Observe an existing prepared statement only while the callback
                        // owns its original connection; no handle or pointer escapes.
                        fn statement(
                            worker: &mut sqlx::sqlite::SqliteWorkerConnection<'_>,
                        ) -> usize {
                            let db = worker.as_raw_handle();
                            let mut found = None;
                            let mut current = std::ptr::null_mut();
                            // SAFETY: no SQLx stream is alive, connection remains exclusively
                            // held, and SQLite-owned SQL bytes are used before advancing.
                            unsafe {
                                loop {
                                    current =
                                        rusqlite::ffi::sqlite3_next_stmt(db.as_ptr(), current);
                                    if current.is_null() {
                                        break;
                                    }
                                    let sql = rusqlite::ffi::sqlite3_sql(current);
                                    if !sql.is_null()
                                        && std::ffi::CStr::from_ptr(sql).to_bytes()
                                            == SQL.as_bytes()
                                    {
                                        assert!(found.replace(current as usize).is_none());
                                    }
                                }
                            }
                            found.expect("ordinary SQLx query populated the original cache")
                        }
                        let before = statement(worker);
                        let row = sqlx::query(SQL)
                            .bind(-13i64)
                            .bind("中🙂")
                            .bind(vec![0u8, 255, 9])
                            .bind(None::<String>)
                            .fetch_one(&mut *worker)
                            .await?;
                        assert_eq!(
                            statement(worker),
                            before,
                            "worker query reused the existing native statement"
                        );
                        let mut stream = sqlx::query(
                            "SELECT value FROM facts UNION ALL SELECT 2 UNION ALL SELECT 3",
                        )
                        .fetch(&mut *worker);
                        assert_eq!(stream.try_next().await?.unwrap().try_get::<i64, _>(0)?, 1);
                        drop(stream); // partially consumed cursor must reset before reuse
                        let values: Vec<i64> = sqlx::query_scalar(
                            "SELECT value FROM facts UNION ALL SELECT 2 UNION ALL SELECT 3",
                        )
                        .fetch_all(&mut *worker)
                        .await?;
                        assert_eq!(values, [1, 2, 3]);
                        assert_eq!(
                            sqlx::query_scalar::<_, i64>("SELECT value FROM facts WHERE value=99")
                                .fetch_optional(&mut *worker)
                                .await?,
                            None
                        );
                        assert!(matches!(
                            sqlx::query_scalar::<_, i64>("SELECT value FROM facts WHERE value=99")
                                .fetch_one(&mut *worker)
                                .await,
                            Err(sqlx::Error::RowNotFound)
                        ));
                        Ok(row)
                    })
                })
                .await
            })
        })
        .await
        .unwrap();
    owner.shutdown().await.unwrap();
    // Returned values remain owned even after the original connection closes.
    assert_eq!(row.get::<i64, _>("number"), -13);
    assert_eq!(row.get::<String, _>("text"), "中🙂");
    assert_eq!(row.get::<Vec<u8>, _>("bytes"), [0, 255, 9]);
    assert_eq!(row.get::<Option<String>, _>("nullable"), None);
}

#[tokio::test]
async fn worker_query_error_ends_stream_and_original_transaction_rolls_back() {
    use futures_util::StreamExt;
    use sqlx::Executor;
    let directory = tempfile::tempdir().unwrap();
    let owner = owner(directory.path()).await;
    let result = owner.run::<()>(|connection| Box::pin(async move {
        let mut tx = connection.begin().await?;
        let result = worker_query(&mut tx, |worker| Box::pin(async move {
            let mut stream = (&mut *worker).fetch_many(sqlx::raw_sql(
                "INSERT INTO facts VALUES(2); SELECT missing FROM facts; INSERT INTO facts VALUES(3);"));
            assert_eq!(stream.next().await.unwrap()?.left().unwrap().rows_affected(), 1);
            let error = stream.next().await.unwrap().unwrap_err();
            assert!(matches!(error, sqlx::Error::Database(_)));
            assert!(stream.next().await.is_none(), "error must terminate the original batch");
            drop(stream);
            assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM facts")
                .fetch_one(&mut *worker).await?, 2, "no statement after the error may execute");
            Err(StoreError::from(error))
        })).await;
        tx.rollback().await?;
        result
    })).await;
    assert!(result.is_err());
    assert_eq!(count(&owner).await, 1);
    owner.shutdown().await.unwrap();
}

#[tokio::test]
async fn accepted_worker_queries_commit_after_waiter_cancellation_without_blocking_readers() {
    let directory = tempfile::tempdir().unwrap();
    let owner = owner(directory.path()).await;
    let (entered, reached) = oneshot::channel();
    let (release, gate) = blocking::channel();
    let writer = owner.clone();
    let waiter = tokio::spawn(async move {
        writer
            .run(move |connection| {
                Box::pin(async move {
                    let mut tx = connection.begin().await?;
                    worker_query(&mut tx, move |worker| {
                        Box::pin(async move {
                            sqlx::query("INSERT INTO facts VALUES(2)")
                                .execute(&mut *worker)
                                .await?;
                            assert_ne!(std::thread::current().name(), Some("runtime-store-owner"));
                            entered.send(()).unwrap();
                            gate.recv_timeout(Duration::from_secs(10)).unwrap();
                            Ok(())
                        })
                    })
                    .await?;
                    tx.commit().await?;
                    Ok(())
                })
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), reached)
        .await
        .unwrap()
        .unwrap();
    let visible = tokio::time::timeout(
        Duration::from_secs(5),
        owner.read(|connection| {
            Box::pin(async move {
                Ok(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM facts")
                    .fetch_one(connection)
                    .await?)
            })
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(visible, 1);
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    release.send(()).unwrap();
    assert_eq!(count(&owner).await, 2);
    owner.shutdown().await.unwrap();
    drop(crate::root::FileLease::acquire(&directory.path().join("writer.lock")).unwrap());
}

#[tokio::test]
async fn accidental_async_wait_in_worker_queries_closes_and_rolls_back() {
    let directory = tempfile::tempdir().unwrap();
    let owner = owner(directory.path()).await;
    let result = owner
        .run::<()>(|connection| {
            Box::pin(async move {
                let mut tx = connection.begin().await?;
                worker_query(&mut tx, |worker| {
                    Box::pin(async move {
                        sqlx::query("INSERT INTO facts VALUES(2)")
                            .execute(worker)
                            .await?;
                        futures_util::future::pending::<()>().await;
                        Ok(())
                    })
                })
                .await?;
                tx.commit().await?;
                Ok(())
            })
        })
        .await;
    assert!(matches!(result, Err(StoreError::OperationUnknown)));
    owner.close_receipt().await.unwrap();
    assert!(owner.run(|_| Box::pin(async { Ok(()) })).await.is_err());
    let db = rusqlite::Connection::open(directory.path().join("callback.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM facts", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(db);
    drop(crate::root::FileLease::acquire(&directory.path().join("writer.lock")).unwrap());
    owner.shutdown().await.unwrap();
}
