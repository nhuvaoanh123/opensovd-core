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

//! Diagnostic Trouble Code (DTC) shapes.
//!
//! `Dtc` is the OEM-visible fault code surfaced through the SOVD
//! `components/{ecu}/faults` entity. It is backed either by `sovd-dfm`
//! (native faults aggregated via the Fault Library) or by the Classic
//! Diagnostic Adapter (faults read from legacy UDS ECUs via service `0x19`).
//!
//! See upstream design.md §"Diagnostic DB" for the data shape requirements
//! (OEM-specific code, FID, count, meta).

use serde::{Deserialize, Serialize};

/// OEM-specific DTC identifier (e.g. 24-bit encoded as `u32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DtcId(pub u32);

impl std::fmt::Display for DtcId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:06X}", self.0)
    }
}

/// DTC severity as defined by ISO 14229-1 / ASAM SOVD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DtcSeverity {
    /// No maintenance needed.
    NoSeverity,
    /// Maintenance required at next regular service.
    MaintenanceOnly,
    /// Check immediately at next halt.
    CheckAtNextHalt,
    /// Check immediately.
    CheckImmediately,
}

/// ISO 14229-1 DTC status byte (§D.1).
///
/// Carries the eight status bits reported by the ECU per DTC:
/// `testFailed`, `testFailedThisOperationCycle`, `pendingDTC`,
/// `confirmedDTC`, `testNotCompletedSinceLastClear`,
/// `testFailedSinceLastClear`, `testNotCompletedThisOperationCycle`,
/// `warningIndicatorRequested`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DtcStatus(pub u8);

/// Bit mask used to filter DTCs when querying `list_dtcs`.
///
/// A bit set to `1` means "only include DTCs that have this status bit set".
/// A mask of `0x00` disables filtering (return all DTCs).
///
/// This mirrors the UDS `DTCStatusMask` argument of service `0x19 02`
/// (`reportDTCByStatusMask`), which the Classic Diagnostic Adapter uses
/// when bridging to legacy ECUs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DtcStatusMask(pub u8);

impl DtcStatusMask {
    /// Mask that matches every DTC (no filtering).
    #[must_use]
    pub const fn all() -> Self {
        Self(0x00)
    }
}

/// DTC grouping used by `clear_dtcs`.
///
/// SOVD `POST .../faults/clear` and UDS `0x14 ClearDiagnosticInformation`
/// both take a "group of DTC" argument. `All` maps to `0xFFFFFF` (clear
/// everything), `Group(u32)` maps to a specific 24-bit group code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DtcGroup {
    /// Clear every DTC on the target component.
    All,
    /// Clear only DTCs belonging to the given group code.
    Group(u32),
}

/// A fully resolved Diagnostic Trouble Code record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dtc {
    /// OEM-specific DTC identifier.
    pub id: DtcId,
    /// ISO 14229 status byte.
    pub status: DtcStatus,
    /// Severity classification.
    pub severity: DtcSeverity,
    /// Human-readable short name (e.g. "`P0420`", "`MotorOverCurrent`").
    pub name: Option<String>,
    /// Occurrence count since last clear.
    pub occurrence_count: u32,
}
