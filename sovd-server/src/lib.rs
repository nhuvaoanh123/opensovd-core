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

//! HTTP/REST SOVD server for the Eclipse `OpenSOVD` core stack.
//!
//! Phase 0 exposes only `GET /sovd/v1/health` to prove the axum + tokio
//! plumbing works end-to-end. Real SOVD entity routes land in Phase 3.
//!
//! See [`ARCHITECTURE.md`](../../ARCHITECTURE.md) for role boundaries. This
//! crate serves **one component** — system-wide multiplexing is
//! `sovd-gateway`'s job.

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

/// SOVD Server instance for a single ECU / component.
///
/// Will implement [`sovd_interfaces::traits::server::SovdServer`] in
/// Phase 3. Fields (component metadata, DFM handle, MDD provider,
/// routine registry) are added then.
pub struct Server {
    // fields added in Phase 3
}

/// Build the SOVD HTTP router.
///
/// Phase 0 only mounts the health endpoint.
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
