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
//! See upstream
//! [`design.md`](../../../../opensovd/docs/design/design.md) §"Fault
//! Library" for why this interface exists and what promises it makes.

use async_trait::async_trait;

use crate::extras::fault::FaultRecord;
use crate::types::error::Result;

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
    /// `FaultRecord` represent two observations. Deduplication (if any)
    /// is the DFM's decision, not the caller's.
    async fn record_fault(&self, fault: FaultRecord) -> Result<()>;
}
