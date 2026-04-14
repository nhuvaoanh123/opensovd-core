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

//! Figment-based configuration loader for `sovd-main`.
//!
//! The merge order mirrors upstream classic-diagnostic-adapter:
//! serialized defaults first, then an optional TOML file, then environment
//! variables prefixed with `OPENSOVD`. CLI flags are applied on top of the
//! loaded configuration by the binary entry point.

use std::path::Path;

use figment::{
    Figment,
    providers::{Env, Format as _, Serialized, Toml},
};

pub mod configfile;

/// Loads the configuration from an optional TOML file.
///
/// If `config_file` is `Some`, the referenced file is merged on top of the
/// built-in defaults. Environment variables prefixed with `OPENSOVD` are then
/// merged on top of that.
///
/// # Errors
/// Returns an error message if the configuration cannot be extracted.
pub fn load_config(config_file: Option<&Path>) -> Result<configfile::Configuration, String> {
    let mut figment = Figment::from(Serialized::defaults(default_config()));

    if let Some(path) = config_file {
        println!("Loading configuration from {}", path.display());
        figment = figment.merge(Toml::file(path));
    }

    figment
        .merge(Env::prefixed("OPENSOVD"))
        .extract()
        .map_err(|e| format!("Failed to build configuration: {e}"))
}

#[must_use]
pub fn default_config() -> configfile::Configuration {
    configfile::Configuration::default()
}

#[cfg(test)]
mod tests {
    use figment::{
        Figment,
        providers::{Format, Serialized, Toml},
    };

    use super::configfile::Configuration;

    #[test]
    fn defaults_use_port_20002() {
        let config = Configuration::default();
        assert_eq!(config.server.address, "0.0.0.0");
        assert_eq!(config.server.port, 20002);
    }

    #[test]
    fn toml_overrides_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let config_str = r#"
[server]
address = "127.0.0.1"
port = 20004
"#;
        let figment = Figment::from(Serialized::defaults(Configuration::default()))
            .merge(Toml::string(config_str));
        let config: Configuration = figment.extract()?;
        assert_eq!(config.server.address, "127.0.0.1");
        assert_eq!(config.server.port, 20004);
        Ok(())
    }
}
