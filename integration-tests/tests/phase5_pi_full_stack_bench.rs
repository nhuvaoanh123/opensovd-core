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

//! Phase 5 Line A D1 — Pi full-stack deployment topology live test.
//!
//! This test exercises a `sovd-main` binary running natively on the
//! Pi bench host (`192.168.0.197:21002`) against the bench fleet it
//! serves. It is the red-side companion of
//! `deploy/pi/phase5-full-stack.sh` — until that script has been
//! executed against the bench, this test fails (nothing is listening
//! on :21002).
//!
//! The test is deliberately narrow: it only validates deliverable D1
//! (topology reachability + `/sovd/v1/components` + per-component
//! `/faults` round-trip). D2..D9 are per the phase-5-line-a.md spec
//! and block on Line B bench readiness; they are not implemented in
//! this test file.
//!
//! # Preflight gate
//!
//! Skips cleanly (returns, does NOT fail) when:
//!
//! - `TAKTFLOW_BENCH` is unset or not `1`
//! - the Pi `:21002` TCP endpoint is not reachable within 1 s
//!
//! When the bench *is* reachable but `sovd-main` is not yet running
//! on the Pi, the test fails — that is the red signal the D1 deploy
//! script has to satisfy.
//!
//! # Port plan (per phase-5-line-a.md)
//!
//! - `sovd-main` on the Pi: `192.168.0.197:21002`
//! - ecu-sim on the Pi: `192.168.0.197:13400` (pre-existing)
//! - proxy on the Pi: `192.168.0.197:13401` (Phase 2 Line B, gated on
//!   bench readiness — NOT probed by this test)

use std::{env, net::SocketAddr, time::Duration};

use reqwest::StatusCode;
use sovd_interfaces::spec::{component::DiscoveredEntities, fault::ListOfFaults};
use tokio::net::TcpStream;

const PI_SOVD_MAIN_ADDR: &str = "192.168.0.197:21002";
const PI_SOVD_MAIN_BASE_URL: &str = "http://192.168.0.197:21002";
const BENCH_ENV: &str = "TAKTFLOW_BENCH";

/// Expected bench components served by `sovd-main` in-memory demo
/// data (see `sovd-server::InMemoryServer::new_with_demo_data`). A
/// Pi-side deploy that runs `sovd-main` without wiring a real DFM
/// backend still exposes these three component ids, which is the
/// minimum D1 green signal.
const EXPECTED_COMPONENTS: &[&str] = &["cvc", "fzc", "rzc"];

/// Result of the preflight probe.
enum Preflight {
    /// Bench env var unset — test must skip cleanly (pass).
    Skip,
    /// Bench env var set and sovd-main on the Pi is reachable — run.
    Run,
    /// Bench env var set but sovd-main is not reachable — fail red
    /// with a diagnostic pointing at the D1 deploy script.
    FailNotDeployed(String),
}

async fn preflight() -> Preflight {
    if env::var(BENCH_ENV).ok().as_deref() != Some("1") {
        eprintln!(
            "skipping phase5 D1 full-stack bench: {BENCH_ENV}=1 not set (set it to run on the bench LAN)"
        );
        return Preflight::Skip;
    }
    let addr: SocketAddr = match PI_SOVD_MAIN_ADDR.parse() {
        Ok(a) => a,
        Err(e) => {
            return Preflight::FailNotDeployed(format!(
                "bad PI_SOVD_MAIN_ADDR {PI_SOVD_MAIN_ADDR}: {e}"
            ));
        }
    };
    match tokio::time::timeout(Duration::from_secs(1), TcpStream::connect(addr)).await {
        Ok(Ok(_)) => Preflight::Run,
        Ok(Err(e)) => Preflight::FailNotDeployed(format!(
            "Pi sovd-main {PI_SOVD_MAIN_ADDR} not reachable: {e} (run deploy/pi/phase5-full-stack.sh against the bench)"
        )),
        Err(_) => Preflight::FailNotDeployed(format!(
            "Pi sovd-main {PI_SOVD_MAIN_ADDR} TCP probe timed out (run deploy/pi/phase5-full-stack.sh against the bench)"
        )),
    }
}

#[tokio::test]
async fn phase5_pi_full_stack_bench() {
    match preflight().await {
        Preflight::Skip => return,
        Preflight::FailNotDeployed(reason) => {
            panic!("phase5 D1 RED: {reason}");
        }
        Preflight::Run => {}
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");

    // --- 1. /sovd/v1/components returns the bench fleet --------------
    let components_url = format!("{PI_SOVD_MAIN_BASE_URL}/sovd/v1/components");
    let response = client
        .get(&components_url)
        .send()
        .await
        .expect("GET /sovd/v1/components");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "sovd-main on Pi should answer /sovd/v1/components with 200"
    );
    let discovered: DiscoveredEntities = response
        .json()
        .await
        .expect("/sovd/v1/components body must decode as DiscoveredEntities");
    let ids: Vec<String> = discovered.items.iter().map(|e| e.id.clone()).collect();
    for expected in EXPECTED_COMPONENTS {
        assert!(
            ids.iter().any(|id| id == expected),
            "Pi sovd-main /components missing expected bench id {expected}; saw {ids:?}"
        );
    }

    // --- 2. per-component /faults round-trip ------------------------
    for component in EXPECTED_COMPONENTS {
        let faults_url = format!("{PI_SOVD_MAIN_BASE_URL}/sovd/v1/components/{component}/faults");
        let response = client
            .get(&faults_url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {faults_url} failed: {e}"));
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Pi sovd-main /components/{component}/faults should return 200"
        );
        let _list: ListOfFaults = response
            .json()
            .await
            .unwrap_or_else(|e| panic!("/components/{component}/faults body decode failed: {e}"));
    }

    eprintln!(
        "phase5_pi_full_stack_bench: D1 topology green against {PI_SOVD_MAIN_ADDR} for {EXPECTED_COMPONENTS:?}"
    );
}
