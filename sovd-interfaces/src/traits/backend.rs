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

//! [`SovdBackend`] — what `sovd-gateway` routes to.
//!
//! A backend is anything that can answer SOVD requests for one logical
//! component. This is the abstraction that lets `sovd-gateway` treat
//! native SOVD servers, the Classic Diagnostic Adapter, and federated
//! gateway hops uniformly.
//!
//! See [`ARCHITECTURE.md`](../../../ARCHITECTURE.md) §"`SovdBackend`".
//!
//! # Why `async-trait`
//!
//! `SovdBackend` is intended to be stored behind `dyn` in the gateway's
//! backend registry (`Vec<Box<dyn SovdBackend + Send + Sync>>`). Stable
//! `async fn in trait` is not dyn-safe for `Send` futures without the
//! `#[async_trait]` attribute, so we apply it here. The per-ECU
//! [`SovdServer`](crate::traits::server::SovdServer) uses native
//! `async fn in trait` because it is used generically, not behind `dyn`.

use async_trait::async_trait;

use crate::{
    spec::{
        component::EntityCapabilities,
        data::Datas,
        fault::{FaultDetails, FaultFilter, ListOfFaults},
        operation::{
            ExecutionStatusResponse, OperationsList, StartExecutionAsyncResponse,
            StartExecutionRequest,
        },
    },
    types::{
        component::ComponentId,
        error::{Result, SovdError},
    },
};

/// Which kind of backend a given [`SovdBackend`] is. Used by the gateway
/// for routing decisions, metrics, and admin endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// Diagnostic Fault Manager (`sovd-dfm`).
    Dfm,
    /// Classic Diagnostic Adapter — legacy UDS ECU via DoIP/CAN.
    Cda,
    /// Native SOVD server (`sovd-server`).
    NativeSovd,
    /// Another `sovd-gateway` reached over HTTPS.
    Federated,
}

/// A routing target for [`SovdGateway`](crate::traits::gateway::SovdGateway).
///
/// Implementors live in whichever crate owns the concrete backend kind:
///
/// - `sovd-dfm` implements `SovdBackend` for `BackendKind::Dfm`.
/// - `sovd-server` implements it for `BackendKind::NativeSovd`.
/// - A future CDA-adapter shim implements it for `BackendKind::Cda`.
/// - A future federated-gateway adapter implements it for
///   `BackendKind::Federated`.
#[async_trait]
pub trait SovdBackend: Send + Sync {
    /// Which component this backend handles. Each `ComponentId` routes to
    /// at most one backend — registering a duplicate is an
    /// [`SovdError::InvalidRequest`](crate::SovdError::InvalidRequest) at
    /// the gateway.
    fn component_id(&self) -> ComponentId;

    /// Kind discriminator — see [`BackendKind`].
    fn kind(&self) -> BackendKind;

    /// See [`SovdServer::list_faults`](crate::traits::server::SovdServer::list_faults).
    async fn list_faults(&self, filter: FaultFilter) -> Result<ListOfFaults>;

    /// See [`SovdServer::get_fault`](crate::traits::server::SovdServer::get_fault).
    ///
    /// Backends that cannot return per-fault detail (e.g. a legacy CDA
    /// UDS forwarder that only exposes aggregate clear) should return
    /// [`SovdError::BackendUnavailable`] with a reason string. The default
    /// impl does exactly that so legacy backends compile unchanged until
    /// they opt in.
    async fn get_fault(&self, code: &str) -> Result<FaultDetails> {
        let _ = code;
        Err(SovdError::InvalidRequest(
            "backend does not implement get_fault".to_owned(),
        ))
    }

    /// See [`SovdServer::clear_all_faults`](crate::traits::server::SovdServer::clear_all_faults).
    async fn clear_all_faults(&self) -> Result<()>;

    /// See [`SovdServer::clear_fault`](crate::traits::server::SovdServer::clear_fault).
    async fn clear_fault(&self, code: &str) -> Result<()>;

    /// List the operations catalog published by this backend. Default
    /// returns an empty catalog so legacy backends compile unchanged.
    async fn list_operations(&self) -> Result<OperationsList> {
        Ok(OperationsList {
            items: Vec::new(),
            schema: None,
        })
    }

    /// See [`SovdServer::start_execution`](crate::traits::server::SovdServer::start_execution).
    async fn start_execution(
        &self,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse>;

    /// See [`SovdServer::execution_status`](crate::traits::server::SovdServer::execution_status).
    ///
    /// Default returns [`SovdError::BackendUnavailable`] so legacy
    /// backends that do not track per-execution state compile unchanged.
    async fn execution_status(
        &self,
        operation_id: &str,
        execution_id: &str,
    ) -> Result<ExecutionStatusResponse> {
        let _ = (operation_id, execution_id);
        Err(SovdError::InvalidRequest(
            "backend does not track operation executions".to_owned(),
        ))
    }

    /// List the data catalog (metadata only) this backend exposes at
    /// `GET .../components/{id}/data`. Default returns an empty catalog.
    async fn list_data(&self) -> Result<Datas> {
        Ok(Datas {
            items: Vec::new(),
            schema: None,
        })
    }

    /// See [`SovdServer::entity_capabilities`](crate::traits::server::SovdServer::entity_capabilities).
    async fn entity_capabilities(&self) -> Result<EntityCapabilities>;

    /// Probe the backend for liveness / readiness. Default returns
    /// [`BackendHealth::Ok`] so backends that do not implement a probe
    /// still flow through `/health`.
    async fn health_probe(&self) -> BackendHealth {
        BackendHealth::Ok
    }

    /// If the backend tracks an operation cycle, return the name of
    /// the currently active cycle. Default `None` — only DFM-style
    /// backends that own an
    /// [`OperationCycle`](crate::traits::operation_cycle::OperationCycle)
    /// override this.
    async fn current_operation_cycle(&self) -> Option<String> {
        None
    }
}

/// Result of a [`SovdBackend::health_probe`] call.
///
/// Carried in [`crate::extras::health::HealthStatus`] — the Phase 4
/// extras-level health envelope reported by `GET /sovd/v1/health`. Per
/// ADR-0015, this is an extras type (not spec) because ISO 17978-3 has
/// no notion of a per-backend probe result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BackendHealth {
    /// Backend responded to a probe without error.
    Ok,
    /// Backend responded but is degraded — includes a short reason.
    Degraded {
        /// Free-form human reason for the degraded state.
        reason: String,
    },
    /// Backend did not respond to the probe.
    Unavailable {
        /// Free-form human reason for the failure.
        reason: String,
    },
}
