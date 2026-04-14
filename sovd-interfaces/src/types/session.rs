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

//! Session and security level types.
//!
//! SOVD exposes the UDS session concept as a `modes/session` entity and the
//! security-access concept as `modes/security`. These shapes are reused
//! both by native `sovd-server` and by CDA when bridging to UDS.

use serde::{Deserialize, Serialize};

/// UDS diagnostic session kind (ISO 14229-1 §9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionKind {
    /// `0x01` — default session.
    Default,
    /// `0x02` — programming session.
    Programming,
    /// `0x03` — extended diagnostic session.
    Extended,
    /// `0x04` — safety system diagnostic session.
    SafetySystem,
    /// Vendor-specific session (`0x40`..`0x5F`).
    Vendor(u8),
}

/// Active diagnostic session on one component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Which kind of session is active.
    pub kind: SessionKind,
    /// Timestamp at which the session will time out if no tester-present
    /// is received. `None` means no timeout is tracked.
    pub expires_at_ms: Option<u64>,
}

/// UDS security access level (ISO 14229-1 §9.4).
///
/// Level `0` means "locked" / no security granted. Odd values are
/// `requestSeed` sub-functions, even values are `sendKey` sub-functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityLevel(pub u8);
