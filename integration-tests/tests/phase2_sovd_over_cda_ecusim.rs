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

//! Phase 2 Line A Scenario 2 — sovd-main [`InMemoryServer`] in front of CDA.
//!
//! The architecture slice under test:
//!
//! ```text
//!   test client --HTTP--> sovd-main (InMemoryServer)
//!                              ├── local: bcm faults from demo state
//!                              └── forward: cvc via CdaBackend
//!                                                 ↓ HTTP
//!                                               mock CDA (a second axum
//!                                               running InMemoryServer)
//! ```
//!
//! We stand up a second in-memory SOVD server in-process and treat it as
//! a "mock CDA" for this test. The real bench-hosted CDA + ecu-sim is
//! exercised separately by `phase2_cda_ecusim_smoke`. This test proves
//! the dispatcher code path works end-to-end without needing the bench:
//! the request leaves sovd-main, travels over a real HTTP socket through
//! `CdaBackend`, hits the mock CDA, returns a spec-typed `ListOfFaults`,
//! and is re-serialized back to the client.
//!
//! This is a pure Line A test — no Docker, no `SocketCAN`, no `DoIP`. It runs
//! as part of `cargo test --workspace` on every machine.

use std::sync::Arc;

use reqwest::StatusCode;
use sovd_interfaces::{ComponentId, spec::fault::ListOfFaults};
use sovd_server::{CdaBackend, InMemoryServer, routes};
use tokio::net::TcpListener;
use url::Url;

/// Boots a fresh in-memory SOVD HTTP server on a random loopback port and
/// returns its base URL plus the join handle so the caller can drop it.
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
        let base_url = format!("http://{addr}");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("server terminated unexpectedly");
        });
        Self { base_url, handle }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

impl Drop for BootedServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[tokio::test]
async fn sovd_main_forwards_cvc_via_cda_backend_to_mock_cda() {
    // 1. Mock CDA: a plain InMemoryServer with demo data. Its cvc/fzc/rzc
    //    faults become the "upstream" response the real test expects.
    let mock_cda_server = Arc::new(InMemoryServer::new_with_demo_data());
    let mock_cda = BootedServer::start(mock_cda_server).await;
    let mock_cda_base = Url::parse(&format!("{}/", mock_cda.base_url)).expect("parse mock cda url");

    // 2. sovd-main: another InMemoryServer, but this one has only a
    //    CdaBackend for cvc pointing at mock CDA. We build it from
    //    `new_empty` so the only way cvc can answer is via the forwarded
    //    backend.
    let sovd_main = Arc::new(InMemoryServer::new_empty());
    // The "mock CDA" here is another InMemoryServer that speaks the
    // native sovd-server routes (/sovd/v1/*), not the real upstream
    // cda-sovd prefix (/vehicle/v15/*). Pin the prefix explicitly so
    // this test remains independent of DEFAULT_CDA_PATH_PREFIX — see
    // sovd-server::backends::cda::DEFAULT_CDA_PATH_PREFIX and ADR-0006.
    let cvc_backend =
        CdaBackend::new_with_path_prefix(ComponentId::new("cvc"), mock_cda_base.clone(), "sovd/v1")
            .expect("build CdaBackend for cvc");
    sovd_main
        .register_forward(Arc::new(cvc_backend))
        .await
        .expect("register cvc forward");

    let sovd_frontend = BootedServer::start(Arc::clone(&sovd_main)).await;

    // 3. GET /sovd/v1/components/cvc/faults against sovd-main. sovd-main
    //    has no local cvc state, so if the dispatcher picks the forward
    //    correctly we get mock CDA's canned cvc faults (P0A1F, P0562).
    let client = reqwest::Client::new();
    let resp = client
        .get(sovd_frontend.url("/sovd/v1/components/cvc/faults"))
        .send()
        .await
        .expect("GET cvc faults via sovd-main");
    assert_eq!(resp.status(), StatusCode::OK);
    let faults: ListOfFaults = resp.json().await.expect("parse ListOfFaults");
    assert!(
        faults.items.iter().any(|f| f.code == "P0A1F"),
        "expected mock CDA's cvc faults to round-trip through CdaBackend; got {faults:?}"
    );

    // 4. GET /sovd/v1/components/unknown_ecu/faults against sovd-main.
    //    unknown_ecu is NOT registered either locally or as a forward, so
    //    we expect 404 — this proves the dispatcher does not silently fall
    //    through to a wrong backend.
    let resp = client
        .get(sovd_frontend.url("/sovd/v1/components/unknown_ecu/faults"))
        .send()
        .await
        .expect("GET unknown_ecu faults via sovd-main");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "unknown_ecu has no backend, expected 404"
    );

    // 5. GET /sovd/v1/components against sovd-main should include `cvc`
    //    even though it is only served via a forward backend (discovery
    //    fan-out from InMemoryServer::list_entities).
    let resp = client
        .get(sovd_frontend.url("/sovd/v1/components"))
        .send()
        .await
        .expect("GET components via sovd-main");
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.expect("parse components");
    let empty_vec: Vec<serde_json::Value> = Vec::new();
    let ids: Vec<String> = body
        .get("items")
        .and_then(serde_json::Value::as_array)
        .unwrap_or(&empty_vec)
        .iter()
        .filter_map(|e| {
            e.get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert!(
        ids.contains(&"cvc".to_owned()),
        "expected cvc in discovered entities; got {ids:?}"
    );
}

#[tokio::test]
async fn sovd_main_serves_local_entities_when_no_forward() {
    // Sanity check: with no forwards, sovd-main behaves exactly like the
    // plain in_memory_mvp_flow — cvc/fzc/rzc come from demo data.
    let sovd_main = Arc::new(InMemoryServer::new_with_demo_data());
    let sovd_frontend = BootedServer::start(Arc::clone(&sovd_main)).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(sovd_frontend.url("/sovd/v1/components/cvc/faults"))
        .send()
        .await
        .expect("GET cvc faults locally");
    assert_eq!(resp.status(), StatusCode::OK);
    let faults: ListOfFaults = resp.json().await.expect("parse ListOfFaults");
    assert_eq!(faults.items.len(), 2);
}
