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
//!
//! Trait method signatures use the spec-derived DTOs from [`crate::spec`]
//! (ported from the ASAM SOVD v1.1.0-rc1 `OpenAPI` template). The internal
//! [`SovdError`](crate::types::error::SovdError) enum is the carve-out: it
//! is the Rust error type returned from every method, mapped onto the
//! wire-format [`spec::error::GenericError`](crate::spec::error::GenericError)
//! at the HTTP layer.

use crate::{
    spec::{
        component::EntityCapabilities,
        data::ReadValue,
        fault::{FaultDetails, FaultFilter, ListOfFaults},
        operation::{ExecutionStatusResponse, StartExecutionAsyncResponse, StartExecutionRequest},
    },
    types::error::Result,
};

/// SOVD server for a single ECU/component.
///
/// All methods are async because every real implementation ultimately
/// crosses an IPC or network boundary (DFM over shared memory, CDA over
/// `DoIP`, native MDD provider over tokio channel).
pub trait SovdServer: Send + Sync {
    /// List faults currently held for this component (`GET .../faults`).
    ///
    /// `filter` is the spec-defined combination of `status[key]` matches,
    /// `severity` upper bound, and `scope`. The empty filter
    /// ([`FaultFilter::all`]) returns every fault.
    fn list_faults(
        &self,
        filter: FaultFilter,
    ) -> impl std::future::Future<Output = Result<ListOfFaults>> + Send;

    /// Fetch the details of one fault by code (`GET .../faults/{fault-code}`).
    ///
    /// `code` is the native fault code as a string (e.g. `"0012E3"`,
    /// `"P102"`, `"modelMissing"`) — the spec uses string codes, not
    /// integers. Returns
    /// [`SovdError::NotFound`](crate::types::error::SovdError::NotFound) if
    /// the code is not currently held.
    fn get_fault(
        &self,
        code: &str,
    ) -> impl std::future::Future<Output = Result<FaultDetails>> + Send;

    /// Clear all faults for this component (`DELETE .../faults`).
    ///
    /// The spec only exposes "delete every fault" and "delete one specific
    /// fault by code" — there is no group-based clear. Clearing is
    /// idempotent: clearing an already-empty set is not an error.
    fn clear_all_faults(&self) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Clear one fault by code (`DELETE .../faults/{fault-code}`).
    fn clear_fault(&self, code: &str) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Start an operation (SOVD `POST .../operations/{op}/executions`).
    ///
    /// `operation_id` is the SOVD operation id (string, e.g.
    /// `"SteeringAngleControl"`). `request` carries the optional
    /// `timeout`, free-form `parameters`, and `proximity_response`. The
    /// returned [`StartExecutionAsyncResponse`] carries the execution id
    /// the caller polls via [`execution_status`](Self::execution_status).
    fn start_execution(
        &self,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> impl std::future::Future<Output = Result<StartExecutionAsyncResponse>> + Send;

    /// Query the current status of a previously started execution
    /// (`GET .../operations/{op}/executions/{execution-id}`).
    fn execution_status(
        &self,
        operation_id: &str,
        execution_id: &str,
    ) -> impl std::future::Future<Output = Result<ExecutionStatusResponse>> + Send;

    /// Read one data resource (`GET .../data/{data-id}`).
    ///
    /// On CDA-backed ECUs this translates to UDS `0x22
    /// ReadDataByIdentifier`. On native servers it reads from the local
    /// MDD-backed provider. The returned [`ReadValue`] carries the already
    /// decoded value; embedded schema (`?include-schema=true`) is handled
    /// at the HTTP layer.
    fn read_data(
        &self,
        data_id: &str,
    ) -> impl std::future::Future<Output = Result<ReadValue>> + Send;

    /// Return entity capabilities (`GET /{collection}/{entity-id}`).
    fn entity_capabilities(
        &self,
    ) -> impl std::future::Future<Output = Result<EntityCapabilities>> + Send;
}
