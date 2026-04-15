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

//! Unified error type returned by every trait in `sovd-interfaces`.
//!
//! Downstream crates map this enum onto their own wire formats:
//!
//! - `sovd-server` maps it to SOVD HTTP status codes and error bodies.
//! - `sovd-gateway` forwards it upstream unchanged (possibly wrapped in
//!   `BackendUnavailable` if the routed-to backend is down).
//! - `sovd-client` deserializes SOVD HTTP error bodies into this enum.

use thiserror::Error;

use crate::types::component::ComponentId;

/// Unified result alias for SOVD trait methods.
pub type Result<T> = core::result::Result<T, SovdError>;

/// Every fallible trait in `sovd-interfaces` returns `Result<T, SovdError>`.
///
/// Variants are kept small and non-overlapping. If a new error class is
/// needed in Phase 3/4, prefer adding a variant here over widening an
/// existing one.
#[derive(Debug, Error)]
pub enum SovdError {
    /// A requested entity (component, DTC, routine, DID) was not found.
    #[error("not found: {entity}")]
    NotFound {
        /// What was being looked up, e.g. `"component \"bcm\""`.
        entity: String,
    },

    /// The request was structurally valid but semantically rejected.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// The backend for the given component is currently not reachable.
    #[error("backend unavailable for component: {0}")]
    BackendUnavailable(ComponentId),

    /// The caller lacks the required session/security level.
    #[error("unauthorized")]
    Unauthorized,

    /// An operation execution started but terminated with a failure.
    #[error("operation {id} failed: {reason}")]
    OperationFailed {
        /// SOVD operation id (string per spec).
        id: String,
        /// Vendor-supplied failure reason.
        reason: String,
    },

    /// Low-level transport error (HTTP, `DoIP`, CAN, socket, ...).
    #[error("transport error: {0}")]
    Transport(String),

    /// Catch-all for bugs inside an implementation. Prefer a specific
    /// variant above unless you truly have nowhere else to map.
    #[error("internal error: {0}")]
    Internal(String),

    /// Generic soft-fail per ADR-0018: the backend produced a usable
    /// response but in a degraded mode. The HTTP layer maps this to a
    /// 200 with `stale: true` in the response extras rather than to a
    /// 5xx so that a single downstream hiccup does not kill a tester
    /// session. Callers that need to distinguish full-strength from
    /// degraded responses look at the extras flag.
    #[error("degraded: {reason}")]
    Degraded {
        /// Short machine-readable label for why the backend had to
        /// degrade. Keep to a few words, e.g. `"sqlite busy"`,
        /// `"cda retry budget exceeded"`, or
        /// `"lock acquisition timeout"`.
        reason: String,
    },

    /// Last-known snapshot served because fresh data was not
    /// available (ADR-0018 rule 4). `age_ms` is how old the cached
    /// data is from the caller's point of view — useful in the wire
    /// response extras so a tester can decide whether to trust it.
    #[error("stale cache: age_ms={age_ms}")]
    StaleCache {
        /// How long ago the cached snapshot was captured, in
        /// milliseconds.
        age_ms: u64,
    },

    /// One federated gateway host is unreachable, but the remaining
    /// hosts may still be able to serve the request. The HTTP layer
    /// and fan-out aggregator treat this as a soft marker instead of
    /// poisoning the whole response (ADR-0018 rule 5).
    #[error("host unreachable: {component_id}")]
    HostUnreachable {
        /// Component id that the unreachable remote host was supposed
        /// to serve.
        component_id: ComponentId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // D1-red: ADR-0018 introduces three new SovdError variants that
    // let backends report soft-failure without poisoning the session.
    // These tests name the variants explicitly so their absence fails
    // the build loudly rather than silently.

    #[test]
    fn degraded_variant_exists_and_displays_reason() {
        let err = SovdError::Degraded {
            reason: "cda retry budget exceeded".into(),
        };
        let rendered = err.to_string();
        assert!(
            rendered.contains("degraded"),
            "Degraded Display missing label: {rendered}"
        );
        assert!(
            rendered.contains("cda retry budget exceeded"),
            "Degraded Display missing reason: {rendered}"
        );
    }

    #[test]
    fn stale_cache_variant_carries_age_ms() {
        let err = SovdError::StaleCache { age_ms: 1_750 };
        let rendered = err.to_string();
        assert!(rendered.contains("stale"), "StaleCache label: {rendered}");
        assert!(rendered.contains("1750"), "StaleCache age_ms: {rendered}");
    }

    #[test]
    fn host_unreachable_variant_carries_component_id() {
        let err = SovdError::HostUnreachable {
            component_id: ComponentId::new("cvc"),
        };
        let rendered = err.to_string();
        assert!(
            rendered.contains("host unreachable"),
            "HostUnreachable label: {rendered}"
        );
        assert!(
            rendered.contains("cvc"),
            "HostUnreachable component: {rendered}"
        );
    }

    #[test]
    fn soft_fail_variants_are_debug_printable() {
        // Regression — if the Debug derive on the enum breaks for
        // struct variants, every tracing::warn! call using `?err`
        // panics at format time.
        let err = SovdError::Degraded {
            reason: "lock timeout".into(),
        };
        let dbg = format!("{err:?}");
        assert!(dbg.contains("Degraded"));
    }
}
