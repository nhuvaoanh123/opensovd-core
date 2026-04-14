// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
// SPDX-License-Identifier: Apache-2.0

//! HTTP/REST SOVD server for the Eclipse `OpenSOVD` core stack.
//!
//! Phase 0 exposes only `GET /sovd/v1/health` to prove the axum + tokio
//! plumbing works end-to-end. Real SOVD entity routes land in Phase 3.

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

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
