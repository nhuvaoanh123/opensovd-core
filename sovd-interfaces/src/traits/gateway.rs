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

//! [`SovdGateway`] — system-wide SOVD multiplexer.
//!
//! One per system. Accepts SOVD requests, resolves `ComponentId` to a
//! registered [`SovdBackend`], and forwards the call. Also aggregates
//! multi-component responses (e.g. "list DTCs across every component").
//!
//! See upstream
//! [`design.md`](../../../../opensovd/docs/design/design.md) §"SOVD Gateway".

use super::backend::SovdBackend;
use crate::{
    spec::fault::{Fault, FaultFilter},
    types::{component::ComponentId, error::Result},
};

/// System-wide SOVD gateway.
///
/// Backend registration is synchronous and non-async: it only mutates the
/// gateway's local routing table. All request-dispatching methods are
/// async because they cross the backend trait boundary.
pub trait SovdGateway: Send + Sync {
    /// Register a new backend.
    ///
    /// Returns
    /// [`SovdError::InvalidRequest`](crate::SovdError::InvalidRequest) if
    /// a backend for the same [`ComponentId`] is already registered.
    ///
    /// # Errors
    ///
    /// - duplicate `ComponentId` already registered
    fn register_backend(&mut self, backend: Box<dyn SovdBackend + Send + Sync>) -> Result<()>;

    /// Iterate over all currently registered backends. Order is
    /// implementation-defined but stable within a single gateway instance.
    fn backends(&self) -> Box<dyn Iterator<Item = &(dyn SovdBackend + Send + Sync)> + '_>;

    /// Fan-out `list_faults` across every backend and tag each fault with
    /// its originating [`ComponentId`].
    ///
    /// Implementations should call backends concurrently (e.g. via
    /// `futures::future::join_all`). A single backend failure does **not**
    /// fail the whole call — backends that error out are omitted and
    /// logged. Only a total failure (all backends errored) returns an
    /// error.
    fn list_all_faults(
        &self,
        filter: FaultFilter,
    ) -> impl std::future::Future<Output = Result<Vec<(ComponentId, Fault)>>> + Send;

    /// Look up the backend for a specific component.
    ///
    /// Returns [`SovdError::NotFound`](crate::SovdError::NotFound) if no
    /// backend is registered for `target`.
    fn route(
        &self,
        target: ComponentId,
    ) -> impl std::future::Future<Output = Result<&(dyn SovdBackend + Send + Sync)>> + Send;
}
