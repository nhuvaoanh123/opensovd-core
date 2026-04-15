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

//! Phase 4 Line A — OpenAPI staleness gate (D6).
//!
//! Fails whenever the committed `sovd-server/openapi.yaml` does not
//! match the live output of `utoipa::openapi::OpenApi::to_yaml()`.
//! Regenerate via:
//!
//! ```bash
//! cargo run -p xtask -- openapi-dump
//! ```
//!
//! then re-commit the yaml.

use std::path::{Path, PathBuf};

use utoipa::OpenApi;

fn openapi_yaml_path() -> PathBuf {
    // Workspace root is one level above integration-tests/.
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = here.parent().expect("workspace root");
    workspace_root.join("sovd-server").join("openapi.yaml")
}

#[test]
fn phase4_openapi_yaml_is_in_sync_with_live_api_doc() {
    let live = sovd_server::openapi::ApiDoc::openapi()
        .to_yaml()
        .expect("serialize live ApiDoc to yaml");

    let path = openapi_yaml_path();
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "could not read committed openapi at {} — run `cargo run -p xtask -- openapi-dump`: {e}",
            path.display()
        )
    });

    // Normalise line endings so Windows CRLF does not cause false
    // drift when the yaml was regenerated on Linux.
    let live_normalised = live.replace("\r\n", "\n");
    let committed_normalised = committed.replace("\r\n", "\n");
    assert_eq!(
        live_normalised.trim(),
        committed_normalised.trim(),
        "committed {} is stale; run `cargo run -p xtask -- openapi-dump` and re-commit",
        path.display()
    );
}
