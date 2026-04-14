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

use crate::spec::{
    component::EntityCapabilities,
    fault::{FaultFilter, ListOfFaults},
    operation::{StartExecutionAsyncResponse, StartExecutionRequest},
};
use crate::types::{component::ComponentId, error::Result};

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

    /// See [`SovdServer::clear_all_faults`](crate::traits::server::SovdServer::clear_all_faults).
    async fn clear_all_faults(&self) -> Result<()>;

    /// See [`SovdServer::clear_fault`](crate::traits::server::SovdServer::clear_fault).
    async fn clear_fault(&self, code: &str) -> Result<()>;

    /// See [`SovdServer::start_execution`](crate::traits::server::SovdServer::start_execution).
    async fn start_execution(
        &self,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse>;

    /// See [`SovdServer::entity_capabilities`](crate::traits::server::SovdServer::entity_capabilities).
    async fn entity_capabilities(&self) -> Result<EntityCapabilities>;
}
