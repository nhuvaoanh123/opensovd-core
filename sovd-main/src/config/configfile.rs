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
use sovd_server::backends::cda::DEFAULT_CDA_PATH_PREFIX;

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
    ///
    /// Empty string disables the DFM forward without needing TOML
    /// `null` syntax in deployment configs.
    #[serde(default = "default_dfm_component_id")]
    pub dfm_component_id: Option<String>,
    /// Which demo components should stay local to the in-process
    /// `InMemoryServer`. Defaults to the legacy demo trio so existing
    /// D1 deployments stay stable until they opt into a narrower surface.
    #[serde(default = "default_local_demo_components")]
    pub local_demo_components: Vec<String>,
    /// Optional CDA-backed forwards registered at startup.
    #[serde(default, rename = "cda_forward")]
    pub cda_forwards: Vec<CdaForwardConfig>,
}

#[allow(clippy::unnecessary_wraps)]
fn default_dfm_component_id() -> Option<String> {
    Some("dfm".to_owned())
}

fn default_local_demo_components() -> Vec<String> {
    // 3-ECU bench per ADR-0023: CVC (physical STM32, central),
    // SC (physical TMS570, safety), BCM (POSIX virtual, body control).
    vec!["cvc".to_owned(), "sc".to_owned(), "bcm".to_owned()]
}

fn default_cda_path_prefix() -> String {
    DEFAULT_CDA_PATH_PREFIX.to_owned()
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CdaForwardConfig {
    pub component_id: String,
    #[serde(default)]
    pub remote_component_id: Option<String>,
    pub base_url: String,
    #[serde(default = "default_cda_path_prefix")]
    pub path_prefix: String,
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
            local_demo_components: default_local_demo_components(),
            cda_forwards: Vec::new(),
        }
    }
}
