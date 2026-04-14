// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
// SPDX-License-Identifier: Apache-2.0

//! Trait contracts for every role in the `opensovd-core` workspace.
//!
//! Each submodule defines exactly one role. Implementers live in the
//! matching `sovd-*` crate and are listed in
//! [`ARCHITECTURE.md`](../../../ARCHITECTURE.md).

pub mod backend;
pub mod client;
pub mod fault_sink;
pub mod gateway;
pub mod server;
