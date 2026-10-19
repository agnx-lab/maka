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

use std::{
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

mod authority;
#[cfg(test)]
mod cancellation;
#[cfg(test)]
mod worker_callback;
pub use authority::ConnectionAuthority;

use futures_util::{FutureExt, future::Shared};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use tokio::sync::{Semaphore, mpsc, oneshot, watch};

use crate::StoreError;
use tokio_util::sync::CancellationToken;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

type Job = Box<
    dyn for<'c> FnOnce(&'c mut SqliteConnection, Result<(), StoreError>) -> BoxFuture<'c, ()>
        + Send,
>;
// Returns whether the connection remains reusable after this accepted job.
type ReadJob = Box<
    dyn for<'c> FnOnce(Result<&'c mut SqliteConnection, StoreError>) -> BoxFuture<'c, bool> + Send,
>;
enum Command {
    Run(Job),
    Shutdown,
}

/// Ingress is bounded; accepted work and the database lease belong to a dedicated
/// thread, independent of the caller's executor. Dropping ingress drains work.
pub struct OwnedConnection {
    failures: Arc<AtomicU64>,
    jobs: mpsc::Sender<Command>,
    read_jobs: mpsc::Sender<ReadJob>,
    read_stop: CancellationToken,
    closed: Shared<BoxFuture<'static, Result<(), String>>>,
    readers: Arc<Semaphore>,
    changes: watch::Receiver<()>,
}

/// Short writer observations can be sent from a reader without retaining a
/// second database owner or its reader/shutdown authority.
#[derive(Clone)]
pub(crate) struct WriterIngress {
    jobs: mpsc::Sender<Command>,
    failures: Arc<AtomicU64>,
}

impl WriterIngress {
    pub(crate) fn failures(&self) -> u64 {
        self.failures.load(Ordering::Acquire)
    }

    pub(crate) async fn run<T: Send + 'static>(
        &self,
        operation: impl for<'c> FnOnce(&'c mut SqliteConnection) -> BoxFuture<'c, Result<T, StoreError>>
        + Send
        + 'static,
    ) -> Result<T, StoreError> {
        let (reply, result) = oneshot::channel();
        let failures = self.failures.clone();
        self.jobs
            .send(Command::Run(Box::new(move |connection, valid| {
                Box::pin(async move {
                    let result = match valid {
                        Ok(()) => operation(connection).await,
                        Err(error) => Err(error),
                    };
                    if result.is_err() {
                        failures.fetch_add(1, Ordering::AcqRel);
                    }
                    let _ = reply.send(result);
                })
            })))
            .await
            .map_err(|_| StoreError::ConnectionClosed)?;
        result.await.map_err(|_| StoreError::OperationUnknown)?
    }
}

impl OwnedConnection {
    pub(crate) fn writer(&self) -> WriterIngress {
        WriterIngress {
            jobs: self.jobs.clone(),
            failures: self.failures.clone(),
        }
    }

    /// Disposable write hints. Re-read canonical state after each wakeup;
    /// rolled-back work may also notify. Only the original worker owns ingress.
    pub fn subscribe_changes(&self) -> watch::Receiver<()> {
        self.changes.clone()
    }

    /// Observe the original worker's actual terminal result without closing ingress.
    pub(crate) fn close_receipt(&self) -> Shared<BoxFuture<'static, Result<(), String>>> {
        self.closed.clone()
    }

    pub async fn open(
        options: SqliteConnectOptions,
        authority: ConnectionAuthority,
        initialize: for<'c> fn(&'c mut SqliteConnection) -> BoxFuture<'c, Result<(), StoreError>>,
    ) -> Result<Self, StoreError> {
        let (jobs, mut receiver) = mpsc::channel::<Command>(32);
        let (read_jobs, read_receiver) = mpsc::channel::<ReadJob>(2);
        let read_stop = CancellationToken::new();
        let reader_stop = read_stop.clone();
        let authority = Arc::new(authority);
        let (ready, startup) = oneshot::channel();
        let (completion, closed) = oneshot::channel();
        let (changed, changes) = watch::channel(());
        std::thread::Builder::new()
            .name("runtime-store-owner".into())
            .spawn(move || {
                let mut queued_drop_panicked = false;
                let mut result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?;
                    let validate = || authority.validate();
                    validate()?;
                    let mut connection = runtime.block_on(connect_owned(&options))?;
                    let read_options = options.clone().create_if_missing(false).read_only(true);
                    let reader_owner = runtime.spawn(run_readers(
                        read_receiver,
                        reader_stop.clone(),
                        read_options,
                        authority.clone(),
                    ));
                    let mut last_changes = 0_i64;
                    let initialized = std::panic::catch_unwind(AssertUnwindSafe(|| {
                        runtime.block_on(async {
                            initialize(&mut connection).await?;
                            last_changes = sqlx::query_scalar("SELECT total_changes()")
                                .fetch_one(&mut connection)
                                .await?;
                            validate()
                        })
                    }))
                    .unwrap_or(Err(StoreError::OperationUnknown));
                    let success = initialized.is_ok();
                    // If open's waiter disappeared, close without accepting work.
                    if ready.send(initialized).is_ok() && success {
                        while let Some(command) = runtime.block_on(receiver.recv()) {
                            let job = match command {
                                Command::Run(job) => job,
                                Command::Shutdown => {
                                    // Reject new sends, then drain every accepted job.
                                    receiver.close();
                                    continue;
                                }
                            };
                            let executed = std::panic::catch_unwind(AssertUnwindSafe(|| {
                                runtime.block_on(job(&mut connection, validate()))
                            }));
                            if executed.is_err() {
                                // An unwound connection cannot accept further jobs.
                                break;
                            }
                            match runtime.block_on(
                                sqlx::query_scalar::<_, i64>("SELECT total_changes()")
                                    .fetch_one(&mut connection),
                            ) {
                                Ok(total) if total == last_changes => {}
                                Ok(total) => {
                                    last_changes = total;
                                    changed.send_replace(());
                                }
                                // Preserve the job's result and real close path. A
                                // subscriber's canonical read will report any failure.
                                Err(_) => {
                                    changed.send_replace(());
                                }
                            }
                        }
                    }
                    // Readers can await writer replies. Close admission, then reject
                    // queued work, including sends holding an earlier capacity permit,
                    // before joining readers. None also accounts for those permits.
                    receiver.close();
                    while let Some(command) = runtime.block_on(receiver.recv()) {
                        queued_drop_panicked |=
                            std::panic::catch_unwind(AssertUnwindSafe(|| drop(command))).is_err();
                    }
                    // Close read ingress independently, then drain every accepted read
                    // before releasing the writer's original authority.
                    reader_stop.cancel();
                    let reader_failure = runtime
                        .block_on(reader_owner)
                        .map_err(|error| error.to_string())
                        .and_then(|closed| closed.map_err(|error| error.to_string()))
                        .err();
                    // Await real close before releasing authority, even after failure.
                    runtime
                        .block_on(connection.close())
                        .map_err(|error| StoreError::CloseFailed(error.to_string()))?;
                    match reader_failure {
                        Some(error) => Err(StoreError::CloseFailed(error)),
                        None => Ok(()),
                    }
                }))
                .unwrap_or_else(|_| {
                    Err(StoreError::StartupCleanupFailed {
                        cause: Box::new(StoreError::Initialization),
                        cleanup: Box::new(StoreError::OperationUnknown),
                    })
                });
                // Unexecuted accepted jobs still own their original payloads.
                // Their destructors cannot unwind past this authority boundary.
                if std::panic::catch_unwind(AssertUnwindSafe(|| drop(receiver))).is_err()
                    || queued_drop_panicked
                {
                    result = Err(StoreError::CloseFailed(format!(
                        "queued connection work destruction panicked; prior close: {result:?}"
                    )));
                }
                let cleanup_failed = matches!(
                    result,
                    Err(StoreError::CloseFailed(_) | StoreError::StartupCleanupFailed { .. })
                );
                if !cleanup_failed {
                    drop(authority);
                }
                let _ = completion.send(result);
                // A failed real close is not permission to release the original
                // authority. Keep its actual worker alive; no replacement owner
                // or process exit can manufacture a cleanup acknowledgement.
                if cleanup_failed {
                    loop {
                        std::thread::park();
                    }
                }
            })?;
        match startup.await {
            Ok(Ok(())) => Ok(Self {
                failures: Arc::new(AtomicU64::new(0)),
                jobs,
                read_jobs,
                read_stop,
                closed: async move {
                    closed
                        .await
                        .map_err(|_| {
                            "connection owner exited without close acknowledgement".to_owned()
                        })?
                        .map_err(|error| error.to_string())
                }
                .boxed()
                .shared(),
                readers: Arc::new(Semaphore::new(2)),
                changes,
            }),
            Ok(Err(error)) => {
                drop(jobs);
                match closed.await {
                    Ok(Ok(())) => Err(error),
                    result => Err(StoreError::StartupCleanupFailed {
                        cause: Box::new(error),
                        cleanup: Box::new(match result {
                            Ok(Err(error)) => error,
                            Err(_) => StoreError::OperationUnknown,
                            Ok(Ok(())) => unreachable!(),
                        }),
                    }),
                }
            }
            Err(_) => {
                drop(jobs);
                match closed.await {
                    Ok(Ok(())) => Err(StoreError::Initialization),
                    Ok(Err(error @ StoreError::StartupCleanupFailed { .. })) => Err(error),
                    Ok(Err(error)) if !matches!(error, StoreError::CloseFailed(_)) => {
                        // Owned startup confirms any acquired connection has closed;
                        // a crashed worker or failed close takes the typed branch above.
                        Err(error)
                    }
                    result => Err(StoreError::StartupCleanupFailed {
                        cause: Box::new(StoreError::Initialization),
                        cleanup: Box::new(match result {
                            Ok(Err(error)) => error,
                            Err(_) => StoreError::OperationUnknown,
                            Ok(Ok(())) => unreachable!(),
                        }),
                    }),
                }
            }
        }
    }

    pub async fn run<T: Send + 'static>(
        &self,
        operation: impl for<'c> FnOnce(&'c mut SqliteConnection) -> BoxFuture<'c, Result<T, StoreError>>
        + Send
        + 'static,
    ) -> Result<T, StoreError> {
        self.writer().run(operation).await
    }

    pub async fn shutdown(&self) -> Result<(), StoreError> {
        self.readers.close();
        self.read_stop.cancel();
        // Closed ingress is also normal for repeated or concurrent shutdown.
        let _ = self.jobs.send(Command::Shutdown).await;
        self.closed.clone().await.map_err(StoreError::CloseFailed)
    }

    /// Bounded independent readers keep large snapshots off the serialized writer.
    /// Accepted reads drain on shutdown even if their caller or executor disappears.
    pub(crate) async fn read<T: Send + 'static>(
        &self,
        operation: impl for<'c> FnOnce(&'c mut SqliteConnection) -> BoxFuture<'c, Result<T, StoreError>>
        + Send
        + 'static,
    ) -> Result<T, StoreError> {
        self.read_job(operation, false).await
    }

    /// Pure database reads may stop when their observer disappears. The owner
    /// still awaits operation and transaction cleanup before reusing a reader.
    pub(crate) async fn read_interruptible<T: Send + 'static>(
        &self,
        operation: impl for<'c> FnOnce(&'c mut SqliteConnection) -> BoxFuture<'c, Result<T, StoreError>>
        + Send
        + 'static,
    ) -> Result<T, StoreError> {
        self.read_job(operation, true).await
    }

    async fn read_job<T: Send + 'static>(
        &self,
        operation: impl for<'c> FnOnce(&'c mut SqliteConnection) -> BoxFuture<'c, Result<T, StoreError>>
        + Send
        + 'static,
        interruptible: bool,
    ) -> Result<T, StoreError> {
        let permit = self
            .readers
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| StoreError::ConnectionClosed)?;
        let (mut reply, result) = oneshot::channel();
        self.read_jobs
            .send(Box::new(move |connection| {
                Box::pin(async move {
                    let _permit = permit;
                    let mut clean = true;
                    let executed = match connection {
                        Ok(connection) => {
                            let cancel = Arc::new(AtomicBool::new(false));
                            if interruptible {
                                match connection.lock_handle().await {
                                    Ok(mut locked) => {
                                        let cancel = cancel.clone();
                                        // Remember cancellation between statements, then consume
                                        // it once so transaction rollback can run normally.
                                        locked.set_progress_handler(1000, move || {
                                            !cancel.swap(false, Ordering::Relaxed)
                                        });
                                    }
                                    Err(error) => {
                                        let _ = reply.send(Err(error.into()));
                                        return false;
                                    }
                                }
                            }
                            let executed = {
                                let operation =
                                    AssertUnwindSafe(async { operation(&mut *connection).await })
                                        .catch_unwind();
                                tokio::pin!(operation);
                                tokio::select! {
                                    result = &mut operation => result,
                                    () = reply.closed(), if interruptible => {
                                        cancel.store(true, Ordering::Relaxed);
                                        operation.await
                                    }
                                }
                            };
                            if interruptible {
                                // The handle lock follows queued Transaction::drop rollbacks.
                                clean = match connection.lock_handle().await {
                                    Ok(mut locked) => {
                                        locked.remove_progress_handler();
                                        // SAFETY: the SQLx guard excludes worker FFI calls;
                                        // this only reads the live handle's transaction state.
                                        unsafe {
                                            rusqlite::ffi::sqlite3_get_autocommit(
                                                locked.as_raw_handle().as_ptr(),
                                            ) != 0
                                        }
                                    }
                                    Err(_) => false,
                                };
                            }
                            executed
                        }
                        Err(error) => Ok(Err(error)),
                    };
                    let reusable = clean
                        && !matches!(
                            &executed,
                            Err(_) | Ok(Err(StoreError::Sqlx(sqlx::Error::WorkerCrashed)))
                        );
                    let result = executed.unwrap_or(Err(StoreError::OperationUnknown));
                    let _ = reply.send(result);
                    reusable
                })
            }))
            .await
            .map_err(|_| StoreError::ConnectionClosed)?;
        result.await.map_err(|_| StoreError::OperationUnknown)?
    }

    pub async fn close(self) -> Result<(), StoreError> {
        self.shutdown().await
    }
}

// Both persistent readers remain on the original owner's runtime. The mutex
// protects only ingress removal; SQL and accepted jobs run independently.
async fn run_readers(
    receiver: mpsc::Receiver<ReadJob>,
    stop: CancellationToken,
    options: SqliteConnectOptions,
    authority: Arc<ConnectionAuthority>,
) -> Result<(), StoreError> {
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    let mut readers = tokio::task::JoinSet::new();
    for _ in 0..2 {
        readers.spawn(reader_worker(
            receiver.clone(),
            stop.clone(),
            options.clone(),
            authority.clone(),
        ));
    }
    let mut failure = None;
    while let Some(result) = readers.join_next().await {
        retain_reader_failure(result, &mut failure);
    }
    match failure {
        Some(error) => Err(StoreError::CloseFailed(error)),
        None => Ok(()),
    }
}

async fn reader_worker(
    receiver: Arc<tokio::sync::Mutex<mpsc::Receiver<ReadJob>>>,
    stop: CancellationToken,
    options: SqliteConnectOptions,
    authority: Arc<ConnectionAuthority>,
) -> Result<(), StoreError> {
    let mut connection = None;
    let mut failure = None;
    loop {
        let job = {
            let mut receiver = receiver.lock().await;
            tokio::select! {
                biased;
                () = stop.cancelled(), if !receiver.is_closed() => {
                    receiver.close();
                    receiver.recv().await
                },
                job = receiver.recv() => job,
            }
        };
        let Some(job) = job else { break };
        let mut valid = authority.validate();
        if valid.is_ok() && connection.is_none() {
            match connect_reader(&options).await {
                Ok(opened) => connection = Some(opened),
                Err(error) => {
                    if matches!(error, StoreError::StartupCleanupFailed { .. }) {
                        failure.get_or_insert(error.to_string());
                    }
                    valid = Err(error);
                }
            }
        }
        // Every job checks the current original authority, including after a
        // connection startup await. The immutable connection is never a grant.
        let ready = valid
            .and_then(|()| authority.validate())
            .map(|()| connection.as_mut().expect("validated reader connection"));
        let reusable = job(ready).await;
        if !reusable
            && let Some(connection) = connection.take()
            && let Err(error) = close_reader(connection).await
        {
            failure.get_or_insert(error.to_string());
        }
    }
    if let Some(connection) = connection
        && let Err(error) = close_reader(connection).await
    {
        failure.get_or_insert(error.to_string());
    }
    match failure {
        Some(error) => Err(StoreError::CloseFailed(error)),
        None => Ok(()),
    }
}

async fn close_reader(connection: SqliteConnection) -> Result<(), StoreError> {
    AssertUnwindSafe(connection.close())
        .catch_unwind()
        .await
        .map_err(|_| StoreError::CloseFailed("reader close panicked".into()))?
        .map_err(|error| StoreError::CloseFailed(error.to_string()))
}

async fn connect_reader(options: &SqliteConnectOptions) -> Result<SqliteConnection, StoreError> {
    let mut connection = connect_owned(options).await?;
    if let Err(error) = crate::sqlite_functions::register(&mut connection).await {
        return Err(close_startup(connection, error).await);
    }
    Ok(connection)
}

// These are the original owner's startup and reader cleanup boundaries, not
// another SQL owner. A returned ordinary startup error confirms actual close.
async fn close_startup(connection: SqliteConnection, cause: StoreError) -> StoreError {
    match connection.close().await {
        Ok(()) => cause,
        Err(cleanup) => StoreError::StartupCleanupFailed {
            cause: Box::new(cause),
            cleanup: Box::new(StoreError::CloseFailed(cleanup.to_string())),
        },
    }
}
async fn connect_owned(options: &SqliteConnectOptions) -> Result<SqliteConnection, StoreError> {
    let mut connection = match SqliteConnection::establish(options).await {
        Ok(connection) => connection,
        Err(sqlx::Error::WorkerCrashed) => {
            return Err(StoreError::StartupCleanupFailed {
                cause: Box::new(StoreError::Initialization),
                cleanup: Box::new(StoreError::OperationUnknown),
            });
        }
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = connection.configure(options).await {
        return Err(close_startup(connection, error.into()).await);
    }
    Ok(connection)
}
fn retain_reader_failure(
    result: Result<Result<(), StoreError>, tokio::task::JoinError>,
    failure: &mut Option<String>,
) {
    if let Err(error) = result
        .map_err(|error| error.to_string())
        .and_then(|closed| closed.map_err(|error| error.to_string()))
    {
        failure.get_or_insert(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{File, OpenOptions},
        sync::{Arc, mpsc as blocking},
        time::Duration,
    };

    #[tokio::test]
    async fn canonical_session_reads_bypass_a_blocked_writer() {
        let directory = tempfile::tempdir().unwrap();
        let namespaces = crate::root::RootNamespaces {
            ownership: directory.path().join("owners"),
            control: directory.path().join("control"),
        };
        let root = Arc::new(
            crate::root::RootOwner::create(&directory.path().join("root"), &namespaces).unwrap(),
        );
        let store = Arc::new(crate::EventLog::for_root(root).await.unwrap());
        store
            .create_session("foreground", "original", &serde_json::json!({}), 1)
            .await
            .unwrap();
        let (entered, reached) = oneshot::channel();
        let (release, gate) = oneshot::channel();
        let writer = store.clone();
        let blocked = tokio::spawn(async move {
            writer
                .connection
                .run(move |_| {
                    Box::pin(async move {
                        entered.send(()).unwrap();
                        gate.await.unwrap();
                        Ok(())
                    })
                })
                .await
        });
        reached.await.unwrap();
        // Awaited commits remain visible even while a later writer cannot finish.
        tokio::time::timeout(Duration::from_secs(5), async {
            assert!(
                store
                    .get_session::<serde_json::Value>("foreground")
                    .await
                    .unwrap()
                    .is_some()
            );
            assert!(
                store
                    .observe_session::<serde_json::Value>("foreground")
                    .await
                    .unwrap()
                    .is_some()
            );
            assert!(
                store
                    .latest_message_opening("foreground")
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(store.context_usage("foreground").await.unwrap().1.is_none());
        })
        .await
        .unwrap();
        release.send(()).unwrap();
        blocked.await.unwrap().unwrap();
        Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_waits_for_persistent_readers_actual_sqlite_close() {
        struct CloseGate {
            entered: blocking::Sender<()>,
            release: blocking::Receiver<()>,
        }
        impl Drop for CloseGate {
            fn drop(&mut self) {
                self.entered.send(()).unwrap();
                self.release.recv_timeout(Duration::from_secs(10)).unwrap();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let lock = directory.path().join("writer.lock");
        let lease = Arc::new(crate::root::FileLease::acquire(&lock).unwrap());
        let contender = File::open(&lock).unwrap();
        let owner = Arc::new(
            OwnedConnection::open(
                SqliteConnectOptions::new()
                    .filename(directory.path().join("close.sqlite"))
                    .create_if_missing(true),
                ConnectionAuthority::Writer { lease, root: None },
                |_| Box::pin(async { Ok(()) }),
            )
            .await
            .unwrap(),
        );
        let (entered, closing) = blocking::channel();
        let (release, gate) = blocking::channel();
        let reader = owner.clone();
        let caller = tokio::spawn(async move {
            reader
                .read(move |connection| {
                    Box::pin(async move {
                        let close = CloseGate {
                            entered,
                            release: gate,
                        };
                        connection
                            .lock_handle()
                            .await?
                            .set_progress_handler(1, move || {
                                let _retained = &close;
                                true
                            });
                        Ok(())
                    })
                })
                .await
        });
        caller.await.unwrap().unwrap();
        let closing_owner = owner.clone();
        let shutdown = tokio::spawn(async move { closing_owner.shutdown().await });
        tokio::time::timeout(Duration::from_secs(5), async {
            while closing.try_recv().is_err() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!shutdown.is_finished());
        assert!(owner.closed.clone().now_or_never().is_none());
        assert!(matches!(
            contender.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), shutdown)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        contender.try_lock().unwrap();
    }

    #[tokio::test]
    async fn persistent_readers_reuse_connections_and_see_later_commits_after_panics() {
        let directory = tempfile::tempdir().unwrap();
        let lease = Arc::new(
            crate::root::FileLease::acquire(&directory.path().join("writer.lock")).unwrap(),
        );
        let owner = Arc::new(OwnedConnection::open(
            SqliteConnectOptions::new().filename(directory.path().join("reuse.sqlite")).create_if_missing(true),
            ConnectionAuthority::Writer { lease, root: None },
            |connection| Box::pin(async move {
                sqlx::raw_sql("PRAGMA journal_mode=WAL; CREATE TABLE facts(value INTEGER); INSERT INTO facts VALUES(1);").execute(connection).await?;
                Ok(())
            }),
        ).await.unwrap());
        let entered = Arc::new(Semaphore::new(0));
        let release = Arc::new(Semaphore::new(0));
        let mut callers = Vec::new();
        for _ in 0..2 {
            let reader = owner.clone();
            let entered = entered.clone();
            let release = release.clone();
            callers.push(tokio::spawn(async move {
                reader
                    .read(move |connection| {
                        Box::pin(async move {
                            sqlx::query("PRAGMA cache_size=-1234")
                                .execute(connection)
                                .await?;
                            entered.add_permits(1);
                            release.acquire().await.unwrap().forget();
                            Ok(())
                        })
                    })
                    .await
            }));
        }
        tokio::time::timeout(Duration::from_secs(5), entered.acquire_many(2))
            .await
            .unwrap()
            .unwrap()
            .forget();
        release.add_permits(2);
        for caller in callers {
            caller.await.unwrap().unwrap();
        }
        owner
            .run(|connection| {
                Box::pin(async move {
                    sqlx::query("INSERT INTO facts VALUES(2)")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .await
            .unwrap();
        let (marker, count) = owner
            .read(|connection| {
                Box::pin(async move {
                    let marker: i64 = sqlx::query_scalar("PRAGMA cache_size")
                        .fetch_one(&mut *connection)
                        .await?;
                    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM facts")
                        .fetch_one(connection)
                        .await?;
                    Ok((marker, count))
                })
            })
            .await
            .unwrap();
        assert_eq!((marker, count), (-1234, 2));
        assert!(matches!(
            owner
                .read::<()>(|_| Box::pin(async { panic!("reader operation unwound") }))
                .await,
            Err(StoreError::OperationUnknown)
        ));
        assert_eq!(
            owner
                .read(|connection| Box::pin(async move {
                    Ok(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM facts")
                        .fetch_one(connection)
                        .await?)
                }))
                .await
                .unwrap(),
            2
        );
        owner.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn failed_pragma_configuration_closes_the_established_worker_before_releasing_authority()
    {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("configured.sqlite");
        let lock = directory.path().join("writer.lock");
        let lease = Arc::new(crate::root::FileLease::acquire(&lock).unwrap());
        let original = Arc::downgrade(&lease);
        let result = OwnedConnection::open(
            SqliteConnectOptions::new()
                .filename(&database)
                .create_if_missing(true)
                .pragma("maka_invalid", "("),
            ConnectionAuthority::Writer { lease, root: None },
            |_| Box::pin(async { panic!("configuration must fail before initialization") }),
        )
        .await;
        assert!(matches!(
            result,
            Err(StoreError::Sqlx(sqlx::Error::Database(_)))
        ));
        assert!(
            database.exists(),
            "the actual SQLite handle must have been established"
        );
        assert!(original.upgrade().is_none());
        drop(crate::root::FileLease::acquire(&lock).unwrap());
    }

    #[tokio::test]
    async fn actual_reader_close_failure_fences_the_original_writer_and_root_authority() {
        struct PanickingDrop;
        impl Drop for PanickingDrop {
            fn drop(&mut self) {
                panic!("actual SQLite callback destructor failed");
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        let namespaces = crate::root::RootNamespaces {
            ownership: directory.path().join("owners"),
            control: directory.path().join("control"),
        };
        let authority = Arc::new(crate::root::RootOwner::create(&root, &namespaces).unwrap());
        let original = Arc::downgrade(&authority);
        let owner = OwnedConnection::open(
            SqliteConnectOptions::new()
                .filename(root.join(crate::root::ROOT_DATABASE))
                .create_if_missing(true),
            ConnectionAuthority::Root(authority),
            |_| Box::pin(async { Ok(()) }),
        )
        .await
        .unwrap();
        let read = owner
            .read(|connection| {
                Box::pin(async move {
                    let actual = PanickingDrop;
                    connection
                        .lock_handle()
                        .await?
                        .set_progress_handler(1, move || {
                            let _retained = &actual;
                            true
                        });
                    Ok(())
                })
            })
            .await;
        assert!(
            read.is_ok(),
            "successful reads retain their connection until shutdown"
        );
        let (entered, reached) = oneshot::channel();
        let (release, gate) = oneshot::channel();
        owner
            .jobs
            .send(Command::Run(Box::new(move |_, _| {
                Box::pin(async move {
                    entered.send(()).unwrap();
                    gate.await.unwrap();
                    panic!("accepted writer operation unwound");
                })
            })))
            .await
            .unwrap();
        reached.await.unwrap();
        let queued_payload = PanickingDrop;
        owner
            .jobs
            .send(Command::Run(Box::new(move |_, _| {
                // The earlier operation unwinds before this queued job runs;
                // receiver destruction must settle its original payload here.
                drop(queued_payload);
                Box::pin(async {})
            })))
            .await
            .unwrap();
        release.send(()).unwrap();
        let closed = owner.shutdown().await;
        assert!(matches!(closed, Err(StoreError::CloseFailed(_))));
        assert!(
            closed
                .unwrap_err()
                .to_string()
                .contains("queued connection work destruction panicked")
        );
        drop(owner);
        assert!(
            original.upgrade().is_some(),
            "the same original SQL worker must retain authority"
        );
        assert_eq!(
            crate::root::RootOwner::open(&root, &namespaces)
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[tokio::test]
    async fn read_snapshot_does_not_block_writes_and_shutdown_drains_abandoned_readers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshot.sqlite");
        let lock = directory.path().join("writer.lock");
        let lease = Arc::new(crate::root::FileLease::acquire(&lock).unwrap());
        let contender = File::open(&lock).unwrap();
        let owner = Arc::new(OwnedConnection::open(
            SqliteConnectOptions::new().filename(&path).create_if_missing(true),
            ConnectionAuthority::Writer { lease, root: None },
            |connection| Box::pin(async move {
                sqlx::raw_sql("PRAGMA journal_mode=WAL; CREATE TABLE facts(value INTEGER); INSERT INTO facts VALUES(1);")
                    .execute(connection).await?;
                Ok(())
            }),
        ).await.unwrap());
        let (ready, opened) = oneshot::channel();
        let (release, gate) = oneshot::channel();
        let (finished, observed) = oneshot::channel();
        let reader = owner.clone();
        let caller = tokio::spawn(async move {
            reader
                .read(move |connection| {
                    Box::pin(async move {
                        let mut tx = connection.begin().await?;
                        let first: i64 = sqlx::query_scalar("SELECT SUM(value) FROM facts")
                            .fetch_one(&mut *tx)
                            .await?;
                        ready.send(()).unwrap();
                        gate.await.unwrap();
                        let second: i64 = sqlx::query_scalar("SELECT SUM(value) FROM facts")
                            .fetch_one(&mut *tx)
                            .await?;
                        assert!(
                            sqlx::query("INSERT INTO facts VALUES(99)")
                                .execute(&mut *tx)
                                .await
                                .is_err(),
                            "reader must not promote to a writer"
                        );
                        tx.rollback().await?;
                        finished.send((first, second)).unwrap();
                        Ok(())
                    })
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(5), opened)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(5),
            owner.run(|connection| {
                Box::pin(async move {
                    sqlx::query("INSERT INTO facts VALUES(2)")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            }),
        )
        .await
        .unwrap()
        .unwrap();
        caller.abort();
        let _ = caller.await;
        let closing = owner.clone();
        let shutdown = tokio::spawn(async move { closing.shutdown().await });
        assert!(
            tokio::time::timeout(Duration::from_millis(50), owner.closed.clone())
                .await
                .is_err()
        );
        assert!(matches!(
            contender.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        release.send(()).unwrap();
        assert_eq!(observed.await.unwrap(), (1, 1));
        tokio::time::timeout(Duration::from_secs(5), shutdown)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        contender.try_lock().unwrap();
        assert!(matches!(
            owner.read(|_| Box::pin(async { Ok(()) })).await,
            Err(StoreError::ConnectionClosed)
        ));
    }

    #[test]
    fn dropped_caller_runtime_keeps_authority_until_real_commit_and_close() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("owner.sqlite");
        let lock = directory.path().join("writer.lock");
        let lease = Arc::new(crate::root::FileLease::acquire(&lock).unwrap());
        let contender = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock)
            .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let owner = runtime
            .block_on(OwnedConnection::open(
                options,
                ConnectionAuthority::Writer { lease, root: None },
                |connection| {
                    Box::pin(async move {
                        sqlx::query("CREATE TABLE committed(value INTEGER)")
                            .execute(connection)
                            .await?;
                        Ok(())
                    })
                },
            ))
            .unwrap();
        let (entered, reached) = blocking::channel();
        let (release, gate) = blocking::channel();
        let close_ack = owner.closed.clone();
        runtime.spawn(async move {
            owner
                .run(move |connection| {
                    Box::pin(async move {
                        connection.lock_handle().await?.set_commit_hook(move || {
                            entered.send(()).unwrap();
                            gate.recv_timeout(Duration::from_secs(10)).unwrap();
                            true
                        });
                        sqlx::query("INSERT INTO committed VALUES (7)")
                            .execute(connection)
                            .await?;
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
        assert!(matches!(
            contender.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        release.send(()).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            close_ack.await.unwrap();
            contender.try_lock().unwrap();
            let mut connection =
                SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path))
                    .await
                    .unwrap();
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT value FROM committed")
                    .fetch_one(&mut connection)
                    .await
                    .unwrap(),
                7
            );
            connection.close().await.unwrap();
        });
    }

    #[tokio::test]
    async fn shutdown_is_idempotent_while_handles_remain_alive() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("writer.lock");
        let lease = Arc::new(crate::root::FileLease::acquire(&path).unwrap());
        let contender = File::open(&path).unwrap();
        let owner = Arc::new(
            OwnedConnection::open(
                SqliteConnectOptions::new().in_memory(true),
                ConnectionAuthority::Writer { lease, root: None },
                |_| Box::pin(async { Ok(()) }),
            )
            .await
            .unwrap(),
        );
        let retained = owner.clone();
        // Replacing the pathname must prevent the old worker from accepting new work.
        std::fs::rename(&path, directory.path().join("displaced.lock")).unwrap();
        let replacement = crate::root::FileLease::acquire(&path).unwrap();
        assert!(matches!(
            retained
                .run::<()>(|_| Box::pin(async { panic!("replaced lease must reject work") }))
                .await,
            Err(StoreError::Io(_))
        ));
        drop(replacement);
        let (first, second) = tokio::join!(owner.shutdown(), retained.shutdown());
        first.unwrap();
        second.unwrap();
        retained.shutdown().await.unwrap();
        contender.try_lock().unwrap();
        assert!(matches!(
            retained.run(|_| Box::pin(async { Ok(()) })).await,
            Err(StoreError::ConnectionClosed)
        ));
    }
}
