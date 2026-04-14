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

//! Phase 2 Line A Scenario 1 — CDA + upstream ecu-sim SIL smoke test.
//!
//! This test assumes that a CDA instance is already running on
//! `127.0.0.1:20002` against the Pi-hosted ecu-sim (typically started by
//! `deploy/sil/run-cda-local.sh`). It fires a small suite of SOVD REST
//! calls at that CDA and asserts every response deserializes cleanly into
//! a `sovd_interfaces::spec::*` type per ADR-0015.
//!
//! # Preflight gate
//!
//! The test body only runs when:
//!
//! - the env var `TAKTFLOW_BENCH=1` is set (worker intends to hit the live
//!   bench), AND
//! - a short TCP probe to `192.168.0.197:13400` (Pi ecu-sim `DoIP` port)
//!   succeeds within 1 second.
//!
//! Otherwise the test logs the reason and returns `Ok(())` — this keeps
//! `cargo test --workspace` clean on machines that are not on the bench
//! LAN, without adding a feature flag that would silently hide the test.

use std::{env, net::SocketAddr, time::Duration};

use reqwest::StatusCode;
use sovd_interfaces::spec::{
    component::DiscoveredEntities,
    fault::ListOfFaults,
    operation::{
        ExecutionStatus, ExecutionStatusResponse, OperationsList, StartExecutionAsyncResponse,
        StartExecutionRequest,
    },
};
use tokio::net::TcpStream;

/// Bench CDA endpoint — CDA runs locally on Windows pointing at the Pi
/// ecu-sim, so from the test client's perspective CDA is at loopback.
const CDA_BASE_URL: &str = "http://127.0.0.1:20002";

/// Pi `DoIP` port — this is what we probe for the preflight gate. We do
/// NOT speak `DoIP` from the test; we only use a TCP SYN to confirm the
/// bench is reachable.
const PI_DOIP_ADDR: &str = "192.168.0.197:13400";

/// Env var that opts the worker into running bench-gated tests.
const BENCH_ENV: &str = "TAKTFLOW_BENCH";

async fn bench_reachable() -> bool {
    if env::var(BENCH_ENV).ok().as_deref() != Some("1") {
        eprintln!(
            "skipping phase2 cda+ecusim smoke: {BENCH_ENV}=1 not set (set it to run on the bench LAN)"
        );
        return false;
    }
    let addr: SocketAddr = match PI_DOIP_ADDR.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("skipping phase2 cda+ecusim smoke: bad PI_DOIP_ADDR {PI_DOIP_ADDR}: {e}");
            return false;
        }
    };
    match tokio::time::timeout(Duration::from_secs(1), TcpStream::connect(addr)).await {
        Ok(Ok(_)) => true,
        Ok(Err(e)) => {
            eprintln!("skipping phase2 cda+ecusim smoke: Pi {PI_DOIP_ADDR} not reachable: {e}");
            false
        }
        Err(_) => {
            eprintln!("skipping phase2 cda+ecusim smoke: Pi {PI_DOIP_ADDR} TCP probe timed out");
            false
        }
    }
}

/// GET a URL, assert 200, and dump the raw body to stderr if JSON parse
/// fails so we can diff the response against the spec shape.
async fn get_typed<T: serde::de::DeserializeOwned>(client: &reqwest::Client, url: &str) -> T {
    let resp = client
        .get(url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {url}: network error: {e}"));
    let status = resp.status();
    assert_eq!(status, StatusCode::OK, "GET {url} -> {status}");
    let body = resp
        .text()
        .await
        .unwrap_or_else(|e| panic!("GET {url}: read body: {e}"));
    match serde_json::from_str::<T>(&body) {
        Ok(value) => value,
        Err(e) => panic!("GET {url}: spec parse failed: {e}\n--- raw body ---\n{body}\n---"),
    }
}

#[tokio::test]
async fn phase2_cda_ecusim_smoke() {
    if !bench_reachable().await {
        return;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build reqwest client");

    // 1. GET /sovd/v1/components — expect at least one ECU entry.
    let entities: DiscoveredEntities =
        get_typed(&client, &format!("{CDA_BASE_URL}/sovd/v1/components")).await;
    assert!(
        !entities.items.is_empty(),
        "CDA returned zero components; expected at least one ecu-sim entry"
    );
    let first_component = entities
        .items
        .first()
        .expect("checked non-empty above")
        .id
        .clone();
    eprintln!(
        "phase2 smoke: discovered {} components, probing \"{}\"",
        entities.items.len(),
        first_component
    );

    // 2. GET /sovd/v1/components/{id}/faults — empty or canned list, but
    //    must parse as ListOfFaults.
    let _faults: ListOfFaults = get_typed(
        &client,
        &format!("{CDA_BASE_URL}/sovd/v1/components/{first_component}/faults"),
    )
    .await;

    // 3. GET /sovd/v1/components/{id}/operations — spec OperationsList.
    let operations: OperationsList = get_typed(
        &client,
        &format!("{CDA_BASE_URL}/sovd/v1/components/{first_component}/operations"),
    )
    .await;

    if let Some(op) = operations.items.first() {
        // 4. POST .../operations/{op_id}/executions — trigger simulated
        //    routine; expect an async handle.
        let url = format!(
            "{CDA_BASE_URL}/sovd/v1/components/{first_component}/operations/{}/executions",
            op.id
        );
        let body = StartExecutionRequest {
            timeout: Some(5),
            parameters: None,
            proximity_response: None,
        };
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .expect("POST start execution");
        let status = resp.status();
        let raw = resp.text().await.expect("read start body");
        assert!(status.is_success(), "POST {url} -> {status}; body = {raw}");
        let started: StartExecutionAsyncResponse = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("parse StartExecutionAsyncResponse: {e}; body = {raw}"));

        // 5. GET .../executions/{exec_id} — poll once.
        let exec_url = format!("{url}/{}", started.id);
        let status_resp: ExecutionStatusResponse = get_typed(&client, &exec_url).await;
        assert!(
            matches!(
                status_resp.status,
                Some(
                    ExecutionStatus::Running | ExecutionStatus::Completed | ExecutionStatus::Failed
                )
            ),
            "unexpected execution status: {:?}",
            status_resp.status
        );
    } else {
        eprintln!(
            "phase2 smoke: ecu-sim component \"{first_component}\" exposes no operations; skipping exec flow"
        );
    }
}
