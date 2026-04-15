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

#![allow(clippy::doc_markdown)]

//! Eclipse `OpenSOVD` core - main binary entry point.
//!
//! Phase 3 boots the in-memory MVP server and, when configured,
//! registers a real DFM backed by a [`SqliteSovdDb`] +
//! [`TaktflowOperationCycle`] as a forward for the configured
//! component id. Everything else still resolves against the in-memory
//! demo data, so the Phase 1/2 route tests keep working.

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use clap::Parser;
use opcycle_taktflow::TaktflowOperationCycle;
use sovd_db_sqlite::SqliteSovdDb;
use sovd_dfm::{Dfm, FaultSinkBackend, OperationCycleBackend, PersistenceBackend};
use sovd_interfaces::{
    ComponentId,
    traits::{operation_cycle::OperationCycle, sovd_db::SovdDb},
};
use sovd_server::InMemoryServer;

use crate::config::configfile::{Configuration, ServerMode};

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

    /// Override the persistence backend.
    #[arg(long, value_enum)]
    backend: Option<BackendChoice>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum BackendChoice {
    /// Default standalone backend: SQLite + unix-sock + in-process cycles.
    Sqlite,
    /// S-CORE backend (requires the `score` Cargo feature at compile time).
    Score,
}

impl AppArgs {
    fn update_config(self, config: &mut Configuration) {
        if let Some(listen_address) = self.listen_address {
            config.server.address = listen_address;
        }
        if let Some(listen_port) = self.listen_port {
            config.server.port = listen_port;
        }
        if let Some(backend) = self.backend {
            match backend {
                BackendChoice::Sqlite => {
                    config.backend.persistence = PersistenceBackend::Sqlite;
                    config.backend.fault_sink = FaultSinkBackend::Unix;
                    config.backend.operation_cycle = OperationCycleBackend::Taktflow;
                }
                BackendChoice::Score => {
                    config.backend.persistence = PersistenceBackend::Score;
                    config.backend.fault_sink = FaultSinkBackend::Lola;
                    config.backend.operation_cycle = OperationCycleBackend::ScoreLifecycle;
                }
            }
        }
    }
}

async fn build_dfm(config: &Configuration) -> Result<Dfm, Box<dyn std::error::Error>> {
    let db: Arc<dyn SovdDb> = match config.backend.persistence {
        PersistenceBackend::Sqlite => {
            tracing::info!(
                path = %config.backend.sqlite_path,
                "Opening SQLite DFM store"
            );
            Arc::new(
                SqliteSovdDb::connect(std::path::Path::new(&config.backend.sqlite_path)).await?,
            )
        }
        #[cfg(feature = "score")]
        PersistenceBackend::Score => {
            tracing::warn!("Using sovd-db-score stub backend (Phase 4 will wire real crate)");
            Arc::new(sovd_db_score::ScoreSovdDb::new())
        }
        #[cfg(not(feature = "score"))]
        PersistenceBackend::Score => {
            return Err(
                "backend.persistence = \"score\" requires the `score` Cargo feature".into(),
            );
        }
    };

    let cycles: Arc<dyn OperationCycle> = match config.backend.operation_cycle {
        OperationCycleBackend::Taktflow => Arc::new(TaktflowOperationCycle::new()),
        #[cfg(feature = "score")]
        OperationCycleBackend::ScoreLifecycle => {
            tracing::warn!("Using opcycle-score-lifecycle stub (Phase 4 will wire real crate)");
            Arc::new(opcycle_score_lifecycle::ScoreOperationCycle::new())
        }
        #[cfg(not(feature = "score"))]
        OperationCycleBackend::ScoreLifecycle => {
            return Err(
                "backend.operation_cycle = \"score-lifecycle\" requires the `score` Cargo feature"
                    .into(),
            );
        }
    };

    // fault_sink is configured but the actual IPC reader task is
    // started by the integration test harness (and, in Phase 4, the
    // production runtime). For the sovd-main process today we only
    // need the DFM itself — the fault_sink config is carried in the
    // TOML so a future IPC wiring pass can honor it without config
    // changes.

    Ok(Dfm::builder(ComponentId::new(&config.dfm_component_id))
        .with_db(db)
        .with_cycles(cycles)
        .build()?)
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

    let app = match config.server.mode {
        ServerMode::InMemory => {
            tracing::info!("Booting InMemoryServer with demo data (cvc, fzc, rzc)");
            let server = Arc::new(InMemoryServer::new_with_demo_data());
            // Build and register the DFM as a forward on the configured
            // dfm_component_id. Requests for that component go through
            // the real DFM; everything else still resolves locally.
            let dfm = build_dfm(&config).await?;
            tracing::info!(
                component = %config.dfm_component_id,
                "Registering DFM as forward backend"
            );
            server.register_forward(Arc::new(dfm)).await?;
            sovd_server::routes::app_with_server(server)
        }
        ServerMode::HelloWorld => {
            tracing::info!("Booting hello-world router (health endpoint only)");
            sovd_server::app()
        }
    };

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
