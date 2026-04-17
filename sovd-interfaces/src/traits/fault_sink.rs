/*
 * SPDX-License-Identifier: Apache-2.0
 * SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)
 *
 * See the NOTICE file(s) distributed with this work for additional
 * information regarding copyright ownership.
 *
 * This program and the accompanying materials are made available under the
 * terms of the Apache License Version 2.0 which is available at
 * https://www.apache.org/licenses/LICENSE-2.0
 */

//! [`FaultSink`] — DFM-side ingestion point for Fault Library events.
//!
//! The embedded Fault Library shim (C code on each ECU, out of scope for
//! `opensovd-core`) pushes [`FaultRecord`] values over IPC to the central
//! `sovd-dfm` process. Inside the DFM, the IPC decoder drops decoded
//! records into a `FaultSink` implementation.
//!
//! This is the Rust side of ADR-001 ("S-CORE Interface"): it is the one
//! and only API surface through which faults enter the SOVD stack from
//! platform/application code.
//!
//! Per ADR-0016, this trait has two anticipated backends:
//!
//! - `fault-sink-unix` — default standalone backend over a
//!   `tokio::net::UnixListener` (works on Windows 10 1803+ via `AF_UNIX`).
//! - `fault-sink-lola` — optional S-CORE backend wrapping
//!   `score-communication`, using `LoLa` zero-copy shared-memory
//!   skeleton/proxy to move records without a copy.
//!
//! # Buffer-lifetime contract (widened Phase 3)
//!
//! The trait now takes the ingestion shape as a [`FaultRecordRef`] rather
//! than a moved [`FaultRecord`]. The enum has two variants:
//!
//! - `Owned(FaultRecord)` — the classic path used by `fault-sink-unix`,
//!   which decodes the wire bytes into an owned struct before calling the
//!   sink.
//! - `Borrowed(&'buf FaultRecord)` — a reference tied to a caller-owned
//!   lifetime, used by `fault-sink-lola` to point directly at a `LoLa`
//!   shared-memory slot without allocating.
//!
//! Implementations that need ownership simply `.to_owned()` inside the
//! method. Implementations that can operate on a borrow (for example a
//! logging sink that only needs a `Debug` line) avoid the allocation.
//!
//! See upstream
//! [`design.md`](../../../../opensovd/docs/design/design.md) §"Fault
//! Library" for why this interface exists and what promises it makes.

use async_trait::async_trait;

use crate::{extras::fault::FaultRecord, types::error::Result};

/// Either an owned [`FaultRecord`] or a borrow of one.
///
/// See the module docs for why this exists. The `'buf` lifetime is the
/// lifetime of the borrow in the `Borrowed` case; in the `Owned` case
/// the lifetime is unconstrained (which is why the enum is
/// non-exhaustive in practice — consumers match on both variants).
#[derive(Debug)]
pub enum FaultRecordRef<'buf> {
    /// Owned record. Used by the Unix-socket backend which decodes wire
    /// bytes into a [`FaultRecord`] before handing it to the sink.
    Owned(FaultRecord),
    /// Borrowed record. Used by the `LoLa` zero-copy backend which can
    /// point directly at a shared-memory slot without allocating.
    Borrowed(&'buf FaultRecord),
}

impl FaultRecordRef<'_> {
    /// Materialise an owned [`FaultRecord`]. Cheap for the `Owned`
    /// variant (moves out) and allocates for the `Borrowed` variant.
    #[must_use]
    pub fn into_owned(self) -> FaultRecord {
        match self {
            Self::Owned(r) => r,
            Self::Borrowed(r) => r.clone(),
        }
    }

    /// Borrow as a [`FaultRecord`] regardless of the underlying variant.
    /// Used by read-only paths that don't need ownership.
    ///
    /// Named `record` rather than `as_ref` so it does not collide with
    /// `std::convert::AsRef::as_ref`.
    #[must_use]
    pub fn record(&self) -> &FaultRecord {
        match self {
            Self::Owned(r) => r,
            Self::Borrowed(r) => r,
        }
    }
}

impl From<FaultRecord> for FaultRecordRef<'_> {
    fn from(r: FaultRecord) -> Self {
        Self::Owned(r)
    }
}

impl<'buf> From<&'buf FaultRecord> for FaultRecordRef<'buf> {
    fn from(r: &'buf FaultRecord) -> Self {
        Self::Borrowed(r)
    }
}

/// Ingestion sink for faults coming from the Fault Library.
///
/// Implementations must be cheap to call — the Fault Library shim invokes
/// this from arbitrary platform threads. Expensive work (persistence,
/// debounce evaluation, operation-cycle gating) should be deferred to the
/// DFM's own task loop.
///
/// `async` so that implementations can offload persistence without
/// blocking the IPC reader.
#[async_trait]
pub trait FaultSink: Send + Sync {
    /// Record a single fault event.
    ///
    /// This call is **not** idempotent: two calls with the same
    /// [`FaultRecord`] represent two observations. Deduplication (if any)
    /// is the DFM's decision, not the caller's.
    ///
    /// The `record` argument is a [`FaultRecordRef`] so zero-copy
    /// backends can pass a borrow. Owning backends can pass a
    /// `FaultRecord` directly via [`From`].
    async fn record_fault<'buf>(&self, record: FaultRecordRef<'buf>) -> Result<()>;
}
