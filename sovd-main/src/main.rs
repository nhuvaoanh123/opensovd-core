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

//! Eclipse `OpenSOVD` core - main binary entry point.
//!
//! Phase 0 boots the `sovd-server` axum router and serves the single
//! `GET /sovd/v1/health` endpoint. The listen address is resolved from
//! figment-based configuration (defaults -> optional TOML file -> `OPENSOVD`
//! env vars -> CLI overrides), matching the upstream classic-diagnostic-adapter
//! conventions. Real gateway wiring lands in later phases.

use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;

use crate::config::configfile::Configuration;

mod config;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct AppArgs {
    /// Path to a TOML configuration file.
    #[arg(short = 'c', long)]
    config_file: Option<PathBuf>,

    /// Override the listen address from configuration.
    #[arg(long)]
    listen_address: Option<String>,

    /// Override the listen port from configuration.
    #[arg(long)]
    listen_port: Option<u16>,
}

impl AppArgs {
    fn update_config(self, config: &mut Configuration) {
        if let Some(listen_address) = self.listen_address {
            config.server.address = listen_address;
        }
        if let Some(listen_port) = self.listen_port {
            config.server.port = listen_port;
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = AppArgs::parse();
    let config_file = args.config_file.clone();
    let mut config = config::load_config(config_file.as_deref()).unwrap_or_else(|e| {
        println!("Failed to load configuration: {e}");
        println!("Using default values");
        config::default_config()
    });

    args.update_config(&mut config);

    let app = sovd_server::app();
    let addr: SocketAddr = format!("{}:{}", config.server.address, config.server.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!(
        "OpenSOVD core listening on {}:{}",
        config.server.address,
        config.server.port
    );
    axum::serve(listener, app).await?;
    Ok(())
}
