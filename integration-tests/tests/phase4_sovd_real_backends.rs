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

#![allow(clippy::doc_markdown, clippy::unnested_or_patterns)]

//! Phase 4 Line A — SOVD server real backends integration tests.
//!
//! These tests drive the Phase 4 deliverables D1..D5 against the full
//! axum router with an `InMemoryServer` + a DFM forward backed by a
//! temp-dir SQLite database. They are pure Rust-in-Rust — no Pi, no
//! CDA, no external processes. The full-chain bench test lives in
//! [`phase4_sovd_gateway_cda_ecusim_bench`](crate::phase4_sovd_gateway_cda_ecusim_bench).
//!
//! # Red-to-green discipline
//!
//! Each deliverable is introduced by a dedicated test that fails on the
//! pre-Phase-4 tree and passes once the delivery is wired. We name each
//! test after its deliverable so bisects are cheap and so the
//! `wrapper-line-a.md` gate log can quote them verbatim.

use std::sync::Arc;

use opcycle_taktflow::TaktflowOperationCycle;
use reqwest::StatusCode;
use sovd_db_sqlite::SqliteSovdDb;
use sovd_dfm::Dfm;
use sovd_interfaces::{
    ComponentId,
    extras::fault::{FaultId, FaultRecord, FaultSeverity},
    spec::{
        component::EntityCapabilities,
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

const DFM_COMPONENT_ID: &str = "dfm";
const ACCEPTED_BEARER_TOKEN: &str = "phase4-test-token";

struct BootedDfm {
    base_url: String,
    dfm: Arc<Dfm>,
    _cycles: Arc<dyn OperationCycle>,
    _tmp: tempfile::TempDir,
    handle: tokio::task::JoinHandle<()>,
}

impl BootedDfm {
    async fn start() -> Self {
        Self::start_with_auth(false).await
    }

    async fn start_with_auth(with_auth: bool) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("phase4_real_backends.db");
        let db: Arc<dyn SovdDb> =
            Arc::new(SqliteSovdDb::connect(&db_path).await.expect("sqlite open"));
        let cycles: Arc<dyn OperationCycle> = Arc::new(TaktflowOperationCycle::new());
        let dfm = Arc::new(
            Dfm::builder(ComponentId::new(DFM_COMPONENT_ID))
                .with_db(Arc::clone(&db))
                .with_cycles(Arc::clone(&cycles))
                .with_operation_catalog(vec![
                    sovd_interfaces::spec::operation::OperationDescription {
                        id: "dfm_self_test".into(),
                        name: Some("DFM self test".into()),
                        translation_id: None,
                        proximity_proof_required: false,
                        asynchronous_execution: false,
                        tags: None,
                    },
                    sovd_interfaces::spec::operation::OperationDescription {
                        id: "dfm_slow_op".into(),
                        name: Some("Slow DFM op".into()),
                        translation_id: None,
                        proximity_proof_required: false,
                        asynchronous_execution: true,
                        tags: None,
                    },
                ])
                .with_data_catalog(vec![
                    sovd_interfaces::spec::data::ValueMetadata {
                        id: "dfm_build_id".into(),
                        name: "DFM build identifier".into(),
                        translation_id: None,
                        category: "identData".into(),
                        groups: None,
                        tags: None,
                    },
                    sovd_interfaces::spec::data::ValueMetadata {
                        id: "dfm_heartbeat".into(),
                        name: "DFM heartbeat count".into(),
                        translation_id: None,
                        category: "currentData".into(),
                        groups: None,
                        tags: None,
                    },
                ])
                .build()
                .expect("build dfm"),
        );

        let server = Arc::new(InMemoryServer::new_with_demo_data());
        server
            .register_forward(Arc::clone(&dfm) as Arc<_>)
            .await
            .expect("register forward");
        let app = if with_auth {
            routes::app_with_auth(
                Arc::clone(&server),
                sovd_server::auth::AuthConfig::new(vec![ACCEPTED_BEARER_TOKEN.into()]),
            )
        } else {
            routes::app_with_server(Arc::clone(&server))
        };

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
            _cycles: cycles,
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

fn record(offset: u32, severity: FaultSeverity) -> FaultRecord {
    FaultRecord {
        component: ComponentId::new(DFM_COMPONENT_ID),
        id: FaultId(offset),
        severity,
        timestamp_ms: u64::from(offset).saturating_add(2_000),
        meta: Some(serde_json::json!({"phase": 4, "offset": offset})),
    }
}

// ------------ D1: /faults routes through real SovdDb backend ------------

#[tokio::test]
async fn d1_get_fault_by_code_dispatches_to_dfm() {
    let booted = BootedDfm::start().await;
    let client = reqwest::Client::new();

    // Ingest one fault via the FaultSink trait (DFM side).
    booted
        .dfm
        .record_fault(record(0xC0FE, FaultSeverity::Error).into())
        .await
        .expect("record fault");

    // List the faults to discover the assigned code.
    let response = client
        .get(booted.url(&format!("/sovd/v1/components/{DFM_COMPONENT_ID}/faults")))
        .send()
        .await
        .expect("GET faults");
    assert_eq!(response.status(), StatusCode::OK);
    let list: ListOfFaults = response.json().await.expect("list json");
    assert_eq!(list.items.len(), 1, "expected single fault from DFM");
    let code = list.items.first().expect("first item").code.clone();

    // The crucial D1 assertion: GET .../faults/{code} must ALSO route
    // through the forward backend (not fall back to the InMemoryServer
    // per-component view). With the Phase 3 tree this returns 404
    // because the handler bypasses the forward map and the DFM
    // component is not a local demo component.
    let response = client
        .get(booted.url(&format!(
            "/sovd/v1/components/{DFM_COMPONENT_ID}/faults/{code}"
        )))
        .send()
        .await
        .expect("GET fault by code");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "get_fault must dispatch through forward backend"
    );
    let details: FaultDetails = response.json().await.expect("details json");
    assert_eq!(details.item.code, code);
}

// ------------ D2: /operations routes via OperationCycle-driven backend ------------

#[tokio::test]
async fn d2_operations_list_dispatches_to_dfm() {
    let booted = BootedDfm::start().await;
    let client = reqwest::Client::new();

    let response = client
        .get(booted.url(&format!(
            "/sovd/v1/components/{DFM_COMPONENT_ID}/operations"
        )))
        .send()
        .await
        .expect("GET operations");
    assert_eq!(response.status(), StatusCode::OK);
    let list: OperationsList = response.json().await.expect("list json");
    let ids: Vec<String> = list.items.iter().map(|o| o.id.clone()).collect();
    assert!(
        ids.iter().any(|id| id == "dfm_self_test"),
        "expected dfm_self_test in catalog, got {ids:?}"
    );
}

#[tokio::test]
async fn d2_start_execution_and_status_lifecycle() {
    let booted = BootedDfm::start().await;
    let client = reqwest::Client::new();

    // POST start execution — a synchronous op finishes immediately.
    let body = StartExecutionRequest {
        timeout: Some(5),
        parameters: Some(serde_json::json!({"mode": "quick"})),
        proximity_response: None,
    };
    let response = client
        .post(booted.url(&format!(
            "/sovd/v1/components/{DFM_COMPONENT_ID}/operations/dfm_self_test/executions"
        )))
        .json(&body)
        .send()
        .await
        .expect("POST exec");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let started: StartExecutionAsyncResponse = response.json().await.expect("started json");
    assert!(!started.id.is_empty());

    // GET the status via the same dispatch path.
    let response = client
        .get(booted.url(&format!(
            "/sovd/v1/components/{DFM_COMPONENT_ID}/operations/dfm_self_test/executions/{}",
            started.id
        )))
        .send()
        .await
        .expect("GET exec status");
    assert_eq!(response.status(), StatusCode::OK);
    let status: ExecutionStatusResponse = response.json().await.expect("status json");
    assert!(matches!(
        status.status,
        Some(ExecutionStatus::Completed) | Some(ExecutionStatus::Running)
    ));
}

// ------------ D3: /components/{id}/data mount ------------

#[tokio::test]
async fn d3_data_route_returns_datas() {
    let booted = BootedDfm::start().await;
    let client = reqwest::Client::new();

    let response = client
        .get(booted.url(&format!("/sovd/v1/components/{DFM_COMPONENT_ID}/data")))
        .send()
        .await
        .expect("GET data");
    assert_eq!(response.status(), StatusCode::OK);
    let datas: Datas = response.json().await.expect("datas json");
    let ids: Vec<String> = datas.items.iter().map(|v| v.id.clone()).collect();
    assert!(
        ids.contains(&"dfm_build_id".to_owned()),
        "expected dfm_build_id in data catalog, got {ids:?}"
    );
}

// ------------ D4: /health reports backend state ------------

#[tokio::test]
async fn d4_health_reports_backend_state() {
    let booted = BootedDfm::start().await;
    let client = reqwest::Client::new();

    let response = client
        .get(booted.url("/sovd/v1/health"))
        .send()
        .await
        .expect("GET health");
    assert_eq!(response.status(), StatusCode::OK);
    let health: sovd_interfaces::extras::health::HealthStatus =
        response.json().await.expect("health json");
    assert_eq!(health.status, "ok");
    assert!(
        matches!(
            health.sovd_db,
            sovd_interfaces::extras::health::BackendProbe::Ok
        ),
        "expected sovd_db probe Ok, got {:?}",
        health.sovd_db
    );
}

// ------------ D5: bearer auth + correlation middleware ------------

#[tokio::test]
async fn d5_unauthenticated_request_returns_401() {
    let booted = BootedDfm::start_with_auth(true).await;
    let client = reqwest::Client::new();

    let response = client
        .get(booted.url("/sovd/v1/health"))
        .send()
        .await
        .expect("GET health");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn d5_authenticated_request_passes_and_correlation_id_propagates() {
    let booted = BootedDfm::start_with_auth(true).await;
    let client = reqwest::Client::new();

    let response = client
        .get(booted.url("/sovd/v1/health"))
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {ACCEPTED_BEARER_TOKEN}"),
        )
        .header("x-request-id", "phase4-correlation-id")
        .send()
        .await
        .expect("GET health");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok()),
        Some("phase4-correlation-id")
    );
}

#[tokio::test]
async fn d5_traceparent_is_accepted_alongside_request_id() {
    let booted = BootedDfm::start_with_auth(true).await;
    let client = reqwest::Client::new();

    let response = client
        .get(booted.url("/sovd/v1/health"))
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {ACCEPTED_BEARER_TOKEN}"),
        )
        .header(
            "traceparent",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        )
        .send()
        .await
        .expect("GET health");
    assert_eq!(response.status(), StatusCode::OK);
    // With traceparent and no x-request-id, we synthesize one from the
    // traceparent's trace-id.
    assert!(
        response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .is_some()
    );
}

// ------------ cross-deliverable: still serves in-memory demo data ------------

#[tokio::test]
async fn phase4_in_memory_components_still_work() {
    let booted = BootedDfm::start().await;
    let client = reqwest::Client::new();
    let response = client
        .get(booted.url("/sovd/v1/components/cvc"))
        .send()
        .await
        .expect("GET cvc");
    assert_eq!(response.status(), StatusCode::OK);
    let caps: EntityCapabilities = response.json().await.expect("caps json");
    assert_eq!(caps.id, "cvc");
}
