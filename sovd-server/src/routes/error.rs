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

//! HTTP error mapping: [`SovdError`] → (`StatusCode`, `GenericError`).
//!
//! Per ADR-0015, [`SovdError`] is the internal Rust error type used inside
//! trait method signatures. At the HTTP layer every error is mapped onto
//! the spec-defined [`GenericError`] wire envelope and paired with a
//! status code. Route handlers return [`ApiError`] and let axum's
//! [`IntoResponse`] machinery do the rest.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sovd_interfaces::{SovdError, spec::error::GenericError};

/// Wire-level error response — status plus spec-derived body.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    body: GenericError,
}

impl ApiError {
    /// Construct from a raw status + body pair.
    #[must_use]
    pub fn new(status: StatusCode, body: GenericError) -> Self {
        Self { status, body }
    }
}

impl From<SovdError> for ApiError {
    fn from(err: SovdError) -> Self {
        let (status, error_code) = match &err {
            SovdError::NotFound { .. } => (StatusCode::NOT_FOUND, "resource.not_found"),
            SovdError::InvalidRequest(_) => (StatusCode::BAD_REQUEST, "request.invalid"),
            SovdError::Unauthorized => (StatusCode::UNAUTHORIZED, "auth.unauthorized"),
            SovdError::BackendUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, "backend.unavailable")
            }
            SovdError::OperationFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, "operation.failed")
            }
            SovdError::Transport(_) => (StatusCode::BAD_GATEWAY, "transport.error"),
            SovdError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal.error"),
        };
        Self::new(
            status,
            GenericError {
                error_code: error_code.to_owned(),
                vendor_code: None,
                message: err.to_string(),
                translation_id: None,
                parameters: None,
            },
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}
