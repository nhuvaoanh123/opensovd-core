// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
// SPDX-License-Identifier: Apache-2.0

//! Diagnostic Fault Manager (DFM) for the Eclipse `OpenSOVD` core stack.
//!
//! Central per-system fault aggregator. Receives faults from the embedded
//! Fault Library via the
//! [`sovd_interfaces::traits::fault_sink::FaultSink`] trait, persists them
//! through `sovd-db`, and exposes them as a
//! [`sovd_interfaces::traits::backend::SovdBackend`] to `sovd-gateway`.
//!
//! See [`ARCHITECTURE.md`](../../ARCHITECTURE.md) for role boundaries.
//! Phase 0 stub — aggregation, debouncing, operation-cycle gating, and
//! persistence land in Phase 3.

/// Diagnostic Fault Manager instance.
///
/// Will implement both
/// [`sovd_interfaces::traits::fault_sink::FaultSink`] (ingestion side) and
/// [`sovd_interfaces::traits::backend::SovdBackend`] (read side) in
/// Phase 3. Fields (`SQLite` pool, in-memory DTC table, debounce config) are
/// added then.
pub struct Dfm {
    // fields added in Phase 3
}
