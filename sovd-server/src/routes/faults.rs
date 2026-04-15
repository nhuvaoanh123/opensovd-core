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

//! Fault endpoints — `/sovd/v1/components/{id}/faults` and friends.
//!
//! Mirrors the spec path table `faults/faults.yaml` (see
//! `docs/openapi-audit-2026-04-14.md` §5.2). For the MVP the three filter
//! query parameters (`status[key]`, `severity`, `scope`) are parsed by
//! hand — we deliberately do not pull in `serde_qs` for a single use case.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use sovd_interfaces::{
    ComponentId,
    spec::fault::{FaultDetails, FaultFilter, ListOfFaults},
};

// Forward faults go through `InMemoryServer::dispatch_*` helpers so
// route handlers never have to branch on local-vs-forwarded themselves.

use crate::{InMemoryServer, routes::error::ApiError};

/// Query parameters for `GET .../faults`.
///
/// The spec allows three independent filters. We expose them flat on the
/// query string (`?severity=3&scope=Default&status_key=aggregatedStatus:active`)
/// for the MVP. Full `serde_qs`-style nested encoding lands when we wire
/// the real client.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct FaultQuery {
    /// Severity upper bound (exclusive, per spec).
    pub severity: Option<i32>,
    /// Scope string match.
    pub scope: Option<String>,
    /// `key:value` pairs used to filter by status. Repeatable.
    #[serde(rename = "status_key")]
    pub status_key: Option<String>,
}

impl FaultQuery {
    fn into_filter(self) -> FaultFilter {
        let mut status_keys = Vec::new();
        if let Some(raw) = self.status_key {
            if let Some((k, v)) = raw.split_once(':') {
                status_keys.push((k.to_owned(), v.to_owned()));
            }
        }
        FaultFilter {
            status_keys,
            severity: self.severity,
            scope: self.scope,
        }
    }
}

/// `GET /sovd/v1/components/{component_id}/faults` — list faults.
///
/// # Errors
///
/// Returns 404 if the component is unknown; other
/// [`SovdError`](sovd_interfaces::SovdError) values are mapped via
/// [`ApiError`].
#[utoipa::path(
    get,
    path = "/sovd/v1/components/{component_id}/faults",
    operation_id = "getFaults",
    tag = "fault-handling",
    params(
        ("component_id" = String, Path, description = "Stable component identifier"),
        ("severity" = Option<i32>, Query, description = "Severity upper bound (exclusive)"),
        ("scope" = Option<String>, Query, description = "Scope string"),
        ("status_key" = Option<String>, Query, description = "Status filter in key:value form"),
    ),
    responses(
        (status = 200, description = "List of faults", body = ListOfFaults),
        (status = 404, description = "Component not found"),
    ),
)]
pub async fn list_faults(
    State(server): State<Arc<InMemoryServer>>,
    Path(component_id): Path<String>,
    Query(query): Query<FaultQuery>,
) -> Result<Json<ListOfFaults>, ApiError> {
    let component = ComponentId::new(component_id);
    Ok(Json(
        server
            .dispatch_list_faults(&component, query.into_filter())
            .await?,
    ))
}

/// `GET /sovd/v1/components/{component_id}/faults/{fault_code}` — fault
/// details for one code.
///
/// # Errors
///
/// Returns 404 if the component or fault code is unknown.
#[utoipa::path(
    get,
    path = "/sovd/v1/components/{component_id}/faults/{fault_code}",
    operation_id = "getFaultById",
    tag = "fault-handling",
    params(
        ("component_id" = String, Path, description = "Stable component identifier"),
        ("fault_code" = String, Path, description = "Native fault code (string)"),
    ),
    responses(
        (status = 200, description = "Fault details", body = FaultDetails),
        (status = 404, description = "Fault not found"),
    ),
)]
pub async fn get_fault(
    State(server): State<Arc<InMemoryServer>>,
    Path((component_id, fault_code)): Path<(String, String)>,
) -> Result<Json<FaultDetails>, ApiError> {
    // Phase 4 D1: dispatch through the forward map so DFM-served
    // components can answer per-fault detail. See ADR-0015 §"backend
    // trait surface" for the extended `SovdBackend::get_fault` method.
    let component = ComponentId::new(component_id);
    Ok(Json(
        server.dispatch_get_fault(&component, &fault_code).await?,
    ))
}

/// `DELETE /sovd/v1/components/{component_id}/faults` — clear every fault.
///
/// # Errors
///
/// Returns 404 if the component is unknown.
#[utoipa::path(
    delete,
    path = "/sovd/v1/components/{component_id}/faults",
    operation_id = "deleteAllFaults",
    tag = "fault-handling",
    params(
        ("component_id" = String, Path, description = "Stable component identifier"),
    ),
    responses(
        (status = 204, description = "All faults cleared"),
        (status = 404, description = "Component not found"),
    ),
)]
pub async fn clear_all_faults(
    State(server): State<Arc<InMemoryServer>>,
    Path(component_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let component = ComponentId::new(component_id);
    server.dispatch_clear_all_faults(&component).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /sovd/v1/components/{component_id}/faults/{fault_code}` — clear
/// one fault.
///
/// # Errors
///
/// Returns 404 if the component or fault code is unknown.
#[utoipa::path(
    delete,
    path = "/sovd/v1/components/{component_id}/faults/{fault_code}",
    operation_id = "deleteFaultById",
    tag = "fault-handling",
    params(
        ("component_id" = String, Path, description = "Stable component identifier"),
        ("fault_code" = String, Path, description = "Native fault code (string)"),
    ),
    responses(
        (status = 204, description = "Fault cleared"),
        (status = 404, description = "Fault not found"),
    ),
)]
pub async fn clear_fault(
    State(server): State<Arc<InMemoryServer>>,
    Path((component_id, fault_code)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let component = ComponentId::new(component_id);
    server.dispatch_clear_fault(&component, &fault_code).await?;
    Ok(StatusCode::NO_CONTENT)
}
