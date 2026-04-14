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

use crate::types::{
    component::{ComponentId, ComponentInfo},
    dtc::{Dtc, DtcGroup, DtcStatusMask},
    error::Result,
    routine::RoutineId,
};

/// Outbound SOVD REST client.
pub trait SovdClient: Send + Sync {
    /// `GET /sovd/v1/components/{component}/faults` with the given status
    /// mask filter. See
    /// [`SovdServer::list_dtcs`](crate::traits::server::SovdServer::list_dtcs)
    /// for filter semantics.
    fn list_dtcs(
        &self,
        component: ComponentId,
        filter: DtcStatusMask,
    ) -> impl std::future::Future<Output = Result<Vec<Dtc>>> + Send;

    /// `POST /sovd/v1/components/{component}/faults/clear` with an optional
    /// group filter. See
    /// [`SovdServer::clear_dtcs`](crate::traits::server::SovdServer::clear_dtcs)
    /// for semantics.
    fn clear_dtcs(
        &self,
        component: ComponentId,
        filter: Option<DtcGroup>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// `POST /sovd/v1/components/{component}/operations/{id}/start` with
    /// raw argument bytes. Returns after the ECU has accepted the routine
    /// start; poll the server for status.
    fn start_routine(
        &self,
        component: ComponentId,
        id: RoutineId,
        args: &[u8],
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// `GET /sovd/v1/components/{component}`.
    fn component_info(
        &self,
        component: ComponentId,
    ) -> impl std::future::Future<Output = Result<ComponentInfo>> + Send;
}
