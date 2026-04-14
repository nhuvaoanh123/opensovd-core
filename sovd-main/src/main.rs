// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
// SPDX-License-Identifier: Apache-2.0

//! Eclipse `OpenSOVD` core - main binary entry point.
//!
//! Phase 0 boots the `sovd-server` axum router on `0.0.0.0:8080` and serves
//! the single `GET /sovd/v1/health` endpoint. Real configuration, signal
//! handling, and gateway wiring land in later phases.

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let app = sovd_server::app();
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("OpenSOVD core listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
