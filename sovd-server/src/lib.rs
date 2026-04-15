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

// ADR-0018 D7: deny expect_used on production backend code;
// workspace already denies unwrap_used. Tests keep both for
// readability.
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

//! HTTP/REST SOVD server for the Eclipse `OpenSOVD` core stack.
//!
//! Phase 0 boots a bare `GET /sovd/v1/health` endpoint via [`app`]. Phase 1
//! adds the in-memory MVP server via [`in_memory::InMemoryServer`] +
//! [`routes::app_with_server`], which wires spec-typed route handlers for
//! the five MVP use cases (faults, operations, components, data) against
//! canned demo data. The real DFM-backed server lands in Phase 3/4.
//!
//! See [`ARCHITECTURE.md`](../../ARCHITECTURE.md) for role boundaries.

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

pub mod auth;
pub mod backends;
pub mod correlation;
pub mod in_memory;
pub mod openapi;
pub mod routes;

pub use auth::{AuthConfig, BearerToken};
pub use backends::CdaBackend;
pub use correlation::CorrelationId;
pub use in_memory::{InMemoryComponentServer, InMemoryServer};

/// Build a bare-bones SOVD HTTP router that only exposes the health
/// endpoint. Used when `sovd-main` is configured with `server.mode =
/// "hello_world"`.
pub fn app() -> Router {
    Router::new().route("/sovd/v1/health", get(health))
}

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[cfg(test)]
mod tests {
    use axum::Json;

    use super::{app, health};

    #[tokio::test]
    async fn health_returns_ok_status() {
        let Json(body) = health().await;
        assert_eq!(body.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert_eq!(
            body.get("version").and_then(|v| v.as_str()),
            Some(env!("CARGO_PKG_VERSION")),
        );
    }

    #[test]
    fn router_builds() {
        let _router = app();
    }
}
