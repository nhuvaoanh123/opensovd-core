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

//! [`SovdDb`] — persistence trait for the Diagnostic Fault Manager.
//!
//! The DFM core depends on this trait and nothing else for fault
//! persistence. The concrete backend is picked at runtime by `sovd-main`
//! from the `[backend]` TOML section. Two backends are anticipated:
//!
//! - `sovd-db-sqlite` — default standalone backend (`SQLite` + sqlx + WAL).
//!   See ADR-0003 for the rationale.
//! - `sovd-db-score` — optional S-CORE backend wrapping `score-persistency`.
//!   See ADR-0016 for the pluggability contract.
//!
//! # Narrow surface
//!
//! Methods on this trait are **domain-level**, not storage-level. There
//! are no raw SQL strings, no KVS primitives, and no table/column names.
//! This is deliberate per ADR-0016 §"The three pluggable seams": the trait
//! must fit both a relational store and a key-value store without either
//! one having to lie. If a Phase 4 need arises to leak SQL or KVS
//! semantics through the trait, stop and widen the abstraction instead.
//!
//! # Types at the boundary
//!
//! Trait methods take and return `sovd_interfaces::spec` types (for
//! anything the ISO 17978-3 SOVD spec covers) and `sovd_interfaces::extras`
//! types (for the Taktflow-specific IPC shapes that the spec is silent on).
//! Raw `serde_json::Value` is never used for anything the spec or extras
//! define, per ADR-0015.

use async_trait::async_trait;

use crate::{
    extras::fault::FaultRecord,
    spec::fault::{FaultDetails, FaultFilter, ListOfFaults},
    types::error::Result,
};

/// Opaque identifier for an operation cycle snapshot taken by the DFM.
///
/// Extras-level: the SOVD spec has no notion of "cycle snapshot" at the
/// HTTP wire boundary, but both the `SQLite` and the S-CORE backends need a
/// stable handle to refer to a frozen view of the fault table at cycle-end.
/// Free-form string per ADR-0012's cycle-name namespace.
pub type OperationCycleId = String;

/// Persistence contract for the Diagnostic Fault Manager.
///
/// Implementations MUST be `Send + Sync` and cheap to share behind an
/// `Arc`. Expensive work (I/O) happens inside each async method.
///
/// See ADR-0003 (`SQLite` default), ADR-0015 (type layering), and ADR-0016
/// (pluggability contract).
#[async_trait]
pub trait SovdDb: Send + Sync {
    /// Ingest a single fault event from the Fault Library shim.
    ///
    /// This is idempotent at the **event** level: two calls with the
    /// same [`FaultRecord`] create two rows / two entries. Deduplication
    /// (if any) is the DFM's decision, not the backend's.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`](crate::types::error::SovdError::Internal)
    /// or [`SovdError::Transport`](crate::types::error::SovdError::Transport)
    /// if the backing store is unavailable.
    async fn ingest_fault(&self, record: FaultRecord) -> Result<()>;

    /// List faults matching `filter`, shaped as the SOVD
    /// `ListOfFaults` response.
    ///
    /// The result is aggregated by fault **code** (one [`spec::Fault`] per
    /// code, not per event), per ADR-0003 §2.2 and the SOVD semantics
    /// under `GET .../faults`.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`](crate::types::error::SovdError::Internal)
    /// if the backing store fails to answer.
    ///
    /// [`spec::Fault`]: crate::spec::fault::Fault
    async fn list_faults(&self, filter: FaultFilter) -> Result<ListOfFaults>;

    /// Fetch one fault entry by its SOVD `code` (free-form string, per
    /// spec — not a UDS 24-bit integer).
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::NotFound`](crate::types::error::SovdError::NotFound)
    /// if no fault with the given code has ever been ingested or if it
    /// was previously cleared.
    async fn get_fault(&self, code: &str) -> Result<FaultDetails>;

    /// Clear faults matching `filter`. An empty filter
    /// ([`FaultFilter::all`]) clears every fault in the store — this is
    /// the `DELETE .../faults` path. A filter with a `code`-equivalent
    /// predicate clears one fault — this is the `DELETE .../faults/{code}`
    /// path.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`](crate::types::error::SovdError::Internal)
    /// if the backing store fails to apply the clear.
    async fn clear_faults(&self, filter: FaultFilter) -> Result<()>;

    /// Clear exactly one fault by code. Convenience wrapper separate from
    /// [`Self::clear_faults`] so backends can implement it as a targeted
    /// delete rather than materialising a full filter match.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::NotFound`](crate::types::error::SovdError::NotFound)
    /// if the code is unknown at the moment of the call.
    async fn clear_fault_by_code(&self, code: &str) -> Result<()>;

    /// Snapshot the current fault set under the given operation cycle id.
    ///
    /// Backends are free to implement this as a copy, a tag, or a no-op
    /// depending on their storage model — SQLite writes a row per event
    /// with the cycle id as a column; an S-CORE KVS might write a tag
    /// entry. The only promise the trait makes is that after a successful
    /// snapshot, the caller can later query the DFM for faults belonging
    /// to that cycle id (via a `FaultFilter` whose status-key set contains
    /// `("operationCycle", cycle_id)`).
    ///
    /// Used by the [`OperationCycle`](crate::traits::operation_cycle::OperationCycle)
    /// driver at `end_cycle()` time.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`](crate::types::error::SovdError::Internal)
    /// if the backing store fails to apply the snapshot.
    async fn snapshot_for_operation_cycle(&self, cycle_id: &OperationCycleId) -> Result<()>;
}
