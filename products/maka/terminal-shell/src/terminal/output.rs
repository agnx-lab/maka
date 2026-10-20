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
};

pub(super) const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

enum Request {
    Frame(
        Vec<u8>,
        mpsc::SyncSender<io::Result<()>>,
        Option<(Presentation, u64, InputSnapshot)>,
    ),
    Close(Vec<u8>, mpsc::SyncSender<io::Result<()>>),
}

/// The only stdout owner. A receipt covers the original bytes and flush;
/// abandoning observation never replaces a partially written ANSI frame.
pub(super) struct Output {
    send: mpsc::SyncSender<Request>,
    pending: Option<mpsc::Receiver<io::Result<()>>>,
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
                    let (bytes, receipt, close, presentation) = match request {
                        Request::Frame(bytes, receipt, presentation) => {
                            (bytes, receipt, false, presentation)
                        }
                        Request::Close(bytes, receipt) => (bytes, receipt, true, None),
                    };
                    let result = writer.write_all(&bytes).and_then(|_| writer.flush());
                    let failed = result.is_err();
                    if !failed && let Some((presentation, epoch, snapshot)) = presentation {
                        presentation.publish(epoch, snapshot);
                    }
                    let _ = receipt.send(result);
                    if close || failed {
                        break;
                    }
                }
            })?;
        Ok(Self {
            send,
            pending: None,
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
        if bytes.len() > MAX_FRAME_BYTES {
            return Err("terminal frame exceeds byte limit".into());
        }
        let (send, receive) = mpsc::sync_channel(1);
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
                result.map(|_| true).map_err(|error| error.to_string())
            }
            Err(mpsc::TryRecvError::Empty) => Ok(false),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("terminal output worker stopped before flush".into())
            }
        }
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

#[derive(Default)]
pub(super) struct FrameWriter(pub Vec<u8>);

impl Write for FrameWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|size| size > MAX_FRAME_BYTES)
        {
            return Err(io::Error::other("terminal frame exceeds byte limit"));
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

    struct GatedWriter {
        bytes: Arc<Mutex<Vec<u8>>>,
        gate: mpsc::Receiver<()>,
        entered: mpsc::SyncSender<()>,
    }
    impl Write for GatedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !bytes.is_empty() {
                let _ = self.entered.try_send(());
                self.gate
                    .recv()
                    .map_err(|_| io::Error::other("test gate closed"))?;
                // Force write_all to resume the original partial frame.
                self.bytes.lock().unwrap().push(bytes[0]);
                Ok(1)
            } else {
                Ok(0)
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
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
        })
        .unwrap();
        let presentation = Presentation::default();
        output
            .submit(b"ABC".to_vec(), Some((presentation.clone(), 7, snapshot())))
            .unwrap();
        observing.recv_timeout(Duration::from_secs(1)).unwrap();
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
        for _ in 0..4 {
            gate.send(()).unwrap();
        }
        closed
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert_eq!(*bytes.lock().unwrap(), b"ABC!");
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
        struct Failed;
        impl Write for Failed {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("flush failed"))
            }
        }
        let mut output = Output::new(Failed).unwrap();
        let presentation = Presentation::default();
        output
            .submit(
                b"frame".to_vec(),
                Some((presentation.clone(), 7, snapshot())),
            )
            .unwrap();
        loop {
            match output.poll() {
                Ok(false) => thread::yield_now(),
                Err(error) => {
                    assert_eq!(error, "flush failed");
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
