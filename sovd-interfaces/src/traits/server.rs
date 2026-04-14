// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
// SPDX-License-Identifier: Apache-2.0

//! [`SovdServer`] — one ECU's SOVD endpoint.
//!
//! Implemented by the `sovd-server` crate (one instance per ECU / per device
//! view). Backed by local sources: `sovd-dfm` for fault queries, an
//! MDD-backed DID provider for `data`, and registered routine handlers for
//! `operations`. See upstream
//! [`design.md`](../../../../opensovd/docs/design/design.md) §"SOVD Server".
//!
//! A `SovdServer` serves **one component**. System-wide multiplexing across
//! components is [`SovdGateway`](crate::traits::gateway::SovdGateway)'s job.

use crate::types::{
    component::ComponentInfo,
    data::{DataIdentifier, DataValue},
    dtc::{Dtc, DtcGroup, DtcId, DtcStatusMask},
    error::Result,
    routine::{RoutineId, RoutineState},
};

/// SOVD server for a single ECU/component.
///
/// All methods are async because every real implementation ultimately
/// crosses an IPC or network boundary (DFM over shared memory, CDA over
/// `DoIP`, native MDD provider over tokio channel).
pub trait SovdServer: Send + Sync {
    /// List DTCs currently held for this component.
    ///
    /// `filter` is applied bit-by-bit against each DTC's status byte: only
    /// DTCs with **all** the bits set in the mask are returned. A mask of
    /// `0x00` ([`DtcStatusMask::all`](crate::types::dtc::DtcStatusMask::all))
    /// disables filtering and returns every DTC.
    fn list_dtcs(
        &self,
        filter: DtcStatusMask,
    ) -> impl std::future::Future<Output = Result<Vec<Dtc>>> + Send;

    /// Fetch one DTC by id.
    ///
    /// Returns [`SovdError::NotFound`](crate::SovdError::NotFound) if the
    /// DTC is not currently held. Cleared DTCs are **not** returned — use
    /// history queries (Phase 4) for that.
    fn get_dtc(&self, id: DtcId) -> impl std::future::Future<Output = Result<Dtc>> + Send;

    /// Clear DTCs.
    ///
    /// - `filter = None` clears every DTC held for this component (UDS
    ///   equivalent: `ClearDiagnosticInformation` with group `0xFFFFFF`).
    /// - `filter = Some(DtcGroup::All)` is semantically identical but makes
    ///   the intent explicit at call sites.
    /// - `filter = Some(DtcGroup::Group(code))` clears only DTCs belonging
    ///   to the given group code.
    ///
    /// Clearing is idempotent: clearing an already-empty set is not an
    /// error.
    fn clear_dtcs(
        &self,
        filter: Option<DtcGroup>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Start a routine (SOVD `operations`, UDS service `0x31`).
    ///
    /// `args` carries raw routine input bytes — decoding per ODX/MDD is the
    /// implementer's responsibility. On success the routine has been
    /// **accepted**, not necessarily completed; poll
    /// [`routine_status`](Self::routine_status) to observe progress.
    fn start_routine(
        &self,
        id: RoutineId,
        args: &[u8],
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Query the current state of a routine previously started via
    /// [`start_routine`](Self::start_routine).
    ///
    /// Returns [`RoutineState::Idle`](crate::types::routine::RoutineState::Idle)
    /// if the routine has never been started on this backend.
    fn routine_status(
        &self,
        id: RoutineId,
    ) -> impl std::future::Future<Output = Result<RoutineState>> + Send;

    /// Read one Data Identifier.
    ///
    /// On CDA-backed ECUs this translates to UDS `0x22
    /// ReadDataByIdentifier`. On native servers it reads from the local
    /// MDD-backed provider. The returned [`DataValue`] is already decoded;
    /// schema introspection (`?include-schema=true`) is handled at the HTTP
    /// layer, not here.
    fn read_did(
        &self,
        did: DataIdentifier,
    ) -> impl std::future::Future<Output = Result<DataValue>> + Send;

    /// Return static-ish component metadata (`GET /components/{id}`).
    fn component_info(&self) -> impl std::future::Future<Output = Result<ComponentInfo>> + Send;
}
