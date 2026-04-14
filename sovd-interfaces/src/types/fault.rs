// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
// SPDX-License-Identifier: Apache-2.0

//! Fault records ingested by the Diagnostic Fault Manager.
//!
//! These shapes mirror the upstream `fault-lib` crate (see
//! [`opensovd/docs/design/design.md`](../../../../opensovd/docs/design/design.md)
//! §"Fault Library"). The Fault Library on each ECU pushes faults to the
//! central DFM via IPC; on the DFM side the IPC frames are decoded into
//! [`FaultRecord`] and handed to a
//! [`FaultSink`](crate::traits::fault_sink::FaultSink) implementation.
//!
//! Per ADR-001 (Fault Lib is the S-CORE interface), this file is the Rust
//! mirror of the C shim shape — keep it in lock-step with the C struct.

use serde::{Deserialize, Serialize};

use crate::types::component::ComponentId;

/// ECU-specific Fault Identifier (FID).
///
/// Unique within one ECU. The DFM maps (`ComponentId`, `FaultId`) pairs to
/// OEM-visible DTCs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FaultId(pub u32);

/// Fault severity reported by the Fault Library.
///
/// Mirrors the C `enum FaultSeverity` in the embedded shim (DLT-style).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaultSeverity {
    /// Informational, not a fault per se; never escalates to a DTC.
    Info,
    /// Warning; may escalate to a pending DTC after debouncing.
    Warning,
    /// Error; escalates to a confirmed DTC after debouncing.
    Error,
    /// Fatal; immediate DTC, may trigger Health & Lifecycle reactions.
    Fatal,
}

/// A single fault event as reported by the Fault Library.
///
/// Fields are deliberately minimal — the DFM owns aggregation, counting,
/// operation-cycle gating, and persistence. All the Fault Library does is
/// announce that an event occurred.
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
    /// stores this as-is and surfaces it via SOVD `faults/{id}/data`.
    pub meta: Option<serde_json::Value>,
}
