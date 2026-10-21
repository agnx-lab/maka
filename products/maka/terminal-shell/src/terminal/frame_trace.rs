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

//! Opt-in measurements under the existing presentation lock. Nothing is
//! formatted or written until terminal restoration has finished.

use nsh_gpui_terminal::FrameToken;
use serde_json::{Value, json};
use std::{io::Write, path::PathBuf, time::Instant};

pub(super) struct Trace {
    path: PathBuf,
    started: Instant,
    environment: Value,
    samples: Vec<Sample>,
    pub gpu: Option<Value>,
    pub protocol: Option<&'static str>,
}

pub(super) struct Sample {
    frame: u64,
    viewport_generation: u64,
    geometry: u64,
    stage: &'static str,
    start: Instant,
    end: Instant,
    pub bytes: Option<usize>,
    pub pixel_size: Option<(i32, i32)>,
    pub image: Option<u32>,
    pub outcome: &'static str,
}

impl Trace {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("MAKA_TERMINAL_FRAME_TRACE")?;
        Some(Self {
            path: path.into(),
            started: Instant::now(),
            environment: json!({
                "TERM": std::env::var("TERM").ok(),
                "TERM_PROGRAM": std::env::var("TERM_PROGRAM").ok(),
                "DISPLAY": std::env::var("DISPLAY").ok(),
                "WAYLAND_DISPLAY": std::env::var("WAYLAND_DISPLAY").ok(),
            }),
            // Preallocate a minute at the target 120 Hz and eight Kitty
            // stages. This avoids growth during short measurements, not a cap.
            samples: Vec::with_capacity(60 * 120 * 8),
            gpu: None,
            protocol: None,
        })
    }

    pub fn record(
        &mut self,
        token: &FrameToken,
        geometry: u64,
        stage: &'static str,
        start: Instant,
        end: Instant,
    ) -> &mut Sample {
        self.samples.push(Sample {
            frame: token.sequence(),
            viewport_generation: token.viewport_generation(),
            geometry,
            stage,
            start,
            end,
            bytes: None,
            pixel_size: None,
            image: None,
            outcome: "ok",
        });
        self.samples.last_mut().unwrap()
    }

    pub fn write(self, result: &Result<(), String>) -> Result<(), String> {
        let stopped = Instant::now();
        let samples: Vec<_> = self
            .samples
            .into_iter()
            .map(|sample| {
                json!({
                    "frame": sample.frame,
                    "viewport_generation": sample.viewport_generation,
                    "geometry": sample.geometry,
                    "stage": sample.stage,
                    "start_us": sample.start.saturating_duration_since(self.started).as_micros(),
                    "end_us": sample.end.saturating_duration_since(self.started).as_micros(),
                    "bytes": sample.bytes,
                    "pixel_size": sample.pixel_size,
                    "image": sample.image,
                    "outcome": sample.outcome,
                })
            })
            .collect();
        let data = json!({
            "format": "maka-terminal-frame-trace-v1",
            "version": env!("CARGO_PKG_VERSION"),
            "debug_assertions": cfg!(debug_assertions),
            "pid": std::process::id(),
            "environment": self.environment,
            "gpu": self.gpu,
            "protocol": self.protocol,
            "stopped_us": stopped.saturating_duration_since(self.started).as_micros(),
            "terminal_result": result.as_ref().err(),
            "samples": samples,
        });
        let write = || -> Result<(), Box<dyn std::error::Error>> {
            let mut output = std::io::BufWriter::new(std::fs::File::create(&self.path)?);
            serde_json::to_writer(&mut output, &data)?;
            output.write_all(b"\n")?;
            output.flush()?;
            Ok(())
        };
        write().map_err(|error| format!("terminal frame trace {}: {error}", self.path.display()))
    }
}
