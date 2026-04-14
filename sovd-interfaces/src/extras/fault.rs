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

//! Fault Library IPC types — internal SOVD-stack shapes, **not** in the
//! ASAM SOVD `OpenAPI` spec.
//!
//! Extra (per ADR-0006): the SOVD spec only models the **outbound** fault
//! API (`GET .../faults`). The **inbound** path — how an embedded Fault
//! Library on each ECU pushes events into the central DFM — is internal
//! IPC and lives outside ISO 17978-3. These shapes are the Rust mirror of
//! the C `fault-lib` shim defined in upstream `opensovd/docs/design/design.md`
//! §"Fault Library".
//!
//! Per ADR-001 (S-CORE Interface), this is the one-and-only API surface
//! through which faults enter the SOVD stack from platform/application
//! code.

use serde::{Deserialize, Serialize};

use crate::types::component::ComponentId;

/// ECU-specific Fault Identifier (FID).
///
/// Extra (per ADR-0006): unique within one ECU. The DFM maps
/// (`ComponentId`, `FaultId`) pairs to spec-visible
/// [`crate::spec::fault::Fault`] entries when answering `GET .../faults`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FaultId(pub u32);

/// Severity reported by the embedded Fault Library shim.
///
/// Extra (per ADR-0006): mirrors the C `enum FaultSeverity` in the
/// embedded shim (DLT-style). The spec uses an integer with the
/// convention `1=FATAL, 2=ERROR, 3=WARN, 4=INFO`; this enum is the
/// type-safe Rust mirror of that convention and converts to
/// [`crate::spec::fault::Fault`]`.severity` in the spec port via
/// [`Self::as_i32`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaultSeverity {
    /// Informational — never escalates to a SOVD fault entry.
    Info,
    /// Warning — may escalate to a pending fault after debouncing.
    Warning,
    /// Error — escalates to a confirmed fault after debouncing.
    Error,
    /// Fatal — immediate fault, may trigger Health & Lifecycle reactions.
    Fatal,
}

impl FaultSeverity {
    /// Encode this severity as the integer the SOVD spec uses on the wire.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::Fatal => 1,
            Self::Error => 2,
            Self::Warning => 3,
            Self::Info => 4,
        }
    }
}

/// A single fault event reported by an embedded Fault Library shim.
///
/// Extra (per ADR-0006): the embedded fault path is internal IPC, not
/// REST. Fields are deliberately minimal — the DFM owns aggregation,
/// counting, operation-cycle gating, and persistence. All the Fault
/// Library does is announce that an event occurred.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaultRecord {
    /// Which component reported the fault.
    pub component: ComponentId,

    /// Fault identifier.
    pub id: FaultId,

    /// Severity at the moment of reporting.
    pub severity: FaultSeverity,

    /// Monotonic timestamp (milliseconds since boot) when the fault was
    /// observed on the ECU.
    pub timestamp_ms: u64,

    /// Optional opaque meta-data (snapshot data, freeze frames). The DFM
    /// stores this as-is and surfaces it via SOVD `faults/{code}/data`.
    pub meta: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_as_i32_matches_spec_convention() {
        assert_eq!(FaultSeverity::Fatal.as_i32(), 1);
        assert_eq!(FaultSeverity::Error.as_i32(), 2);
        assert_eq!(FaultSeverity::Warning.as_i32(), 3);
        assert_eq!(FaultSeverity::Info.as_i32(), 4);
    }

    #[test]
    fn fault_record_round_trip() {
        let r = FaultRecord {
            component: ComponentId::new("bcm"),
            id: FaultId(0x0012_00E3),
            severity: FaultSeverity::Error,
            timestamp_ms: 1_234_567,
            meta: Some(serde_json::json!({"battery_voltage": 12.8})),
        };
        let json = serde_json::to_string(&r).expect("serialize");
        let back: FaultRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, back);
    }
}
