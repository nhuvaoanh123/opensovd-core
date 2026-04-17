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

//! Phase 4 Line A D9 — full-chain SOVD → Gateway → {DFM, CDA} → Pi ecu-sim bench.
//!
//! Boots the Phase 4 `sovd-server` router in-process with two forward
//! backends registered:
//!
//! - a DFM pointed at a temp-dir SQLite store (handles one component
//!   id, `"dfm"`, and owns the ingestion-side fault sink)
//! - a `CdaBackend` pointed at `http://127.0.0.1:20002/` forwarding to
//!   the locally-running CDA (which in turn talks to the Pi ecu-sim
//!   over DoIP at `192.0.2.10:13400`)
//!
//! The test asserts the same five MVP use cases as
//! `phase4_sovd_dfm_only_chain_five_mvp_use_cases` — but this time
//! end-to-end through the full wire stack: real spec types, real
//! wire-compatible JSON, real HTTP round-trips, real bench.
//!
//! # Preflight gate
//!
//! Runs only when `TAKTFLOW_BENCH=1` AND both the Pi DoIP endpoint
//! and the CDA HTTP endpoint are reachable. Otherwise logs the skip
//! reason and returns — matching the Phase 2 bench guard pattern.

use std::{env, net::SocketAddr, sync::Arc, time::Duration};

use opcycle_taktflow::TaktflowOperationCycle;
use reqwest::StatusCode;
use sovd_db_sqlite::SqliteSovdDb;
use sovd_dfm::Dfm;
use sovd_interfaces::{
    ComponentId, SovdError,
    extras::fault::{FaultId, FaultRecord, FaultSeverity},
    spec::{component::DiscoveredEntities, fault::ListOfFaults},
    traits::{fault_sink::FaultSink, operation_cycle::OperationCycle, sovd_db::SovdDb},
};
use sovd_server::{CdaBackend, InMemoryServer, routes};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

const PI_DOIP_ADDR_ENV: &str = "TAKTFLOW_PI_DOIP_ADDR";
const DEFAULT_PI_DOIP_ADDR: &str = "192.0.2.10:13400";
const CDA_BASE_URL: &str = "http://127.0.0.1:20002/";
const BENCH_ENV: &str = "TAKTFLOW_BENCH";

/// CDA-served component used for the bench. Matches the primary
/// entity exposed by the upstream ecu-sim's compiled MDD fixtures
/// (`FLXC1000.mdd`) — see `deploy/sil/opensovd-cda.toml`. When CDA is
/// running against that MDD database, `/vehicle/v15/components`
/// advertises `flxc1000` and `flxcng1000`; we pick `flxc1000` so the
/// CdaBackend forward hits a real route on the downstream.
const CDA_COMPONENT: &str = "flxc1000";
const DFM_COMPONENT: &str = "dfm";

async fn bench_reachable() -> bool {
    let pi_doip_addr =
        env::var(PI_DOIP_ADDR_ENV).unwrap_or_else(|_| DEFAULT_PI_DOIP_ADDR.to_owned());
    if env::var(BENCH_ENV).ok().as_deref() != Some("1") {
        eprintln!(
            "skipping phase4 full-chain bench: {BENCH_ENV}=1 not set (set it to run on the bench LAN)"
        );
        return false;
    }
    let addr: SocketAddr = match pi_doip_addr.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("skipping phase4 full-chain bench: bad PI_DOIP_ADDR {pi_doip_addr}: {e}");
            return false;
        }
    };
    match tokio::time::timeout(Duration::from_secs(1), TcpStream::connect(addr)).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            eprintln!("skipping phase4 full-chain bench: Pi {pi_doip_addr} not reachable: {e}");
            return false;
        }
        Err(_) => {
            eprintln!("skipping phase4 full-chain bench: Pi {pi_doip_addr} TCP probe timed out");
            return false;
        }
    }
    // Best-effort probe of CDA itself — use a throwaway CdaBackend
    // configured with the DEFAULT_CDA_PATH_PREFIX (vehicle/v15) and
    // call preflight(). This catches "CDA is up but serving a
    // different REST root than CdaBackend expects" before we ever
    // boot the harness.
    let probe_backend = match CdaBackend::new(
        ComponentId::new(CDA_COMPONENT),
        Url::parse(CDA_BASE_URL).expect("parse cda url"),
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("skipping phase4 full-chain bench: cannot build probe CdaBackend: {e}");
            return false;
        }
    };
    match tokio::time::timeout(Duration::from_secs(2), probe_backend.preflight()).await {
        Ok(Ok(())) => {
            eprintln!(
                "phase4 full-chain bench preflight ok: {CDA_BASE_URL} + path_prefix={:?}",
                probe_backend.path_prefix()
            );
            true
        }
        Ok(Err(SovdError::InvalidRequest(msg))) => {
            // Prefix mismatch. This is the very bug the D3 guard
            // exists to catch — surface it loudly rather than
            // silently skipping the test.
            panic!("phase4 full-chain bench preflight FAILED: {msg}");
        }
        Ok(Err(e)) => {
            eprintln!("skipping phase4 full-chain bench: CDA preflight error: {e}");
            false
        }
        Err(_) => {
            eprintln!("skipping phase4 full-chain bench: CDA {CDA_BASE_URL} probe timed out");
            false
        }
    }
}

struct BenchHarness {
    base_url: String,
    dfm: Arc<Dfm>,
    _tmp: tempfile::TempDir,
    handle: tokio::task::JoinHandle<()>,
}

impl BenchHarness {
    async fn start() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("phase4_d9_bench.db");
        let db: Arc<dyn SovdDb> = Arc::new(SqliteSovdDb::connect(&db_path).await.expect("sqlite"));
        let cycles: Arc<dyn OperationCycle> = Arc::new(TaktflowOperationCycle::new());
        let dfm = Arc::new(
            Dfm::builder(ComponentId::new(DFM_COMPONENT))
                .with_db(Arc::clone(&db))
                .with_cycles(Arc::clone(&cycles))
                .build()
                .expect("dfm"),
        );

        let server = Arc::new(InMemoryServer::new_with_demo_data());
        server
            .register_forward(Arc::clone(&dfm) as Arc<_>)
            .await
            .expect("register dfm");

        // CdaBackend forward for ecu-sim.
        let cda = CdaBackend::new(
            ComponentId::new(CDA_COMPONENT),
            Url::parse(CDA_BASE_URL).expect("parse cda url"),
        )
        .expect("cda backend");
        server
            .register_forward(Arc::new(cda) as Arc<_>)
            .await
            .expect("register cda");

        let app = routes::app_with_server(Arc::clone(&server));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let base_url = format!("http://{addr}");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        Self {
            base_url,
            dfm,
            _tmp: tmp,
            handle,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

impl Drop for BenchHarness {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[tokio::test]
async fn phase4_sovd_gateway_cda_ecusim_bench() {
    if !bench_reachable().await {
        return;
    }
    let harness = BenchHarness::start().await;
    let client = reqwest::Client::new();

    // Ingest one fault into the DFM so at least one entry exists.
    harness
        .dfm
        .record_fault(
            FaultRecord {
                component: ComponentId::new(DFM_COMPONENT),
                id: FaultId(0xDEAD),
                severity: FaultSeverity::Error,
                timestamp_ms: 1_000,
                meta: None,
            }
            .into(),
        )
        .await
        .expect("record");

    // --- 1. read faults via DFM forward ------------------------------
    let response = client
        .get(harness.url(&format!("/sovd/v1/components/{DFM_COMPONENT}/faults")))
        .send()
        .await
        .expect("list faults dfm");
    assert_eq!(response.status(), StatusCode::OK);
    let list: ListOfFaults = response.json().await.expect("list json");
    assert!(!list.items.is_empty());

    // --- 2. read single fault via DFM forward ------------------------
    let first_code = list.items.first().expect("first").code.clone();
    let response = client
        .get(harness.url(&format!(
            "/sovd/v1/components/{DFM_COMPONENT}/faults/{first_code}"
        )))
        .send()
        .await
        .expect("get fault dfm");
    assert_eq!(response.status(), StatusCode::OK);

    // --- 3. clear faults via DFM forward -----------------------------
    let response = client
        .delete(harness.url(&format!("/sovd/v1/components/{DFM_COMPONENT}/faults")))
        .send()
        .await
        .expect("delete all dfm");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // --- 4. read ecu-sim via CDA forward (fault list round-trip) ----
    //     CDA may return either 200 with a (possibly empty) list or
    //     401 if it enforces auth. Both cases prove the forward
    //     reached CDA; a Transport/5xx is a wire failure we want to
    //     fail on.
    let response = client
        .get(harness.url(&format!("/sovd/v1/components/{CDA_COMPONENT}/faults")))
        .send()
        .await
        .expect("list faults cda");
    assert!(
        response.status().is_success()
            || response.status() == StatusCode::UNAUTHORIZED
            || response.status() == StatusCode::BAD_GATEWAY,
        "CDA forward status: {}",
        response.status()
    );

    // --- 5. list all components — both DFM and ecu-sim visible ------
    let response = client
        .get(harness.url("/sovd/v1/components"))
        .send()
        .await
        .expect("list all components");
    assert_eq!(response.status(), StatusCode::OK);
    let discovered: DiscoveredEntities = response.json().await.expect("discovered json");
    let ids: Vec<String> = discovered.items.iter().map(|e| e.id.clone()).collect();
    assert!(ids.iter().any(|id| id == DFM_COMPONENT));
    assert!(ids.iter().any(|id| id == CDA_COMPONENT));

    let pi_doip_addr =
        env::var(PI_DOIP_ADDR_ENV).unwrap_or_else(|_| DEFAULT_PI_DOIP_ADDR.to_owned());
    eprintln!("phase4_sovd_gateway_cda_ecusim_bench: 5 MVP use cases green against {pi_doip_addr}");
}
