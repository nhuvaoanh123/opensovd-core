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

//! Phase 4 Line A D3 — `CdaBackend::preflight` path-prefix mismatch guard.
//!
//! Stands up an in-process mock CDA that serves its REST under a single
//! known prefix (`/sovd/v1/*`, mirroring `InMemoryServer`'s native
//! routes) and runs `CdaBackend::preflight` against it twice:
//!
//! 1. RED: with a `CdaBackend` configured for the wrong prefix
//!    (`vehicle/v15`, the default) — expected to fail with a clear
//!    `SovdError::InvalidRequest` that mentions `path_prefix`.
//! 2. GREEN: with a `CdaBackend` configured for the matching prefix
//!    (`sovd/v1`) — expected to succeed.
//!
//! This is the unit-level counterpart to the `phase4_sovd_gateway_cda_ecusim_bench`
//! live bench guard; it runs on every machine as part of
//! `cargo test --workspace`.

use std::sync::Arc;

use sovd_interfaces::{ComponentId, SovdError};
use sovd_server::{CdaBackend, InMemoryServer, routes};
use tokio::net::TcpListener;
use url::Url;

struct BootedServer {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl BootedServer {
    async fn start(server: Arc<InMemoryServer>) -> Self {
        let app = routes::app_with_server(server);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind random port");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{addr}/");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("server terminated unexpectedly");
        });
        Self { base_url, handle }
    }
}

impl Drop for BootedServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[tokio::test]
async fn preflight_flags_prefix_mismatch_against_mock_cda() {
    // Mock CDA = plain InMemoryServer — serves /sovd/v1/components.
    let mock_cda = Arc::new(InMemoryServer::new_with_demo_data());
    let booted = BootedServer::start(mock_cda).await;
    let base = Url::parse(&booted.base_url).expect("parse mock cda url");

    // --- RED: default CdaBackend::new uses /vehicle/v15 -------------
    let wrong = CdaBackend::new(ComponentId::new("cvc"), base.clone()).expect("build cda backend");
    let err = wrong
        .preflight()
        .await
        .expect_err("preflight must fail when prefix is wrong");
    match err {
        SovdError::InvalidRequest(msg) => {
            assert!(
                msg.contains("path_prefix"),
                "preflight error should mention path_prefix, got: {msg}"
            );
            assert!(
                msg.contains("404"),
                "preflight error should mention the 404 cause, got: {msg}"
            );
        }
        other => panic!("expected SovdError::InvalidRequest, got {other:?}"),
    }

    // --- GREEN: explicit sovd/v1 prefix matches mock CDA ------------
    let right = CdaBackend::new_with_path_prefix(ComponentId::new("cvc"), base.clone(), "sovd/v1")
        .expect("build cda backend with explicit prefix");
    right
        .preflight()
        .await
        .expect("preflight must succeed when prefix matches mock CDA");
}

#[tokio::test]
async fn preflight_reports_backend_unavailable_when_nothing_listens() {
    // A URL that resolves but has nobody on the port — reqwest should
    // treat this as a connect error, and preflight should translate
    // that into SovdError::BackendUnavailable.
    let base = Url::parse("http://127.0.0.1:1/").expect("parse");
    let backend = CdaBackend::new(ComponentId::new("cvc"), base).expect("build");
    let err = backend
        .preflight()
        .await
        .expect_err("preflight must fail when nothing listens");
    assert!(
        matches!(err, SovdError::BackendUnavailable(_)),
        "expected BackendUnavailable, got {err:?}"
    );
}
