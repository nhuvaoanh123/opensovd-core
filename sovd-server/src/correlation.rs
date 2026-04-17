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

//! Correlation-id middleware (Phase 4 Line A D5).
//!
//! Per ADR-0013, SOVD-Core accepts **both** `X-Request-Id` and
//! `traceparent` as the incoming correlation id header. The middleware:
//!
//! 1. Reads `X-Request-Id` from the incoming request if present.
//! 2. Otherwise reads `traceparent` and derives a request-id from its
//!    trace-id field (`00-<trace-id>-<span-id>-<flags>`) so downstream
//!    handlers always see a stable, log-friendly string.
//! 3. Otherwise synthesises a fresh uuid.
//! 4. Stores the resolved id in `request.extensions_mut()` as
//!    [`CorrelationId`].
//! 5. Echoes the id back to the caller via the `x-request-id`
//!    response header.
//!
//! Downstream HTTP clients (e.g. `CdaBackend`) can reach into the
//! request extensions to pull `CorrelationId` and propagate it on
//! their outbound reqwest calls. That wiring is in the `CdaBackend`
//! forwarding path, not here — the middleware only stores.

use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue, header::HeaderMap},
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

/// The `x-request-id` header name — a canonical const so middleware
/// and downstream callers never rely on stringly-typed access.
pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// The `traceparent` header name.
pub const TRACEPARENT_HEADER: HeaderName = HeaderName::from_static("traceparent");

/// Correlation id stored in request extensions.
#[derive(Debug, Clone)]
pub struct CorrelationId(pub String);

/// Axum middleware that materialises a correlation id for every
/// request and echoes it back on the response.
pub async fn middleware(mut request: Request, next: Next) -> Response {
    let id = resolve_correlation_id(request.headers());
    request.extensions_mut().insert(CorrelationId(id.clone()));
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}

/// Resolve an incoming correlation id from headers, falling back to a
/// freshly generated UUID if neither `X-Request-Id` nor `traceparent`
/// is present.
pub fn resolve_correlation_id(headers: &HeaderMap) -> String {
    if let Some(value) = headers.get(REQUEST_ID_HEADER).and_then(|v| v.to_str().ok()) {
        if !value.is_empty() {
            return value.to_owned();
        }
    }
    if let Some(value) = headers
        .get(TRACEPARENT_HEADER)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(trace_id) = parse_traceparent_trace_id(value) {
            return trace_id.to_owned();
        }
    }
    Uuid::new_v4().to_string()
}

/// Parse the `trace-id` field out of a W3C traceparent header.
///
/// Expected format: `version-trace_id-span_id-flags` where version is
/// two hex chars, trace_id is 32 hex chars, span_id is 16 hex chars,
/// and flags is two hex chars. Anything that does not match returns
/// `None`.
fn parse_traceparent_trace_id(value: &str) -> Option<&str> {
    let mut parts = value.split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let span_id = parts.next()?;
    let flags = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if version.len() != 2 || trace_id.len() != 32 || span_id.len() != 16 || flags.len() != 2 {
        return None;
    }
    if !version.chars().all(|c| c.is_ascii_hexdigit())
        || !trace_id.chars().all(|c| c.is_ascii_hexdigit())
        || !span_id.chars().all(|c| c.is_ascii_hexdigit())
        || !flags.chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    Some(trace_id)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn parse_traceparent_happy_path() {
        let id =
            parse_traceparent_trace_id("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01");
        assert_eq!(id, Some("0af7651916cd43dd8448eb211c80319c"));
    }

    #[test]
    fn parse_traceparent_rejects_malformed() {
        assert_eq!(parse_traceparent_trace_id("not-a-trace"), None);
        assert_eq!(parse_traceparent_trace_id(""), None);
        assert_eq!(parse_traceparent_trace_id("00-abc-abc-01"), None);
    }

    #[test]
    fn resolve_prefers_request_id_over_traceparent() {
        let mut headers = HeaderMap::new();
        headers.insert(REQUEST_ID_HEADER, HeaderValue::from_static("my-req-id"));
        headers.insert(
            TRACEPARENT_HEADER,
            HeaderValue::from_static("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
        );
        assert_eq!(resolve_correlation_id(&headers), "my-req-id");
    }

    #[test]
    fn resolve_falls_back_to_traceparent_when_no_request_id() {
        let mut headers = HeaderMap::new();
        headers.insert(
            TRACEPARENT_HEADER,
            HeaderValue::from_static("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
        );
        assert_eq!(
            resolve_correlation_id(&headers),
            "0af7651916cd43dd8448eb211c80319c"
        );
    }

    #[test]
    fn resolve_synthesises_uuid_when_neither_header() {
        let headers = HeaderMap::new();
        let id = resolve_correlation_id(&headers);
        assert_eq!(id.len(), 36, "expected UUID, got: {id}");
    }
}
