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

//! Phase 4 Line A D8 — end-to-end DFM-only chain.
//!
//! Boots the full axum router with an `InMemoryServer` + a DFM
//! forward backed by a temp-dir SQLite database and a fully-populated
//! data + operation catalog. Drives the 10-endpoint surface via
//! `reqwest` and asserts the five MVP use cases:
//!
//! 1. read faults (`GET .../faults`)
//! 2. read single fault (`GET .../faults/{code}`)
//! 3. clear faults (`DELETE .../faults`)
//! 4. read component data catalog (`GET .../data`)
//! 5. start an operation execution (`POST .../executions`)
//!
//! No Pi, no CDA, no external processes — pure Rust-in-Rust.
//! Pure D8 test: every assertion is against spec-typed bodies.

use std::sync::Arc;

use opcycle_taktflow::TaktflowOperationCycle;
use reqwest::StatusCode;
use sovd_db_sqlite::SqliteSovdDb;
use sovd_dfm::Dfm;
use sovd_interfaces::{
    ComponentId,
    extras::fault::{FaultId, FaultRecord, FaultSeverity},
    spec::{
        component::DiscoveredEntities,
        data::Datas,
        fault::{FaultDetails, ListOfFaults},
        operation::{
            ExecutionStatus, ExecutionStatusResponse, OperationsList, StartExecutionAsyncResponse,
            StartExecutionRequest,
        },
    },
    traits::{fault_sink::FaultSink, operation_cycle::OperationCycle, sovd_db::SovdDb},
};
use sovd_server::{InMemoryServer, routes};
use tokio::net::TcpListener;

const COMPONENT: &str = "dfm";

struct Harness {
    base_url: String,
    dfm: Arc<Dfm>,
    _tmp: tempfile::TempDir,
    handle: tokio::task::JoinHandle<()>,
}

impl Harness {
    async fn start() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("phase4_d8.db");
        let db: Arc<dyn SovdDb> = Arc::new(SqliteSovdDb::connect(&db_path).await.expect("sqlite"));
        let cycles: Arc<dyn OperationCycle> = Arc::new(TaktflowOperationCycle::new());

        let operations = vec![
            sovd_interfaces::spec::operation::OperationDescription {
                id: "precharge_check".into(),
                name: Some("HV precharge check".into()),
                translation_id: None,
                proximity_proof_required: false,
                asynchronous_execution: false,
                tags: None,
            },
            sovd_interfaces::spec::operation::OperationDescription {
                id: "long_self_test".into(),
                name: Some("Long DFM self test".into()),
                translation_id: None,
                proximity_proof_required: false,
                asynchronous_execution: true,
                tags: None,
            },
        ];
        let data_catalog = vec![
            sovd_interfaces::spec::data::ValueMetadata {
                id: "hv_pack_voltage".into(),
                name: "HV pack voltage".into(),
                translation_id: None,
                category: "currentData".into(),
                groups: Some(vec!["battery".into()]),
                tags: None,
            },
            sovd_interfaces::spec::data::ValueMetadata {
                id: "dfm_build".into(),
                name: "DFM build identifier".into(),
                translation_id: None,
                category: "identData".into(),
                groups: None,
                tags: None,
            },
        ];

        let dfm = Arc::new(
            Dfm::builder(ComponentId::new(COMPONENT))
                .with_db(Arc::clone(&db))
                .with_cycles(Arc::clone(&cycles))
                .with_operation_catalog(operations)
                .with_data_catalog(data_catalog)
                .build()
                .expect("dfm"),
        );

        let server = Arc::new(InMemoryServer::new_with_demo_data());
        server
            .register_forward(Arc::clone(&dfm) as Arc<_>)
            .await
            .expect("register forward");
        let app = routes::app_with_server(server);

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

impl Drop for Harness {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn fault(offset: u32, severity: FaultSeverity) -> FaultRecord {
    FaultRecord {
        component: ComponentId::new(COMPONENT),
        id: FaultId(offset),
        severity,
        timestamp_ms: u64::from(offset).saturating_add(7_000),
        meta: Some(serde_json::json!({"source": "phase4_d8"})),
    }
}

/// End-to-end DFM-only chain covering all five MVP use cases in one
/// test. We run them in a single `tokio::test` so the axum task is
/// spawned once per run — individual use-case failures land on the
/// specific assertion rather than a fresh tempdir boot per case.
#[tokio::test]
async fn phase4_sovd_dfm_only_chain_five_mvp_use_cases() {
    let harness = Harness::start().await;
    let client = reqwest::Client::new();

    // Sanity: /sovd/v1/components includes the DFM forward and the
    // three demo entities (cvc/fzc/rzc).
    let response = client
        .get(harness.url("/sovd/v1/components"))
        .send()
        .await
        .expect("list components");
    assert_eq!(response.status(), StatusCode::OK);
    let discovered: DiscoveredEntities = response.json().await.expect("discovered json");
    let ids: Vec<String> = discovered.items.iter().map(|e| e.id.clone()).collect();
    assert!(ids.iter().any(|id| id == COMPONENT), "dfm in {ids:?}");

    // Ingest three faults via the FaultSink trait.
    for offset in [0x0Au32, 0x0B, 0x0C] {
        harness
            .dfm
            .record_fault(fault(offset, FaultSeverity::Error).into())
            .await
            .expect("record");
    }

    // --- 1. read faults ----------------------------------------------
    let response = client
        .get(harness.url(&format!("/sovd/v1/components/{COMPONENT}/faults")))
        .send()
        .await
        .expect("list faults");
    assert_eq!(response.status(), StatusCode::OK);
    let list: ListOfFaults = response.json().await.expect("faults json");
    assert_eq!(list.items.len(), 3, "expected 3 faults");
    let first_code = list.items.first().expect("first").code.clone();

    // --- 2. read single fault ----------------------------------------
    let response = client
        .get(harness.url(&format!(
            "/sovd/v1/components/{COMPONENT}/faults/{first_code}"
        )))
        .send()
        .await
        .expect("get fault");
    assert_eq!(response.status(), StatusCode::OK);
    let details: FaultDetails = response.json().await.expect("fault details json");
    assert_eq!(details.item.code, first_code);

    // --- 3. clear faults ---------------------------------------------
    let response = client
        .delete(harness.url(&format!("/sovd/v1/components/{COMPONENT}/faults")))
        .send()
        .await
        .expect("delete all");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = client
        .get(harness.url(&format!("/sovd/v1/components/{COMPONENT}/faults")))
        .send()
        .await
        .expect("list faults after clear");
    let list: ListOfFaults = response.json().await.expect("faults json");
    assert!(list.items.is_empty(), "expected empty after clear");

    // --- 4. read component data catalog ------------------------------
    let response = client
        .get(harness.url(&format!("/sovd/v1/components/{COMPONENT}/data")))
        .send()
        .await
        .expect("list data");
    assert_eq!(response.status(), StatusCode::OK);
    let datas: Datas = response.json().await.expect("datas json");
    let data_ids: Vec<String> = datas.items.iter().map(|v| v.id.clone()).collect();
    assert!(
        data_ids.contains(&"hv_pack_voltage".to_owned())
            && data_ids.contains(&"dfm_build".to_owned()),
        "expected both catalog entries, got {data_ids:?}"
    );

    // --- 5. start operation execution --------------------------------
    //     Also verify list_operations + execution_status per D2
    let response = client
        .get(harness.url(&format!("/sovd/v1/components/{COMPONENT}/operations")))
        .send()
        .await
        .expect("list operations");
    assert_eq!(response.status(), StatusCode::OK);
    let ops: OperationsList = response.json().await.expect("ops json");
    assert!(ops.items.iter().any(|o| o.id == "precharge_check"));

    let body = StartExecutionRequest {
        timeout: Some(30),
        parameters: Some(serde_json::json!({"mode": "full"})),
        proximity_response: None,
    };
    let response = client
        .post(harness.url(&format!(
            "/sovd/v1/components/{COMPONENT}/operations/precharge_check/executions"
        )))
        .json(&body)
        .send()
        .await
        .expect("start exec");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let started: StartExecutionAsyncResponse = response.json().await.expect("started json");
    assert!(!started.id.is_empty());

    let response = client
        .get(harness.url(&format!(
            "/sovd/v1/components/{COMPONENT}/operations/precharge_check/executions/{}",
            started.id
        )))
        .send()
        .await
        .expect("exec status");
    assert_eq!(response.status(), StatusCode::OK);
    let status: ExecutionStatusResponse = response.json().await.expect("status json");
    assert_eq!(status.status, Some(ExecutionStatus::Completed));

    // And /health should report the DFM-backed probe as Ok.
    let response = client
        .get(harness.url("/sovd/v1/health"))
        .send()
        .await
        .expect("health");
    assert_eq!(response.status(), StatusCode::OK);
    let health: sovd_interfaces::extras::health::HealthStatus =
        response.json().await.expect("health json");
    assert_eq!(health.status, "ok");
}
