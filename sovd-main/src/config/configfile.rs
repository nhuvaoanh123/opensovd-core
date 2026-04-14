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

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Configuration {
    pub server: ServerConfig,
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
        }
    }
}
