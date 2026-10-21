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

//! Presenter-owned aggregate admission; permits follow actual allocations and work.
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("native resource budget exhausted")]
pub struct BudgetExhausted;

#[derive(Clone, Debug)]
pub struct NativeBudget(Arc<Mutex<Usage>>);
#[derive(Debug)]
struct Usage {
    encoded: usize,
    pixels: usize,
    jobs: usize,
    limits: BudgetLimits,
}
#[derive(Clone, Copy, Debug)]
pub struct BudgetLimits {
    pub encoded_bytes: usize,
    pub pixel_bytes: usize,
    pub worker_jobs: usize,
}
impl Default for BudgetLimits {
    fn default() -> Self {
        Self {
            encoded_bytes: 64 * 1024 * 1024,
            pixel_bytes: 128 * 1024 * 1024,
            worker_jobs: 16,
        }
    }
}
impl Default for NativeBudget {
    fn default() -> Self {
        Self::new(BudgetLimits::default())
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Encoded,
    Pixels,
    /// Aggregate native work admission. Bytes stay charged by their own permits.
    Job,
}
#[derive(Debug)]
pub struct Permit {
    budget: NativeBudget,
    kind: Kind,
    bytes: usize,
}
impl NativeBudget {
    pub fn new(limits: BudgetLimits) -> Self {
        Self(Arc::new(Mutex::new(Usage {
            encoded: 0,
            pixels: 0,
            jobs: 0,
            limits,
        })))
    }
    pub fn reserve(&self, kind: Kind, bytes: usize) -> Result<Permit, BudgetExhausted> {
        let mut usage = self.0.lock().expect("native budget lock");
        let (used, limit) = match kind {
            Kind::Encoded => (usage.encoded, usage.limits.encoded_bytes),
            Kind::Pixels => (usage.pixels, usage.limits.pixel_bytes),
            Kind::Job => (usage.jobs, usage.limits.worker_jobs),
        };
        let next = used
            .checked_add(bytes)
            .filter(|n| *n <= limit)
            .ok_or(BudgetExhausted)?;
        match kind {
            Kind::Encoded => usage.encoded = next,
            Kind::Pixels => usage.pixels = next,
            Kind::Job => usage.jobs = next,
        }
        Ok(Permit {
            budget: self.clone(),
            kind,
            bytes,
        })
    }
    pub fn usage(&self) -> (usize, usize, usize) {
        let u = self.0.lock().unwrap();
        (u.encoded, u.pixels, u.jobs)
    }

    /// Reserve currently available capacity before an opaque native getter allocates.
    pub fn reserve_remaining(&self, kind: Kind) -> Result<Permit, BudgetExhausted> {
        let mut usage = self.0.lock().expect("native budget lock");
        let (used, limit) = match kind {
            Kind::Encoded => (usage.encoded, usage.limits.encoded_bytes),
            Kind::Pixels => (usage.pixels, usage.limits.pixel_bytes),
            Kind::Job => (usage.jobs, usage.limits.worker_jobs),
        };
        let bytes = limit
            .checked_sub(used)
            .filter(|bytes| *bytes > 0)
            .ok_or(BudgetExhausted)?;
        match kind {
            Kind::Encoded => usage.encoded = limit,
            Kind::Pixels => usage.pixels = limit,
            Kind::Job => usage.jobs = limit,
        }
        Ok(Permit {
            budget: self.clone(),
            kind,
            bytes,
        })
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut usage = self.budget.0.lock().expect("native budget lock");
        let value = match self.kind {
            Kind::Encoded => &mut usage.encoded,
            Kind::Pixels => &mut usage.pixels,
            Kind::Job => &mut usage.jobs,
        };
        *value = value
            .checked_sub(self.bytes)
            .expect("original native budget reservation");
    }
}

impl Permit {
    pub fn capacity(&self) -> usize {
        self.bytes
    }

    /// Return unused capacity after a native API reports its actual allocation.
    pub fn shrink_to(&mut self, bytes: usize) {
        assert!(
            bytes <= self.bytes,
            "native permit cannot grow without admission"
        );
        let released = self.bytes - bytes;
        let mut usage = self.budget.0.lock().expect("native budget lock");
        let used = match self.kind {
            Kind::Encoded => &mut usage.encoded,
            Kind::Pixels => &mut usage.pixels,
            Kind::Job => &mut usage.jobs,
        };
        *used = used
            .checked_sub(released)
            .expect("original native reservation");
        self.bytes = bytes;
    }
}
