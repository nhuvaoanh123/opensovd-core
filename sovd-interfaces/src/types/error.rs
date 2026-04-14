// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
// SPDX-License-Identifier: Apache-2.0

//! Unified error type returned by every trait in `sovd-interfaces`.
//!
//! Downstream crates map this enum onto their own wire formats:
//!
//! - `sovd-server` maps it to SOVD HTTP status codes and error bodies.
//! - `sovd-gateway` forwards it upstream unchanged (possibly wrapped in
//!   `BackendUnavailable` if the routed-to backend is down).
//! - `sovd-client` deserializes SOVD HTTP error bodies into this enum.

use thiserror::Error;

use crate::types::{component::ComponentId, routine::RoutineId};

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

    /// A routine was started but reported a failure.
    #[error("routine {id} failed: {reason}")]
    RoutineFailed {
        /// Which routine failed.
        id: RoutineId,
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
}
