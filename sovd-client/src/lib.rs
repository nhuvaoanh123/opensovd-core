// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
// SPDX-License-Identifier: Apache-2.0

//! Outbound SOVD REST client for the Eclipse `OpenSOVD` core stack.
//!
//! Used by off-board testers, on-board apps, cloud services, and by
//! `sovd-gateway` itself when talking to downstream native-SOVD ECUs.
//! See [`ARCHITECTURE.md`](../../ARCHITECTURE.md) for role boundaries.
//!
//! Phase 0: skeleton only. The [`Client`] unit struct declared here will
//! implement [`sovd_interfaces::traits::SovdClient`] in Phase 3/4.

/// Outbound SOVD REST client instance.
///
/// Will implement [`sovd_interfaces::traits::SovdClient`] in Phase 3/4.
/// Fields (base URL, HTTP client, auth material) are added then.
pub struct Client {
    // fields added in Phase 3/4
}
