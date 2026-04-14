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

//! Shared types, traits, and interfaces for the Eclipse `OpenSOVD` core stack.
//!
//! This crate is the contract boundary between every other crate in
//! `opensovd-core`. It contains only type shapes and trait signatures — no
//! runtime code, no I/O, no async executors. See
//! [`ARCHITECTURE.md`](../../ARCHITECTURE.md) for how the traits fit together
//! and which crate implements what.
//!
//! # Module map
//!
//! - [`types`] — request/response DTOs, DTC/component/routine/session types
//!   and the [`types::error::SovdError`] error enum.
//! - [`traits`] — the Server/Gateway/Backend/Client/FaultSink trait
//!   definitions.
//!
//! # Conventions
//!
//! - All fallible operations return
//!   [`Result<T, SovdError>`](types::error::SovdError).
//! - Async traits use `async-trait` where trait objects are needed
//!   (`Box<dyn SovdBackend + Send + Sync>`), and stable `async fn in trait`
//!   elsewhere (Rust 1.75+, workspace pins to 1.88.0).
//! - Semantics references to upstream
//!   [`opensovd/docs/design/design.md`](../../../opensovd/docs/design/design.md)
//!   are called out inline.

pub mod spec;
pub mod traits;
pub mod types;

// Flat re-exports for the most frequently used shapes, so downstream crates
// can write `use sovd_interfaces::{SovdError, Dtc, ComponentId};`.
pub use traits::{
    backend::{BackendKind, SovdBackend},
    client::SovdClient,
    fault_sink::FaultSink,
    gateway::SovdGateway,
    server::SovdServer,
};
pub use types::{
    component::{ComponentId, ComponentInfo, HwRevision, SwVersion},
    data::{DataIdentifier, DataValue},
    dtc::{Dtc, DtcGroup, DtcId, DtcSeverity, DtcStatus, DtcStatusMask},
    error::SovdError,
    fault::{FaultId, FaultRecord, FaultSeverity},
    routine::{RoutineId, RoutineResult, RoutineState},
    session::{SecurityLevel, Session, SessionKind},
};
