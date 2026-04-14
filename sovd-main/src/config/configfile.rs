// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
// SPDX-License-Identifier: Apache-2.0

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
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration {
            server: ServerConfig {
                address: "0.0.0.0".to_owned(),
                port: 20002,
            },
        }
    }
}
