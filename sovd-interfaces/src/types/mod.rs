// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
// SPDX-License-Identifier: Apache-2.0

//! Request/response DTOs and domain types used by every `opensovd-core` crate.
//!
//! Each submodule owns one slice of the SOVD domain. Keep shapes minimal in
//! Phase 0 — new fields are added in Phase 3/4 once real backends land.

pub mod component;
pub mod data;
pub mod dtc;
pub mod error;
pub mod fault;
pub mod routine;
pub mod session;
