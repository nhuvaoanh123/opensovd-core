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

//! Axum route handlers for the in-memory MVP SOVD server.
//!
//! The public entry point is [`app_with_server`], which mounts every MVP
//! endpoint against an [`Arc<InMemoryServer>`](crate::InMemoryServer)
//! state. All handlers take typed spec DTOs from
//! [`sovd_interfaces::spec`] on the way in and return typed spec DTOs on
//! the way out; the HTTP layer never manipulates raw JSON.
//!
//! ## Per-component vs. multi-component
//!
//! [`SovdServer`](sovd_interfaces::traits::server::SovdServer) is a
//! per-component trait. These routes hold a multi-component
//! [`InMemoryServer`] and dispatch to a per-component view
//! ([`InMemoryComponentServer`](crate::InMemoryComponentServer)) on every
//! request based on the `{component-id}` path segment. That keeps the
//! axum `State` concrete (not `Arc<dyn SovdServer>`, which is not
//! dyn-safe with native `async fn in trait`) while preserving ADR-0015's
//! rule that all boundary types come from `sovd-interfaces::spec`.

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};

use crate::InMemoryServer;

pub mod components;
pub mod error;
pub mod faults;
pub mod health;
pub mod operations;

/// Build the full MVP router for `server`, mounting the health endpoint
/// plus every in-scope SOVD entity route.
pub fn app_with_server(server: Arc<InMemoryServer>) -> Router {
    Router::new()
        .route("/sovd/v1/health", get(health::health))
        .route("/sovd/v1/components", get(components::list_components))
        .route(
            "/sovd/v1/components/{component_id}",
            get(components::get_component),
        )
        .route(
            "/sovd/v1/components/{component_id}/faults",
            get(faults::list_faults).delete(faults::clear_all_faults),
        )
        .route(
            "/sovd/v1/components/{component_id}/faults/{fault_code}",
            get(faults::get_fault).delete(faults::clear_fault),
        )
        .route(
            "/sovd/v1/components/{component_id}/operations",
            get(operations::list_operations),
        )
        .route(
            "/sovd/v1/components/{component_id}/operations/{operation_id}/executions",
            post(operations::start_execution),
        )
        .route(
            "/sovd/v1/components/{component_id}/operations/{operation_id}/executions/{execution_id}",
            get(operations::execution_status),
        )
        .with_state(server)
}
