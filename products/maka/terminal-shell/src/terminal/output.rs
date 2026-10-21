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

enum Payload {
    Bytes(Vec<u8>),
    #[cfg(target_os = "linux")]
    Pixels {
        frame: Box<nsh_gpui_terminal::CandidateFrame>,
        image: u32,
        previous: Option<u32>,
        columns: u16,
        rows: u16,
        geometry: u64,
    },
}

#[cfg(target_os = "linux")]
pub(super) struct PixelUpload {
    pub token: nsh_gpui_terminal::FrameToken,
    pub image: u32,
    pub retire: Vec<u8>,
    pub geometry: u64,
}

struct Flushed {
    at: Instant,
    #[cfg(target_os = "linux")]
    pixels: Option<PixelUpload>,
}

enum Request {
    Frame(
        Payload,
        mpsc::SyncSender<io::Result<Flushed>>,
        Option<(Presentation, u64, Box<InputSnapshot>)>,
    ),
    Close(Vec<u8>, mpsc::SyncSender<io::Result<()>>),
}

/// The only stdout owner. A receipt covers the original bytes and flush;
/// abandoning observation never replaces a partially written ANSI frame.
pub(super) struct Output {
    send: mpsc::SyncSender<Request>,
    pending: Option<mpsc::Receiver<io::Result<Flushed>>>,
    flushed: Option<Instant>,
    #[cfg(target_os = "linux")]
    pixels: Option<PixelUpload>,
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
                        Request::Frame(payload, receipt, presentation) => {
                            let mut attempted_write = false;
                            let result = (|| {
                                #[cfg(target_os = "linux")]
                                let mut pixels = None;
                                match payload {
                                    Payload::Bytes(bytes) => {
                                        attempted_write = true;
                                        writer.write_all(&bytes)?;
                                    }
                                    #[cfg(target_os = "linux")]
                                    Payload::Pixels {
                                        frame,
                                        image,
                                        previous,
                                        columns,
                                        rows,
                                        geometry,
                                    } => {
                                        let frame = nsh_gpui_terminal::kitty::KittyFrame::encode(
                                            &frame, image, previous, columns, rows,
                                        )
                                        .map_err(io::Error::other)?;
                                        pixels = Some(PixelUpload {
                                            token: frame.token(),
                                            image: frame.image_id(),
                                            retire: frame.retire_previous().to_vec(),
                                            geometry,
                                        });
                                        attempted_write = true;
                                        writer.write_all(frame.upload())?;
                                    }
                                };
                                writer.flush()?;
                                Ok(Flushed {
                                    at: Instant::now(),
                                    #[cfg(target_os = "linux")]
                                    pixels,
                                })
                            })();
                            let failed = result.is_err();
                            let result = result.map(|flushed| {
                                if let Some((presentation, epoch, snapshot)) = presentation {
                                    presentation.publish(epoch, *snapshot);
                                }
                                flushed
                            });
                            let _ = receipt.send(result);
                            if failed && attempted_write {
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
            #[cfg(target_os = "linux")]
            pixels: None,
        })
    }

    pub fn submit(
        &mut self,
        bytes: Vec<u8>,
        presentation: Option<(Presentation, u64, InputSnapshot)>,
    ) -> Result<(), String> {
        if bytes.len() > MAX_FRAME_BYTES {
            return Err("terminal frame exceeds byte limit".into());
        }
        let presentation =
            presentation.map(|(owner, epoch, snapshot)| (owner, epoch, Box::new(snapshot)));
        self.queue(Payload::Bytes(bytes), presentation)
    }

    /// Compression and upload run on the existing sole writer. Neither the
    /// terminal reader nor the GPUI thread waits for a slow terminal link.
    #[cfg(target_os = "linux")]
    pub fn submit_pixels(
        &mut self,
        frame: nsh_gpui_terminal::CandidateFrame,
        image: u32,
        previous: Option<u32>,
        columns: u16,
        rows: u16,
        geometry: u64,
    ) -> Result<(), String> {
        self.queue(
            Payload::Pixels {
                frame: Box::new(frame),
                image,
                previous,
                columns,
                rows,
                geometry,
            },
            None,
        )
    }

    fn queue(
        &mut self,
        payload: Payload,
        presentation: Option<(Presentation, u64, Box<InputSnapshot>)>,
    ) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("terminal frame still in flight".into());
        }
        let (send, receive) = mpsc::sync_channel(1);
        self.send
            .try_send(Request::Frame(payload, send, presentation))
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
                        self.flushed = Some(flushed.at);
                        #[cfg(target_os = "linux")]
                        {
                            self.pixels = flushed.pixels;
                        }
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

    #[cfg(target_os = "linux")]
    pub fn take_pixels(&mut self) -> Option<PixelUpload> {
        self.pixels.take()
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

    #[cfg(target_os = "linux")]
    #[test]
    fn rejected_pixel_encoding_keeps_the_healthy_writer_for_restoration() {
        struct Recorded(Arc<Mutex<(Vec<u8>, u32)>>);
        impl Write for Recorded {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().0.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                self.0.lock().unwrap().1 += 1;
                Ok(())
            }
        }
        let recorded = Arc::new(Mutex::new((Vec::new(), 0)));
        let mut output = Output::new(Recorded(recorded.clone())).unwrap();
        // An inert candidate is rejected by the real codec before any byte.
        output
            .submit_pixels(
                nsh_gpui_terminal::CandidateFrame::default(),
                41,
                None,
                120,
                40,
                1,
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match output.poll() {
                Err(_) => break,
                Ok(false) => {
                    assert!(Instant::now() < deadline);
                    thread::yield_now();
                }
                Ok(true) => panic!("inert pixel frame must fail admission"),
            }
        }
        output
            .close(b"RESTORE".to_vec())
            .unwrap()
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert_eq!(*recorded.lock().unwrap(), (b"RESTORE".to_vec(), 1));
    }
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
