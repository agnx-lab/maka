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

use super::Presentation;
use nsh_platform_tui::InputSnapshot;
use std::{
    io::{self, Write},
    sync::mpsc,
    thread,
    time::Instant,
};

pub(super) const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

enum Request {
    Frame(
        Vec<u8>,
        mpsc::SyncSender<io::Result<Instant>>,
        Option<(Presentation, u64, Box<InputSnapshot>)>,
    ),
    Close(Vec<u8>, mpsc::SyncSender<io::Result<()>>),
}

/// The only stdout owner. A receipt covers the original bytes and flush;
/// abandoning observation never replaces a partially written ANSI frame.
pub(super) struct Output {
    send: mpsc::SyncSender<Request>,
    pending: Option<mpsc::Receiver<io::Result<Instant>>>,
    flushed: Option<Instant>,
}

impl Output {
    pub fn new(mut writer: impl Write + Send + 'static) -> io::Result<Self> {
        // One frame plus one final cleanup, including before the worker has
        // picked up that frame. submit itself enforces a single frame.
        let (send, receive) = mpsc::sync_channel(2);
        thread::Builder::new()
            .name("maka-nsh-output".into())
            .spawn(move || {
                while let Ok(request) = receive.recv() {
                    match request {
                        Request::Frame(bytes, receipt, presentation) => {
                            let result = writer.write_all(&bytes).and_then(|_| writer.flush());
                            let failed = result.is_err();
                            let result = result.map(|()| {
                                let flushed = Instant::now();
                                if let Some((presentation, epoch, snapshot)) = presentation {
                                    presentation.publish(epoch, *snapshot);
                                }
                                flushed
                            });
                            let _ = receipt.send(result);
                            if failed {
                                break;
                            }
                        }
                        Request::Close(bytes, receipt) => {
                            let _ =
                                receipt.send(writer.write_all(&bytes).and_then(|_| writer.flush()));
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            send,
            pending: None,
            flushed: None,
        })
    }

    pub fn submit(
        &mut self,
        bytes: Vec<u8>,
        presentation: Option<(Presentation, u64, InputSnapshot)>,
    ) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("terminal frame still in flight".into());
        }
        // Display producers enforce their own frame budget. This original
        // write may also contain one complete encoded clipboard value.
        let (send, receive) = mpsc::sync_channel(1);
        let presentation =
            presentation.map(|(owner, epoch, snapshot)| (owner, epoch, Box::new(snapshot)));
        self.send
            .try_send(Request::Frame(bytes, send, presentation))
            .map_err(|error| error.to_string())?;
        self.pending = Some(receive);
        Ok(())
    }

    pub fn poll(&mut self) -> Result<bool, String> {
        let Some(receive) = &self.pending else {
            return Ok(true);
        };
        match receive.try_recv() {
            Ok(result) => {
                self.pending = None;
                result
                    .map(|flushed| {
                        self.flushed = Some(flushed);
                        true
                    })
                    .map_err(|error| error.to_string())
            }
            Err(mpsc::TryRecvError::Empty) => Ok(false),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("terminal output worker stopped before flush".into())
            }
        }
    }

    /// Actual writer flush time, rather than the time the driver polled its receipt.
    pub fn take_flushed(&mut self) -> Option<Instant> {
        self.flushed.take()
    }

    /// Queue restoration behind the current frame, without joining a writer
    /// that the terminal may have blocked indefinitely.
    pub fn close(self, cleanup: Vec<u8>) -> Result<mpsc::Receiver<io::Result<()>>, String> {
        let (send, receive) = mpsc::sync_channel(1);
        self.send
            .try_send(Request::Close(cleanup, send))
            .map_err(|error| error.to_string())?;
        Ok(receive)
    }
}

#[derive(Debug)]
struct FrameBudget;

impl std::fmt::Display for FrameBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("terminal frame exceeds byte limit")
    }
}
impl std::error::Error for FrameBudget {}

pub(super) fn exceeded_budget(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|error| error.is::<FrameBudget>())
}

pub(super) struct FrameWriter(pub Vec<u8>, usize);

impl Default for FrameWriter {
    fn default() -> Self {
        Self(Vec::new(), MAX_FRAME_BYTES)
    }
}

impl FrameWriter {
    /// Reserve the exact epilogue before attempting a candidate buffer diff.
    pub fn reserve(&mut self, suffix: usize) -> io::Result<()> {
        self.1 = MAX_FRAME_BYTES
            .checked_sub(suffix)
            .ok_or_else(|| io::Error::other(FrameBudget))?;
        if self.0.len() > self.1 {
            return Err(io::Error::other(FrameBudget));
        }
        Ok(())
    }

    pub fn release(&mut self) {
        self.1 = MAX_FRAME_BYTES;
    }
}

impl Write for FrameWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|size| size > self.1)
        {
            return Err(io::Error::other(FrameBudget));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn snapshot() -> InputSnapshot {
        nsh_platform_tui::Surface::new(40, 8, nsh_platform_tui::CellMetrics::default())
            .input_snapshot()
    }

    fn complete_clipboard_payload() -> Vec<u8> {
        use base64::Engine as _;
        let text = "界".repeat(2 * 1024 * 1024 + 1);
        let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
        let payload = format!("\x1b]52;c;{encoded}\x07").into_bytes();
        assert!(payload.len() > MAX_FRAME_BYTES);
        payload
    }

    struct GatedWriter {
        bytes: Arc<Mutex<Vec<u8>>>,
        gate: mpsc::Receiver<()>,
        entered: mpsc::SyncSender<()>,
        first_write: bool,
    }
    impl Write for GatedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !bytes.is_empty() {
                let _ = self.entered.try_send(());
                self.gate
                    .recv()
                    .map_err(|_| io::Error::other("test gate closed"))?;
                // Force one partial write, then resume the complete original
                // buffer without a separate gate operation for every byte.
                let count = if std::mem::take(&mut self.first_write) {
                    1
                } else {
                    bytes.len()
                };
                self.bytes
                    .lock()
                    .unwrap()
                    .extend_from_slice(&bytes[..count]);
                Ok(count)
            } else {
                Ok(0)
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn query_flush_receipt_uses_the_writer_time_without_publishing_a_frame() {
        struct FlushGate {
            entered: mpsc::SyncSender<()>,
            release: Option<mpsc::Receiver<()>>,
        }
        impl Write for FlushGate {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                if let Some(release) = self.release.take() {
                    self.entered.send(()).unwrap();
                    release.recv().unwrap();
                }
                Ok(())
            }
        }
        let (entered, observing) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::channel();
        let mut output = Output::new(FlushGate {
            entered,
            release: Some(gate),
        })
        .unwrap();
        output
            .submit(super::super::graphics::QUERY.to_vec(), None)
            .unwrap();
        observing.recv_timeout(Duration::from_secs(1)).unwrap();
        let before_flush = Instant::now();
        assert!(!output.poll().unwrap());
        assert!(output.take_flushed().is_none());
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !output.poll().unwrap() {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert!(output.take_flushed().unwrap() >= before_flush);
        assert!(output.take_flushed().is_none());
        output
            .close(Vec::new())
            .unwrap()
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
    }

    #[test]
    fn reserved_suffix_and_prefix_share_the_complete_frame_limit() {
        let mut writer = FrameWriter::default();
        writer.write_all(b"PREFIX").unwrap();
        writer.reserve(17).unwrap();
        writer
            .write_all(&vec![b'x'; MAX_FRAME_BYTES - 6 - 17])
            .unwrap();
        let error = writer.write_all(b"!").unwrap_err();
        assert!(exceeded_budget(&error));
        writer.release();
        writer.write_all(&[b'e'; 17]).unwrap();
        assert_eq!(writer.0.len(), MAX_FRAME_BYTES);
        assert!(exceeded_budget(&writer.write_all(b"!").unwrap_err()));
        assert!(!exceeded_budget(&io::Error::other("real IO failure")));
    }

    #[test]
    fn blocked_output_retains_one_frame_and_restores_after_its_last_byte() {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let (gate, receive) = mpsc::channel();
        let (entered, observing) = mpsc::sync_channel(1);
        let mut output = Output::new(GatedWriter {
            bytes: bytes.clone(),
            gate: receive,
            entered,
            first_write: true,
        })
        .unwrap();
        let presentation = Presentation::default();
        let original = complete_clipboard_payload();
        output
            .submit(
                original.clone(),
                Some((presentation.clone(), 7, snapshot())),
            )
            .unwrap();
        observing.recv_timeout(Duration::from_secs(1)).unwrap();
        gate.send(()).unwrap();
        observing.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            bytes.lock().unwrap().as_slice() == &original[..1],
            "only the original prefix may be written while blocked"
        );
        assert_eq!(
            presentation
                .presented
                .load(std::sync::atomic::Ordering::Acquire),
            0
        );
        assert!(!output.poll().unwrap());
        assert!(output.submit(b"replacement".to_vec(), None).is_err());
        let closed = output.close(b"!".to_vec()).unwrap();
        assert!(matches!(closed.try_recv(), Err(mpsc::TryRecvError::Empty)));
        for _ in 0..2 {
            gate.send(()).unwrap();
        }
        closed
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        let mut expected = original;
        expected.push(b'!');
        assert!(
            bytes.lock().unwrap().as_slice() == expected.as_slice(),
            "the complete OSC52 must precede cleanup exactly once"
        );
        // No driver poll observes either completion: the actual flush
        // publishes the frame before the final cleanup receipt arrives.
        assert_eq!(
            presentation
                .presented
                .load(std::sync::atomic::Ordering::Acquire),
            7
        );
    }

    #[test]
    fn flush_failure_never_produces_a_success_receipt() {
        struct Failed(Arc<Mutex<Vec<u8>>>);
        impl Write for Failed {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("flush failed"))
            }
        }
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let mut output = Output::new(Failed(bytes.clone())).unwrap();
        let presentation = Presentation::default();
        let original = complete_clipboard_payload();
        output
            .submit(
                original.clone(),
                Some((presentation.clone(), 7, snapshot())),
            )
            .unwrap();
        loop {
            match output.poll() {
                Ok(false) => thread::yield_now(),
                Err(error) => {
                    assert_eq!(error, "flush failed");
                    assert!(
                        bytes.lock().unwrap().as_slice() == original.as_slice(),
                        "flush failure must follow the complete original write"
                    );
                    assert!(output.take_flushed().is_none());
                    assert_eq!(
                        presentation
                            .presented
                            .load(std::sync::atomic::Ordering::Acquire),
                        0
                    );
                    break;
                }
                Ok(true) => panic!("failed frame admitted"),
            }
        }
    }
}
