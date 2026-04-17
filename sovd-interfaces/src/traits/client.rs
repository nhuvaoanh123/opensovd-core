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

//! [`SovdClient`] — outbound SOVD REST caller.
//!
//! Used by off-board testers, on-board apps, cloud services, and by
//! `sovd-gateway` itself when a routed component lives on a downstream
//! native-SOVD ECU (federated topology). See upstream
//! [`design.md`](../../../../opensovd/docs/design/design.md) §"SOVD Client".
//!
//! Unlike [`SovdServer`](crate::traits::server::SovdServer), the client
//! trait takes a [`ComponentId`] on every call: one client instance can
//! address many components behind the same base URL.

use crate::{
    spec::{
        component::EntityCapabilities,
        fault::{FaultFilter, ListOfFaults},
        operation::{StartExecutionAsyncResponse, StartExecutionRequest},
    },
    types::{component::ComponentId, error::Result},
};

/// Outbound SOVD REST client.
pub trait SovdClient: Send + Sync {
    /// `GET /sovd/v1/components/{component}/faults` with the given filter.
    /// See
    /// [`SovdServer::list_faults`](crate::traits::server::SovdServer::list_faults)
    /// for filter semantics.
    fn list_faults(
        &self,
        component: ComponentId,
        filter: FaultFilter,
    ) -> impl std::future::Future<Output = Result<ListOfFaults>> + Send;

    /// `DELETE /sovd/v1/components/{component}/faults` — clear every fault.
    /// See
    /// [`SovdServer::clear_all_faults`](crate::traits::server::SovdServer::clear_all_faults).
    fn clear_all_faults(
        &self,
        component: ComponentId,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// `DELETE /sovd/v1/components/{component}/faults/{code}` — clear one
    /// fault.
    fn clear_fault(
        &self,
        component: ComponentId,
        code: &str,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// `POST /sovd/v1/components/{component}/operations/{operation_id}/executions`.
    /// Returns the spec-defined async-execution response (200 sync flow is
    /// out of scope for this MVP client).
    fn start_execution(
        &self,
        component: ComponentId,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> impl std::future::Future<Output = Result<StartExecutionAsyncResponse>> + Send;

    /// `GET /sovd/v1/components/{component}` — entity capabilities.
    fn entity_capabilities(
        &self,
        component: ComponentId,
    ) -> impl std::future::Future<Output = Result<EntityCapabilities>> + Send;
}
