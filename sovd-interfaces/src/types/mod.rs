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

//! Internal Rust-only types used by every `opensovd-core` crate.
//!
//! After the spec port (Deliverable 2/3, 2026-04-14) this module holds
//! **only** types that are not in the ASAM SOVD `OpenAPI` surface:
//!
//! - [`component::ComponentId`] — typed wrapper for routing (the spec uses
//!   bare strings, but internally we want a distinct type).
//! - [`error::SovdError`] — the Rust error enum returned from every trait
//!   method. Mapped to [`crate::spec::error::GenericError`] at the HTTP
//!   layer.
//! - [`session::SessionKind`], [`session::Session`],
//!   [`session::SecurityLevel`] — UDS session / security-access shapes
//!   reused by both native servers and CDA. (The spec exposes these via
//!   `modes/`, but the Phase 0 trait surface still uses the internal Rust
//!   representation; a future spec port will move them into
//!   [`crate::spec::mode`] when the modes resource is on the MVP path.)
//!
//! Wire-format DTOs (`Fault`, `Operation*`, `EntityCapabilities`, …) live
//! in [`crate::spec`] and are derived from the ISO 17978-3 `OpenAPI` YAML.

pub mod component;
pub mod error;
pub mod session;
