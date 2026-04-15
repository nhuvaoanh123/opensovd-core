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
//! - [`spec`] — wire-format DTOs ported directly from the ASAM SOVD
//!   v1.1.0-rc1 `OpenAPI` template (ISO 17978-3 ed.1). Every type is locked
//!   by a snapshot test under `tests/snapshots/`. See
//!   [`docs/openapi-audit-2026-04-14.md`](../../docs/openapi-audit-2026-04-14.md).
//! - [`extras`] — Taktflow-specific shapes that extend the spec, per
//!   ADR-0006. Currently holds the embedded Fault Library IPC types
//!   ([`extras::fault::FaultRecord`]).
//! - [`types`] — internal Rust-only types ([`types::error::SovdError`],
//!   [`types::component::ComponentId`], session/security wrappers).
//! - [`traits`] — the Server/Gateway/Backend/Client/FaultSink trait
//!   definitions; method signatures use the spec-derived DTOs from [`spec`].
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

pub mod extras;
pub mod spec;
pub mod traits;
pub mod types;

// Flat re-exports for the most frequently used shapes, so downstream crates
// can write `use sovd_interfaces::{SovdError, ComponentId, spec::fault::Fault};`.
pub use traits::{
    backend::{BackendKind, SovdBackend},
    client::SovdClient,
    fault_sink::{FaultRecordRef, FaultSink},
    gateway::SovdGateway,
    operation_cycle::{CurrentCycle, CycleName, OperationCycle, OperationCycleEvent},
    server::SovdServer,
    sovd_db::{OperationCycleId, SovdDb},
};
pub use types::{
    component::ComponentId,
    error::SovdError,
    session::{SecurityLevel, Session, SessionKind},
};
