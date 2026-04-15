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

//! Taktflow-specific shapes that extend the SOVD spec.
//!
//! Per **ADR-0006** (`extra:` convention), anything in this module is a
//! deliberate extension to the upstream ASAM SOVD v1.1.0-rc1 surface. Each
//! type carries an `// Extra (per ADR-0006): ...` comment explaining why
//! it exists and what gap it fills.
//!
//! The two main reasons a type lives here instead of in
//! [`crate::spec`]:
//!
//! 1. **Internal IPC** — shapes that travel inside the SOVD stack but are
//!    never serialised to the SOVD REST wire (e.g. [`fault::FaultRecord`],
//!    which the embedded Fault Library shim pushes into the DFM over IPC).
//!
//! 2. **Convenience helpers** — Rust-friendly wrappers around spec values
//!    that the spec leaves open or under-specified (e.g. enum mirrors of
//!    integer severity conventions).
//!
//! Anything added here that later turns out to belong upstream should be
//! moved into [`crate::spec`] as part of the next spec sync.

pub mod fault;
pub mod health;
pub mod response;
