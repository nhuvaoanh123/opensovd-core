// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
// SPDX-License-Identifier: Apache-2.0

//! SOVD gateway — system-wide SOVD multiplexer.
//!
//! Accepts SOVD requests, resolves `ComponentId` to a registered
//! [`sovd_interfaces::traits::backend::SovdBackend`], and forwards the
//! call. See [`ARCHITECTURE.md`](../../ARCHITECTURE.md) for role boundaries.
//!
//! Phase 0 stub. Routing table, concurrent fan-out, and the CDA adapter
//! bridge land in Phase 4.

/// System-wide SOVD gateway instance.
///
/// Will implement [`sovd_interfaces::traits::gateway::SovdGateway`] in
/// Phase 4. Fields (backend registry, routing cache, client pool for
/// federated hops) are added then.
pub struct Gateway {
    // fields added in Phase 4
}
