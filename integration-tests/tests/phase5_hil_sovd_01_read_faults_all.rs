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

//! Phase 5 Line A D2 - HIL scenario 01: read faults across the bench fleet.
//!
//! This test is the code-side companion to
//! `test/hil/scenarios/hil_sovd_01_read_faults_all.yaml`.
//!
//! Per `docs/prompts/phase-5-line-a.md`, the scenario must exist now,
//! but the live bench gate must remain cleanly skippable until Line B
//! declares the full fleet ready. For that reason the test only runs
//! when BOTH:
//!
//! - `TAKTFLOW_BENCH=1`
//! - `PHASE5_BENCH_READY=1`
//!
//! Once both env vars are set, the test becomes a hard red/green gate:
//! every expected bench component must appear in `/sovd/v1/components`
//! and each scenario-listed `/faults` endpoint must answer `200 OK`
//! with a `ListOfFaults` body.

mod common;

use std::{env, fs, net::SocketAddr, path::PathBuf, time::Duration};

use common::override_pi_sovd_gate;
use reqwest::StatusCode;
use serde::Deserialize;
use sovd_interfaces::spec::{component::DiscoveredEntities, fault::ListOfFaults};
use tokio::net::TcpStream;

const INVENTORY_TYPE: &str = "sovd_interfaces::spec::component::DiscoveredEntities";
const FAULTS_TYPE: &str = "sovd_interfaces::spec::fault::ListOfFaults";

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    gate: Gate,
    fleet: Fleet,
    inventory_call: ScenarioCall,
    calls: Vec<ScenarioCall>,
}

#[derive(Debug, Deserialize)]
struct Gate {
    bench_env: String,
    bench_env_value: String,
    readiness_env: String,
    readiness_env_value: String,
    tcp_addr: String,
    base_url: String,
    not_ready_reason: String,
}

#[derive(Debug, Deserialize)]
struct Fleet {
    expected_components: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ScenarioCall {
    name: String,
    method: String,
    path: String,
    expect_status: u16,
    expect_type: String,
    component_id: Option<String>,
}

enum Preflight {
    Skip(String),
    Run(Box<Scenario>),
    Fail(String),
}

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("integration-tests has workspace parent")
        .join("test")
        .join("hil")
        .join("scenarios")
        .join("hil_sovd_01_read_faults_all.yaml")
}

fn load_scenario() -> Scenario {
    let path = scenario_path();
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut scenario: Scenario =
        serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    override_pi_sovd_gate(&mut scenario.gate.tcp_addr, &mut scenario.gate.base_url);
    scenario
}

async fn preflight() -> Preflight {
    let scenario = load_scenario();
    if env::var(&scenario.gate.bench_env).ok().as_deref() != Some(&scenario.gate.bench_env_value) {
        return Preflight::Skip(format!(
            "{}={} not set; skipping {}",
            scenario.gate.bench_env, scenario.gate.bench_env_value, scenario.name
        ));
    }
    if env::var(&scenario.gate.readiness_env).ok().as_deref()
        != Some(&scenario.gate.readiness_env_value)
    {
        return Preflight::Skip(format!(
            "{}={} not set; {}",
            scenario.gate.readiness_env,
            scenario.gate.readiness_env_value,
            scenario.gate.not_ready_reason
        ));
    }
    let addr: SocketAddr = match scenario.gate.tcp_addr.parse() {
        Ok(addr) => addr,
        Err(e) => {
            return Preflight::Fail(format!(
                "bad tcp_addr {} in {}: {e}",
                scenario.gate.tcp_addr,
                scenario_path().display()
            ));
        }
    };
    match tokio::time::timeout(Duration::from_secs(1), TcpStream::connect(addr)).await {
        Ok(Ok(_)) => Preflight::Run(Box::new(scenario)),
        Ok(Err(e)) => Preflight::Fail(format!(
            "Pi sovd-main {} not reachable: {e} (run deploy/pi/phase5-full-stack.sh first)",
            scenario.gate.tcp_addr
        )),
        Err(_) => Preflight::Fail(format!(
            "Pi sovd-main {} TCP probe timed out (run deploy/pi/phase5-full-stack.sh first)",
            scenario.gate.tcp_addr
        )),
    }
}

fn parse_status(call: &ScenarioCall) -> StatusCode {
    StatusCode::from_u16(call.expect_status).unwrap_or_else(|e| {
        panic!(
            "scenario {} has invalid expect_status {}: {e}",
            call.name, call.expect_status
        )
    })
}

#[tokio::test]
async fn phase5_hil_sovd_01_read_faults_all() {
    let scenario = match preflight().await {
        Preflight::Skip(reason) => {
            eprintln!("phase5 D2 skip: {reason}");
            return;
        }
        Preflight::Fail(reason) => panic!("phase5 D2 RED: {reason}"),
        Preflight::Run(scenario) => *scenario,
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");

    assert_eq!(
        scenario.inventory_call.method, "GET",
        "inventory_call.method must stay GET for this narrow D2 harness"
    );
    assert_eq!(
        scenario.inventory_call.expect_type, INVENTORY_TYPE,
        "inventory_call.expect_type must stay {INVENTORY_TYPE}"
    );
    let inventory_url = format!("{}{}", scenario.gate.base_url, scenario.inventory_call.path);
    let response = client
        .get(&inventory_url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {inventory_url} failed: {e}"));
    assert_eq!(
        response.status(),
        parse_status(&scenario.inventory_call),
        "{inventory_url} should satisfy the scenario contract"
    );
    let discovered: DiscoveredEntities = response
        .json()
        .await
        .unwrap_or_else(|e| panic!("inventory decode failed: {e}"));
    let ids: Vec<String> = discovered
        .items
        .iter()
        .map(|item| item.id.clone())
        .collect();
    for expected in &scenario.fleet.expected_components {
        assert!(
            ids.iter().any(|id| id == expected),
            "bench inventory missing expected component {expected}; saw {ids:?}"
        );
    }

    for call in &scenario.calls {
        assert_eq!(
            call.method, "GET",
            "D2 only supports GET /faults calls; {} drifted",
            call.name
        );
        assert_eq!(
            call.expect_type, FAULTS_TYPE,
            "D2 only supports ListOfFaults calls; {} drifted",
            call.name
        );
        let url = format!("{}{}", scenario.gate.base_url, call.path);
        let response = client
            .get(&url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {url} failed: {e}"));
        assert_eq!(
            response.status(),
            parse_status(call),
            "{url} should satisfy scenario call {}",
            call.name
        );
        let _list: ListOfFaults = response
            .json()
            .await
            .unwrap_or_else(|e| panic!("{url} body decode failed: {e}"));
        if let Some(component_id) = &call.component_id {
            assert!(
                scenario
                    .fleet
                    .expected_components
                    .iter()
                    .any(|expected| expected == component_id),
                "scenario call {} references non-fleet component {component_id}",
                call.name
            );
        }
    }

    eprintln!(
        "phase5_hil_sovd_01_read_faults_all: D2 green against {} for {:?}",
        scenario.gate.tcp_addr, scenario.fleet.expected_components
    );
}
