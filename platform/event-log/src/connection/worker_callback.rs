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
