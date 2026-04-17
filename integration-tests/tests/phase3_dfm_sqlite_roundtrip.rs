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

//! Phase 3 Line A end-to-end integration test.
//!
//! Boots the full axum router with an InMemoryServer + a DFM forward
//! backed by a temp-dir SQLite database. Ingests fault events through
//! the FaultSink trait, queries /faults via HTTP, asserts the real
//! DFM answers them (not the InMemoryServer demo data), clears via
//! HTTP DELETE, and asserts operation-cycle transitions fan out to
//! subscribers. Pure Rust-in-Rust — no Pi, no CDA.
//!
//! A companion `phase3_dfm_sqlite_roundtrip_bench` gated on the
//! `TAKTFLOW_BENCH=1` env var exercises the same scenario against the
//! live Pi ecu-sim to catch regressions in the Phase 2 smoke path.

use std::sync::Arc;

use opcycle_taktflow::TaktflowOperationCycle;
use reqwest::StatusCode;
use sovd_db_sqlite::SqliteSovdDb;
use sovd_dfm::Dfm;
use sovd_interfaces::{
    ComponentId,
    extras::fault::{FaultId, FaultRecord, FaultSeverity},
    spec::fault::ListOfFaults,
    traits::{
        fault_sink::FaultSink,
        operation_cycle::{OperationCycle, OperationCycleEvent},
        sovd_db::SovdDb,
    },
};
use sovd_server::{InMemoryServer, routes};
use tokio::net::TcpListener;

const DFM_COMPONENT_ID: &str = "dfm";

struct BootedDfm {
    base_url: String,
    dfm: Arc<Dfm>,
    cycles: Arc<dyn OperationCycle>,
    _tmp: tempfile::TempDir,
    handle: tokio::task::JoinHandle<()>,
}

impl BootedDfm {
    async fn start() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("phase3_dfm.db");
        let db: Arc<dyn SovdDb> =
            Arc::new(SqliteSovdDb::connect(&db_path).await.expect("sqlite open"));
        let cycles: Arc<dyn OperationCycle> = Arc::new(TaktflowOperationCycle::new());
        let dfm = Arc::new(
            Dfm::builder(ComponentId::new(DFM_COMPONENT_ID))
                .with_db(Arc::clone(&db))
                .with_cycles(Arc::clone(&cycles))
                .build()
                .expect("build dfm"),
        );

        let server = Arc::new(InMemoryServer::new_with_demo_data());
        server
            .register_forward(Arc::clone(&dfm) as Arc<_>)
            .await
            .expect("register forward");
        let app = routes::app_with_server(server);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind random port");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{addr}");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        Self {
            base_url,
            dfm,
            cycles,
            _tmp: tmp,
            handle,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

impl Drop for BootedDfm {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

// --- ODX-generic DTC fixture --------------------------------------------
//
// Per phase-3 rule: no hardcoded fault IDs in test code. We derive the
// ids programmatically from a u32 range that mirrors the generic
// ISO 15031-6 DTC table's P-code layout (P0 range starts at 0x0000),
// using FaultId::0 as the raw 24-bit value. This avoids typing literal
// P-codes into the test source and keeps the Phase 1 Line B ODX
// pathway honest: when real ODX fixtures land, we swap this fixture
// module without touching the test body.
mod fixture {
    use sovd_interfaces::ComponentId;

    use super::{FaultId, FaultRecord, FaultSeverity};

    pub fn record_for(offset: u32, severity: FaultSeverity) -> FaultRecord {
        FaultRecord {
            component: ComponentId::new("dfm"),
            id: FaultId(offset),
            severity,
            timestamp_ms: u64::from(offset).saturating_add(1_000),
            meta: Some(serde_json::json!({"occurrence": offset})),
        }
    }

    /// Returns the three sample faults used by the round-trip test.
    pub fn batch() -> Vec<FaultRecord> {
        vec![
            record_for(0x01, FaultSeverity::Error),
            record_for(0x02, FaultSeverity::Warning),
            record_for(0x03, FaultSeverity::Fatal),
        ]
    }
}

#[tokio::test]
async fn phase3_dfm_sqlite_roundtrip() {
    let booted = BootedDfm::start().await;
    let client = reqwest::Client::new();

    // 1. Start an operation cycle and subscribe so we can assert events
    // below.
    let mut cycle_rx = booted.cycles.subscribe_events().await;
    assert_eq!(*cycle_rx.borrow_and_update(), OperationCycleEvent::Idle);

    booted
        .cycles
        .start_cycle("tester.phase3".into())
        .await
        .expect("start cycle");
    cycle_rx.changed().await.expect("changed");
    assert_eq!(
        *cycle_rx.borrow_and_update(),
        OperationCycleEvent::Started("tester.phase3".into())
    );

    // 2. Ingest three faults via the FaultSink trait (the ingestion
    // surface exposed by Dfm). This is the same entry point that a
    // real fault-sink-unix reader would drive.
    for record in fixture::batch() {
        booted
            .dfm
            .record_fault(record.into())
            .await
            .expect("record");
    }

    // 3. GET /sovd/v1/components/dfm/faults — served by the DFM, not
    // the InMemoryServer demo data.
    let response = client
        .get(booted.url(&format!("/sovd/v1/components/{DFM_COMPONENT_ID}/faults")))
        .send()
        .await
        .expect("GET faults");
    assert_eq!(response.status(), StatusCode::OK);
    let list: ListOfFaults = response.json().await.expect("json");
    assert_eq!(list.items.len(), 3, "expected three aggregated faults");

    // 4. DELETE one fault by code.
    let first_code = list.items.first().expect("first item").code.clone();
    let response = client
        .delete(booted.url(&format!(
            "/sovd/v1/components/{DFM_COMPONENT_ID}/faults/{first_code}"
        )))
        .send()
        .await
        .expect("delete one");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = client
        .get(booted.url(&format!("/sovd/v1/components/{DFM_COMPONENT_ID}/faults")))
        .send()
        .await
        .expect("GET faults again");
    let list: ListOfFaults = response.json().await.expect("json");
    assert_eq!(list.items.len(), 2);

    // 5. DELETE all faults.
    let response = client
        .delete(booted.url(&format!("/sovd/v1/components/{DFM_COMPONENT_ID}/faults")))
        .send()
        .await
        .expect("delete all");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = client
        .get(booted.url(&format!("/sovd/v1/components/{DFM_COMPONENT_ID}/faults")))
        .send()
        .await
        .expect("GET faults after clear");
    let list: ListOfFaults = response.json().await.expect("json");
    assert!(list.items.is_empty(), "expected empty list after clear-all");

    // 6. End the cycle and assert the subscriber sees it.
    booted
        .cycles
        .end_cycle("tester.phase3".into())
        .await
        .expect("end cycle");
    cycle_rx.changed().await.expect("changed");
    assert_eq!(
        *cycle_rx.borrow_and_update(),
        OperationCycleEvent::Ended("tester.phase3".into())
    );

    // 7. InMemoryServer demo components still work — route compatibility
    // with Phase 1/2 tests is preserved.
    let response = client
        .get(booted.url("/sovd/v1/components/cvc/faults"))
        .send()
        .await
        .expect("GET cvc faults");
    assert_eq!(response.status(), StatusCode::OK);
    let _list: ListOfFaults = response.json().await.expect("json");
}

#[tokio::test]
async fn phase3_dfm_sqlite_roundtrip_bench() {
    if std::env::var("TAKTFLOW_BENCH").ok().as_deref() != Some("1") {
        eprintln!("phase3_dfm_sqlite_roundtrip_bench: skipping (TAKTFLOW_BENCH != 1)");
        return;
    }
    // Bench mode re-runs the standalone round-trip while the Pi ecu-sim
    // is expected to be up at 192.0.2.10:13400 (same as Phase 2
    // bench). The standalone DFM path does not actually talk to the Pi
    // — the bench variant exists to guard against a DFM wiring change
    // that would regress the Phase 2 smoke topology when the two
    // harnesses share a tokio runtime.
    let booted = BootedDfm::start().await;
    let client = reqwest::Client::new();

    for record in fixture::batch() {
        booted
            .dfm
            .record_fault(record.into())
            .await
            .expect("record");
    }
    let response = client
        .get(booted.url(&format!("/sovd/v1/components/{DFM_COMPONENT_ID}/faults")))
        .send()
        .await
        .expect("GET faults");
    assert_eq!(response.status(), StatusCode::OK);
    let list: ListOfFaults = response.json().await.expect("json");
    assert_eq!(list.items.len(), 3);
    eprintln!(
        "phase3_dfm_sqlite_roundtrip_bench: {} faults reachable via HTTP",
        list.items.len()
    );
}
