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

//! Forwarding SOVD backends for the hybrid [`InMemoryServer`] + CDA dispatcher.
//!
//! [`InMemoryServer`]: crate::in_memory::InMemoryServer
//!
//! These backends implement
//! [`sovd_interfaces::traits::backend::SovdBackend`] and are stored behind
//! `Box<dyn SovdBackend>` in the dispatcher registry (see
//! [`crate::in_memory::InMemoryServer`]). That is the trait-object-safe
//! contract — it uses `#[async_trait]` per ADR-0015, in contrast to the
//! per-component [`sovd_interfaces::traits::server::SovdServer`] which is
//! native async but not dyn-safe.
//!
//! See [`ARCHITECTURE.md`](../../../ARCHITECTURE.md) §"`SovdBackend`" for
//! where this fits in the gateway pattern, and `MASTER-PLAN.md` §2.1 for
//! the hybrid-dispatcher Phase 2 Line A slice that uses it.

pub mod cda;

pub use cda::CdaBackend;
