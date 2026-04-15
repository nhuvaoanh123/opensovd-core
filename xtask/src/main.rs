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

//! `cargo xtask` — workspace developer task runner.
//!
//! Currently only implements `openapi-dump`, which regenerates
//! `sovd-server/openapi.yaml` from the live `utoipa::openapi::OpenApi`
//! document in `sovd-server::openapi::ApiDoc`. The staleness gate in
//! `integration-tests/tests/phase4_openapi_staleness.rs` fails the
//! test suite whenever the committed yaml drifts from the live output,
//! so this command is the only blessed way to refresh it.

use std::{fs, path::PathBuf};

use clap::{Parser, Subcommand};
use utoipa::OpenApi;

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "OpenSOVD workspace task runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Regenerate `sovd-server/openapi.yaml` from the live ApiDoc.
    OpenapiDump {
        /// Print a diff to stderr and exit non-zero instead of
        /// overwriting the file. Used in CI staleness gates.
        #[arg(long)]
        check: bool,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::OpenapiDump { check } => match openapi_dump(check) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask openapi-dump failed: {e}");
                std::process::ExitCode::FAILURE
            }
        },
    }
}

fn openapi_dump(check: bool) -> Result<(), Box<dyn std::error::Error>> {
    let live = sovd_server::openapi::ApiDoc::openapi().to_yaml()?;

    let path = openapi_yaml_path();
    if check {
        let committed = fs::read_to_string(&path)?;
        let live_normalised = live.replace("\r\n", "\n");
        let committed_normalised = committed.replace("\r\n", "\n");
        if live_normalised.trim() != committed_normalised.trim() {
            eprintln!("openapi.yaml is stale at {}", path.display());
            return Err("openapi.yaml staleness gate failed".into());
        }
        eprintln!("openapi.yaml is in sync");
        return Ok(());
    }

    // Ensure a trailing newline on write so git's diff view looks
    // tidy even on editors that do not auto-append one.
    let mut buf = live;
    if !buf.ends_with('\n') {
        buf.push('\n');
    }
    fs::write(&path, buf)?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

fn openapi_yaml_path() -> PathBuf {
    // CARGO_MANIFEST_DIR points at opensovd-core/xtask/; go up one.
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = here.parent().expect("workspace root").to_path_buf();
    root.join("sovd-server").join("openapi.yaml")
}
