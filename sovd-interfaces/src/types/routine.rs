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

//! Routine (SOVD `operations`) types.
//!
//! A routine maps to UDS service `0x31 RoutineControl` in CDA-backed ECUs
//! and to registered handlers in native `sovd-server` backends.
//!
//! Lifecycle (mirrors ISO 14229-1 §10.4):
//!
//! 1. Caller invokes
//!    [`SovdServer::start_routine`](crate::traits::server::SovdServer::start_routine)
//!    with a [`RoutineId`] and raw argument bytes.
//! 2. The backend validates pre-conditions (session, security, input
//!    length). On failure it returns
//!    [`SovdError::RoutineFailed`](crate::types::error::SovdError::RoutineFailed).
//! 3. The caller polls
//!    [`SovdServer::routine_status`](crate::traits::server::SovdServer::routine_status)
//!    which returns a [`RoutineState`] describing progress.
//! 4. When the routine reaches [`RoutineState::Finished`] the caller reads
//!    the [`RoutineResult`] carried inside it.

use serde::{Deserialize, Serialize};

/// Routine identifier, typically a 16-bit UDS routine id widened to `u32`
/// to allow for vendor extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoutineId(pub u32);

impl std::fmt::Display for RoutineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:04X}", self.0)
    }
}

/// Raw routine result payload returned by the ECU on `requestRoutineResults`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineResult {
    /// Opaque vendor-specific result bytes.
    pub data: Vec<u8>,
}

/// Current lifecycle state of a routine invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutineState {
    /// Routine has not been started on this backend yet.
    Idle,
    /// Routine was accepted by the ECU and is still running.
    Running,
    /// Routine reached a terminal state with a result payload.
    Finished(RoutineResult),
    /// Routine terminated in error. The attached string is vendor-supplied.
    Failed(String),
}
