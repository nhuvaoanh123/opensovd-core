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

//! Health-report extras for `GET /sovd/v1/health`.
//!
//! Extra (per ADR-0006): ISO 17978-3 does not standardise a health
//! endpoint. We add this under `extras::health` rather than `spec` so
//! the wire boundary remains explicit about which fields are
//! spec-derived and which are Taktflow extensions.
//!
//! See ADR-0015 (`sovd-interfaces` layering) for why health lives here
//! instead of in `spec`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::traits::backend::BackendHealth;

/// Per-backend probe result returned under each field in
/// [`HealthStatus`]. Re-export of
/// [`crate::traits::backend::BackendHealth`] so callers can `use
/// extras::health::BackendProbe` without reaching into `traits`.
pub type BackendProbe = BackendHealth;

/// Envelope returned by `GET /sovd/v1/health`.
///
/// Extra (per ADR-0006): fields beyond `status` / `version` are
/// Taktflow extensions. The shape is stable across the Phase 4 Line A
/// surface and is documented in the generated `OpenAPI` yaml.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct HealthStatus {
    /// Top-level liveness status. Always `"ok"` when the server is
    /// answering HTTP; backend-specific failures are reported in the
    /// `sovd_db` / `fault_sink` / `operation_cycle` probes below.
    pub status: String,

    /// Crate version reported from `env!("CARGO_PKG_VERSION")`.
    pub version: String,

    /// Result of probing the persistence backend.
    pub sovd_db: BackendProbe,

    /// Result of probing the ingestion-side fault sink.
    pub fault_sink: BackendProbe,

    /// Name of the currently active operation cycle, if any.
    /// `None` means the cycle driver is in [`Idle`](crate::traits::operation_cycle::OperationCycleEvent::Idle).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_cycle: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_status_round_trip_ok() {
        let h = HealthStatus {
            status: "ok".into(),
            version: "0.1.0".into(),
            sovd_db: BackendProbe::Ok,
            fault_sink: BackendProbe::Ok,
            operation_cycle: Some("tester.phase4".into()),
        };
        let json = serde_json::to_string(&h).expect("serialize");
        let back: HealthStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(h, back);
    }

    #[test]
    fn health_status_round_trip_degraded() {
        let h = HealthStatus {
            status: "ok".into(),
            version: "0.1.0".into(),
            sovd_db: BackendProbe::Degraded {
                reason: "slow responses".into(),
            },
            fault_sink: BackendProbe::Unavailable {
                reason: "socket not bound".into(),
            },
            operation_cycle: None,
        };
        let json = serde_json::to_string(&h).expect("serialize");
        let back: HealthStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(h, back);
    }
}
