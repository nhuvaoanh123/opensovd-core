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

#![allow(clippy::doc_markdown)]

//! `GET /sovd/v1/health` — SOVD liveness + backend probe (Phase 4 D4).
//!
//! Returns a [`HealthStatus`] extras envelope describing the top-level
//! status, the server version, per-backend probe results, and the
//! currently active operation cycle (if any). The probe fans out to
//! every forward backend registered with [`InMemoryServer`] via
//! [`InMemoryServer::probe_forwards`]; no-forward deployments always
//! report `sovd_db: Ok` because the in-memory server does not sit
//! behind a real store.

use std::sync::Arc;

use axum::{Json, extract::State};
use sovd_interfaces::{extras::health::HealthStatus, traits::backend::BackendHealth};

use crate::InMemoryServer;

/// Return the Phase 4 health envelope.
pub async fn health(State(server): State<Arc<InMemoryServer>>) -> Json<HealthStatus> {
    let forwards_health = server.probe_forwards().await;
    let operation_cycle = server.observe_cycle_name().await;
    Json(HealthStatus {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        sovd_db: forwards_health.clone(),
        fault_sink: match &forwards_health {
            BackendHealth::Ok => BackendHealth::Ok,
            other => other.clone(),
        },
        operation_cycle,
    })
}
