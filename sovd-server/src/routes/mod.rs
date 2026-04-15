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
    middleware::from_fn_with_state,
    routing::{get, post},
};

use crate::{InMemoryServer, auth::AuthConfig, correlation};

pub mod components;
pub mod data;
pub mod error;
pub mod faults;
pub mod health;
pub mod operations;

/// Dev-only `GET /sovd/v1/openapi.json` — returns the generated `OpenAPI`
/// document as JSON. Gated behind `cfg(debug_assertions)` so release
/// binaries never expose it.
#[cfg(debug_assertions)]
pub async fn openapi_json() -> axum::Json<utoipa::openapi::OpenApi> {
    axum::Json(crate::openapi::openapi())
}

fn base_router() -> Router<Arc<InMemoryServer>> {
    let router = Router::new()
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
            "/sovd/v1/components/{component_id}/data",
            get(data::list_data),
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
        );

    #[cfg(debug_assertions)]
    let router = router.route("/sovd/v1/openapi.json", get(openapi_json));

    router
}

/// Build the full MVP router for `server`, mounting the health endpoint
/// plus every in-scope SOVD entity route. Debug builds additionally
/// expose `GET /sovd/v1/openapi.json` for spec-generation tooling.
///
/// This is the no-auth variant — every request is accepted. Use
/// [`app_with_auth`] when the caller needs bearer token enforcement.
/// Correlation-id middleware is applied in both variants (ADR-0013).
pub fn app_with_server(server: Arc<InMemoryServer>) -> Router {
    base_router()
        .with_state(server)
        .layer(axum::middleware::from_fn(correlation::middleware))
}

/// Build the full MVP router with bearer-token authentication and
/// correlation-id middleware. Per ADR-0009 + ADR-0013.
///
/// Requests must carry `Authorization: Bearer <token>` where `<token>`
/// is one of the accepted tokens in [`AuthConfig`]; `/sovd/v1/health`
/// is subject to the same enforcement as every other route in the
/// bearer path (Phase 4 does not carve out a health-liveness
/// exemption — the config can add one later).
pub fn app_with_auth(server: Arc<InMemoryServer>, auth: AuthConfig) -> Router {
    let auth_state = Arc::new(auth);
    base_router()
        .with_state(server)
        .layer(from_fn_with_state(auth_state, crate::auth::middleware))
        .layer(axum::middleware::from_fn(correlation::middleware))
}
