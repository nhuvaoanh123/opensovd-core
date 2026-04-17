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
    use std::path::PathBuf;

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
        assert_eq!(
            config.local_demo_components,
            vec!["cvc".to_owned(), "sc".to_owned(), "bcm".to_owned()]
        );
        assert_eq!(config.dfm_component_id.as_deref(), Some("dfm"));
        assert!(config.cda_forwards.is_empty());
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

    #[test]
    fn toml_parses_hybrid_phase5_overrides() -> Result<(), Box<dyn std::error::Error>> {
        let config_str = r#"
dfm_component_id = ""
local_demo_components = ["bcm"]

[[cda_forward]]
component_id = "cvc"
remote_component_id = "cvc00000"
base_url = "http://127.0.0.1:20002"
path_prefix = "vehicle/v15"
"#;
        let figment = Figment::from(Serialized::defaults(Configuration::default()))
            .merge(Toml::string(config_str));
        let config: Configuration = figment.extract()?;
        assert_eq!(config.dfm_component_id.as_deref(), Some(""));
        assert_eq!(config.local_demo_components, vec!["bcm".to_owned()]);
        assert_eq!(config.cda_forwards.len(), 1);
        let first = config
            .cda_forwards
            .first()
            .ok_or("missing cda_forward entry")?;
        assert_eq!(first.component_id, "cvc");
        assert_eq!(first.remote_component_id.as_deref(), Some("cvc00000"));
        assert_eq!(first.base_url, "http://127.0.0.1:20002");
        assert_eq!(first.path_prefix, "vehicle/v15");
        Ok(())
    }

    #[test]
    fn checked_in_phase5_hybrid_template_parses() -> Result<(), Box<dyn std::error::Error>> {
        let template = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("sovd-main crate should live under opensovd-core")?
            .join("deploy")
            .join("pi")
            .join("opensovd-pi-phase5-hybrid.toml");
        let figment = Figment::from(Serialized::defaults(Configuration::default()))
            .merge(Toml::file(&template));
        let config: Configuration = figment.extract()?;
        assert_eq!(config.dfm_component_id.as_deref(), Some(""));
        // 3-ECU bench per ADR-0023: BCM local, CVC+SC forwarded to CDA.
        assert_eq!(config.local_demo_components, vec!["bcm".to_owned()]);
        assert_eq!(config.cda_forwards.len(), 2);
        assert_eq!(
            config
                .cda_forwards
                .iter()
                .map(|forward| forward.component_id.as_str())
                .collect::<Vec<_>>(),
            vec!["cvc", "sc"]
        );
        assert_eq!(
            config
                .cda_forwards
                .iter()
                .map(|forward| forward.remote_component_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("cvc00000"), Some("sc00000")]
        );
        assert!(
            config
                .cda_forwards
                .iter()
                .all(|forward| forward.path_prefix == "vehicle/v15")
        );
        Ok(())
    }
}
