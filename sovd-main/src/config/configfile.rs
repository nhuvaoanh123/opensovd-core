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

//! Configuration data types for `sovd-main`.
//!
//! The shape mirrors the upstream classic-diagnostic-adapter configuration
//! so TOML files and environment variables can be authored with the same
//! conventions in both projects.

use serde::{Deserialize, Serialize};
use sovd_dfm::DfmBackendConfig;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Configuration {
    pub server: ServerConfig,
    /// Per ADR-0016 `[backend]` section. Runtime-dispatches SovdDb /
    /// FaultSink / OperationCycle picks. Compile-time `score` feature
    /// gates whether the S-CORE crates are linked in at all.
    #[serde(default)]
    pub backend: DfmBackendConfig,
    /// DFM-served component id. Requests to this component on
    /// /sovd/v1/components/{id}/faults go through the DFM's SovdDb.
    /// Anything not matching still falls through to the InMemoryServer
    /// demo data for route-compatibility with Phase 1/2 tests.
    #[serde(default = "default_dfm_component_id")]
    pub dfm_component_id: String,
}

fn default_dfm_component_id() -> String {
    "dfm".to_owned()
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ServerConfig {
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub mode: ServerMode,
}

/// Which axum `Router` [`sovd-main`](crate) mounts at startup.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServerMode {
    /// Full in-memory MVP server exposing every Phase-3/4 endpoint
    /// against canned demo data. This is the default.
    #[default]
    InMemory,
    /// Bare `/sovd/v1/health` endpoint only. Kept for smoke tests that
    /// do not need the full route surface.
    HelloWorld,
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration {
            server: ServerConfig {
                address: "0.0.0.0".to_owned(),
                port: 20002,
                mode: ServerMode::default(),
            },
            backend: DfmBackendConfig::default(),
            dfm_component_id: default_dfm_component_id(),
        }
    }
}
