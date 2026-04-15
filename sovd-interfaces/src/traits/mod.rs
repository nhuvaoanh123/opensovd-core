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

//! Trait contracts for every role in the `opensovd-core` workspace.
//!
//! Each submodule defines exactly one role. Implementers live in the
//! matching `sovd-*` crate and are listed in
//! [`ARCHITECTURE.md`](../../../ARCHITECTURE.md).

pub mod backend;
pub mod client;
pub mod fault_sink;
pub mod gateway;
pub mod operation_cycle;
pub mod server;
pub mod sovd_db;
