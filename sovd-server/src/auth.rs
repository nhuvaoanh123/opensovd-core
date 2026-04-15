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

//! Bearer-token authentication middleware (Phase 4 Line A D5).
//!
//! Per ADR-0009 the scaffolded auth model is bearer + mTLS. Phase 4
//! ships only the bearer side; mTLS follows in a later phase once the
//! CDA transport work is merged upstream. The middleware:
//!
//! - Rejects requests with no `Authorization` header or a non-bearer
//!   scheme with 401 + a spec [`GenericError`] body.
//! - Accepts requests whose bearer token appears in [`AuthConfig::accepted_tokens`].
//! - Stores the accepted token in `request.extensions_mut()` as
//!   [`BearerToken`] so downstream handlers (e.g. the CDA forwarder)
//!   can propagate it.
//!
//! Token validation is deliberately trivial — a constant-time string
//! compare against a list of accepted tokens. The production auth flow
//! (JWT introspection, OAuth2 scopes, mTLS peer identity) lands in a
//! later phase per ADR-0009.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sovd_interfaces::spec::error::GenericError;

/// Accepted bearer token stored in the request extensions for
/// downstream handlers. Opaque string per ADR-0009 §"scaffold bearer".
#[derive(Debug, Clone)]
pub struct BearerToken(pub String);

/// Runtime configuration for [`middleware`].
#[derive(Debug, Clone, Default)]
pub struct AuthConfig {
    accepted_tokens: Vec<String>,
}

impl AuthConfig {
    /// Build from a static list of accepted tokens. Tests pass a
    /// single fixed token; production reads from the `[auth.tokens]`
    /// TOML key (Phase 4 does not yet read this — the config surface
    /// lands alongside mTLS in a later phase).
    #[must_use]
    pub fn new(accepted_tokens: Vec<String>) -> Self {
        Self { accepted_tokens }
    }

    /// Return `true` if `token` is in the accepted list. Constant-time
    /// compare over the list to avoid token-identity timing leaks.
    #[must_use]
    pub fn is_accepted(&self, token: &str) -> bool {
        // Compare every accepted token so early-return does not leak
        // whether an accepted token exists with a matching prefix.
        let mut hit = false;
        for candidate in &self.accepted_tokens {
            if constant_time_eq(candidate.as_bytes(), token.as_bytes()) {
                hit = true;
            }
        }
        hit
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

/// Extract the bearer token from an `Authorization` header value.
///
/// Returns `None` if the header is missing, the value is not UTF-8,
/// or the scheme is not `Bearer` (case-insensitive).
fn extract_bearer(header_value: &str) -> Option<&str> {
    let mut parts = header_value.splitn(2, char::is_whitespace);
    let scheme = parts.next()?;
    let token = parts.next()?.trim();
    if scheme.eq_ignore_ascii_case("Bearer") && !token.is_empty() {
        Some(token)
    } else {
        None
    }
}

/// Axum middleware that enforces bearer authentication per
/// [`AuthConfig`]. On success the token is stored in the request
/// extensions as [`BearerToken`]; on failure a 401 + spec
/// [`GenericError`] body is returned without calling `next`.
pub async fn middleware(
    State(auth): State<Arc<AuthConfig>>,
    mut request: Request,
    next: Next,
) -> Response {
    let token: Option<String> = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(extract_bearer)
        .map(ToOwned::to_owned);
    let Some(token) = token else {
        return unauthorized("missing or malformed Authorization header");
    };
    if !auth.is_accepted(&token) {
        return unauthorized("bearer token not accepted");
    }
    request.extensions_mut().insert(BearerToken(token));
    next.run(request).await
}

fn unauthorized(message: &str) -> Response {
    let body = GenericError {
        error_code: "auth.unauthorized".into(),
        vendor_code: None,
        message: message.to_owned(),
        translation_id: None,
        parameters: None,
    };
    (StatusCode::UNAUTHORIZED, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_bearer_accepts_well_formed_header() {
        assert_eq!(extract_bearer("Bearer abc123"), Some("abc123"));
        assert_eq!(extract_bearer("bearer abc123"), Some("abc123"));
    }

    #[test]
    fn extract_bearer_rejects_non_bearer_scheme() {
        assert_eq!(extract_bearer("Basic abc"), None);
        assert_eq!(extract_bearer("Token abc"), None);
    }

    #[test]
    fn extract_bearer_rejects_empty_token() {
        assert_eq!(extract_bearer("Bearer "), None);
    }

    #[test]
    fn auth_config_accepts_listed_tokens() {
        let cfg = AuthConfig::new(vec!["alpha".into(), "beta".into()]);
        assert!(cfg.is_accepted("alpha"));
        assert!(cfg.is_accepted("beta"));
        assert!(!cfg.is_accepted("gamma"));
    }

    #[test]
    fn constant_time_eq_is_length_sensitive() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"abc", b"abd"));
    }
}
