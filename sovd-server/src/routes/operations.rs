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

//! Operations endpoints — `/sovd/v1/components/{id}/operations` and the
//! executions sub-collection.
//!
//! Mirrors `operations/operations.yaml` from the spec (see
//! `docs/openapi-audit-2026-04-14.md` §5.3).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sovd_interfaces::{
    ComponentId,
    spec::operation::{
        ExecutionStatusResponse, OperationsList, StartExecutionAsyncResponse, StartExecutionRequest,
    },
};

use crate::{InMemoryServer, routes::error::ApiError};

/// `GET /sovd/v1/components/{component_id}/operations` — list operations.
///
/// # Errors
///
/// Returns 404 if the component is unknown.
#[utoipa::path(
    get,
    path = "/sovd/v1/components/{component_id}/operations",
    operation_id = "listOperations",
    tag = "operations-control",
    params(
        ("component_id" = String, Path, description = "Stable component identifier"),
    ),
    responses(
        (status = 200, description = "List of operations", body = OperationsList),
        (status = 404, description = "Component not found"),
    ),
)]
pub async fn list_operations(
    State(server): State<Arc<InMemoryServer>>,
    Path(component_id): Path<String>,
) -> Result<Json<OperationsList>, ApiError> {
    let component = ComponentId::new(component_id);
    Ok(Json(server.dispatch_list_operations(&component).await?))
}

/// `POST /sovd/v1/components/{component_id}/operations/{operation_id}/executions`
/// — start an execution. Always returns 202 async in the MVP.
///
/// # Errors
///
/// Returns 404 if the component or operation id is unknown.
#[utoipa::path(
    post,
    path = "/sovd/v1/components/{component_id}/operations/{operation_id}/executions",
    operation_id = "startExecution",
    tag = "operations-control",
    params(
        ("component_id" = String, Path, description = "Stable component identifier"),
        ("operation_id" = String, Path, description = "SOVD operation id"),
    ),
    request_body = StartExecutionRequest,
    responses(
        (
            status = 202,
            description = "Async execution started",
            body = StartExecutionAsyncResponse,
        ),
        (status = 404, description = "Component or operation not found"),
    ),
)]
pub async fn start_execution(
    State(server): State<Arc<InMemoryServer>>,
    Path((component_id, operation_id)): Path<(String, String)>,
    Json(request): Json<StartExecutionRequest>,
) -> Result<Response, ApiError> {
    let component = ComponentId::new(component_id);
    let started = server
        .dispatch_start_execution(&component, &operation_id, request)
        .await?;
    Ok((StatusCode::ACCEPTED, Json(started)).into_response())
}

/// `GET /sovd/v1/components/{component_id}/operations/{operation_id}/executions/{execution_id}`
/// — look up current execution status.
///
/// # Errors
///
/// Returns 404 if the execution id is unknown or belongs to a different
/// operation than `operation_id`.
#[utoipa::path(
    get,
    path = "/sovd/v1/components/{component_id}/operations/{operation_id}/executions/{execution_id}",
    operation_id = "getExecutionStatus",
    tag = "operations-control",
    params(
        ("component_id" = String, Path, description = "Stable component identifier"),
        ("operation_id" = String, Path, description = "SOVD operation id"),
        ("execution_id" = String, Path, description = "Per-execution identifier (UUID)"),
    ),
    responses(
        (status = 200, description = "Execution status", body = ExecutionStatusResponse),
        (status = 404, description = "Execution not found"),
    ),
)]
pub async fn execution_status(
    State(server): State<Arc<InMemoryServer>>,
    Path((component_id, operation_id, execution_id)): Path<(String, String, String)>,
) -> Result<Json<ExecutionStatusResponse>, ApiError> {
    let component = ComponentId::new(component_id);
    Ok(Json(
        server
            .dispatch_execution_status(&component, &operation_id, &execution_id)
            .await?,
    ))
}
