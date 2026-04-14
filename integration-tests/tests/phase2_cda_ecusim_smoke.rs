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
//! # Path prefix note
//!
//! Upstream Eclipse `OpenSOVD` CDA exposes SOVD routes under `/vehicle/v15`
//! (the historical SOVD 1.0 draft path). Our Phase 2 Line B server will
//! expose `/sovd/v1`, so `sovd_interfaces::spec::*` docs reference the
//! `/sovd/v1/...` surface. For SIL tests that hit CDA directly we use
//! CDA's native `/vehicle/v15` prefix — the response BODY shape is
//! identical because both sides derive from the same SOVD schemas.
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

use reqwest::{
    Response, StatusCode,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::Deserialize;
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

/// Minimal view of CDA's `/vehicle/v15/authorize` response body (the upstream
/// default security plugin returns an `AuthBody` shape — we only need the
/// `access_token` field).
#[derive(Deserialize)]
struct AuthBody {
    access_token: String,
}

/// Acquire a Bearer token from CDA's upstream default security plugin. When
/// CDA is built without the `auth` feature (which is what we build in
/// Phase 2 Line A), any `client_id`/`client_secret` pair yields a JWT.
async fn acquire_bearer(client: &reqwest::Client) -> String {
    let url = format!("{CDA_BASE_URL}/vehicle/v15/authorize");
    let body = serde_json::json!({
        "client_id": "taktflow-phase2-smoke",
        "client_secret": "unused-without-auth-feature",
    });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("POST {url}: network error: {e}"));
    let status = resp.status();
    let raw = resp
        .text()
        .await
        .unwrap_or_else(|e| panic!("POST {url}: read body: {e}"));
    assert!(status.is_success(), "POST {url} -> {status}; body = {raw}");
    let auth: AuthBody =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse AuthBody: {e}; body = {raw}"));
    auth.access_token
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

/// GET a URL that MAY be absent from the upstream ecu-sim's simulated ECU
/// catalog (for example `/faults` when the ECU has no DTC services, or
/// `/operations` when it has no routine controls). Returns `Some(value)`
/// on 200, `None` on 404. Any other status code is a hard failure.
async fn get_optional<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> Option<T> {
    let resp: Response = client
        .get(url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {url}: network error: {e}"));
    let status = resp.status();
    if status == StatusCode::NOT_FOUND {
        eprintln!("phase2 smoke: {url} -> 404 (absent from ecu-sim catalog; accepted)");
        return None;
    }
    assert_eq!(status, StatusCode::OK, "GET {url} -> {status}");
    let body = resp
        .text()
        .await
        .unwrap_or_else(|e| panic!("GET {url}: read body: {e}"));
    match serde_json::from_str::<T>(&body) {
        Ok(value) => Some(value),
        Err(e) => panic!("GET {url}: spec parse failed: {e}\n--- raw body ---\n{body}\n---"),
    }
}

#[tokio::test]
async fn phase2_cda_ecusim_smoke() {
    if !bench_reachable().await {
        return;
    }

    // Unauthenticated client just for /authorize.
    let auth_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build reqwest auth client");

    let token = acquire_bearer(&auth_client).await;
    eprintln!(
        "phase2 smoke: acquired Bearer token ({} bytes)",
        token.len()
    );

    // Authenticated client — all subsequent calls carry the Bearer token.
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).expect("valid auth header"),
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .default_headers(headers)
        .build()
        .expect("build reqwest client");

    // 1. GET /vehicle/v15/components — expect at least one ECU entry.
    let entities: DiscoveredEntities =
        get_typed(&client, &format!("{CDA_BASE_URL}/vehicle/v15/components")).await;
    assert!(
        !entities.items.is_empty(),
        "CDA returned zero components; expected at least one ecu-sim entry"
    );
    // Pick the ECU that actually has an MDD database match — the upstream
    // ecu-sim advertises two logical entities (FLXC1000 and FSNR2000) but
    // our MDD fixtures only cover FLXC1000 / FLXCNG1000. Any component whose
    // id starts with "flxc" is a match; otherwise fall back to the first
    // discovered entity and let later asserts surface the mismatch.
    let first_component = entities
        .items
        .iter()
        .find(|e| e.id.starts_with("flxc"))
        .or_else(|| entities.items.first())
        .expect("checked non-empty above")
        .id
        .clone();
    eprintln!(
        "phase2 smoke: discovered {} components, probing \"{}\"",
        entities.items.len(),
        first_component
    );

    // 2. GET /vehicle/v15/components/{id}/faults — the upstream ecu-sim's
    //    simulated FLXC1000 has no DTC services in its MDD catalog, so CDA
    //    returns 404 ("No services with SID 0x19 found..."). That is a
    //    valid upstream-catalog-sparse state, not a bug. If the endpoint
    //    does return 200 we still verify it parses as `ListOfFaults`.
    let _faults: Option<ListOfFaults> = get_optional(
        &client,
        &format!("{CDA_BASE_URL}/vehicle/v15/components/{first_component}/faults"),
    )
    .await;

    // 3. GET /vehicle/v15/components/{id}/operations — likewise may be 404
    //    when the MDD exposes no routine-control services for this ECU.
    let operations: Option<OperationsList> = get_optional(
        &client,
        &format!("{CDA_BASE_URL}/vehicle/v15/components/{first_component}/operations"),
    )
    .await;

    if let Some(op) = operations.as_ref().and_then(|o| o.items.first()) {
        // 4. POST .../operations/{op_id}/executions — trigger simulated
        //    routine; expect an async handle.
        let url = format!(
            "{CDA_BASE_URL}/vehicle/v15/components/{first_component}/operations/{}/executions",
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

    eprintln!(
        "phase2 smoke: end-to-end SOVD -> CDA -> DoIP -> ecu-sim round-trip OK \
         (auth, component discovery, and spec-typed DiscoveredEntities verified)"
    );
}
