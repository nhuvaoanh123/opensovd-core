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

//! Phase 5 Line A D3 - HIL scenario 02: clear faults and verify.
//!
//! This test is the code-side companion to
//! `test/hil/scenarios/hil_sovd_02_clear_faults.yaml`.
//!
//! Per `docs/prompts/phase-5-line-a.md`, D3 must add a fault-injection
//! step before issuing the clear request so the bench actually has a
//! fault to clear. Line A does not own the physical injection path, so
//! this test treats injection as an explicit precondition captured in
//! the scenario file and fails loudly if the bench reports an empty
//! fault list before the clear.
//!
//! The live gate stays disabled until BOTH:
//!
//! - `TAKTFLOW_BENCH=1`
//! - `PHASE5_BENCH_READY=1`
//!
//! Once both env vars are set, the test becomes a hard red/green gate:
//! every expected bench component must appear in `/sovd/v1/components`,
//! each scenario-listed `/faults` route must be non-empty before clear,
//! `DELETE .../faults` must return `204 No Content`, and the follow-up
//! `GET` must decode as an empty `ListOfFaults`.

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
    precondition: Precondition,
    inventory_call: InventoryCall,
    calls: Vec<ClearCall>,
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

#[derive(Debug, Deserialize)]
struct Precondition {
    kind: String,
    owner: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct InventoryCall {
    name: String,
    method: String,
    path: String,
    expect_status: u16,
    expect_type: String,
}

#[derive(Clone, Debug, Deserialize)]
struct ClearCall {
    name: String,
    component_id: String,
    list_method: String,
    list_path: String,
    expect_list_status: u16,
    clear_method: String,
    clear_path: String,
    expect_clear_status: u16,
    expect_type: String,
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
        .join("hil_sovd_02_clear_faults.yaml")
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

fn parse_status(value: u16, call_name: &str, field_name: &str) -> StatusCode {
    StatusCode::from_u16(value)
        .unwrap_or_else(|e| panic!("scenario {call_name} has invalid {field_name}={value}: {e}"))
}

async fn verify_inventory(client: &reqwest::Client, scenario: &Scenario) {
    assert_eq!(
        scenario.inventory_call.method, "GET",
        "inventory_call.method must stay GET for the D3 harness"
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
        parse_status(
            scenario.inventory_call.expect_status,
            &scenario.inventory_call.name,
            "expect_status"
        ),
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
}

async fn run_clear_call(client: &reqwest::Client, scenario: &Scenario, call: &ClearCall) {
    assert_eq!(
        call.list_method, "GET",
        "D3 list_method must stay GET; {} drifted",
        call.name
    );
    assert_eq!(
        call.clear_method, "DELETE",
        "D3 clear_method must stay DELETE; {} drifted",
        call.name
    );
    assert_eq!(
        call.expect_type, FAULTS_TYPE,
        "D3 only supports ListOfFaults calls; {} drifted",
        call.name
    );
    assert!(
        scenario
            .fleet
            .expected_components
            .iter()
            .any(|expected| expected == &call.component_id),
        "scenario call {} references non-fleet component {}",
        call.name,
        call.component_id
    );

    let list_url = format!("{}{}", scenario.gate.base_url, call.list_path);
    let response = client
        .get(&list_url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {list_url} failed: {e}"));
    assert_eq!(
        response.status(),
        parse_status(call.expect_list_status, &call.name, "expect_list_status"),
        "{list_url} should satisfy scenario call {} before clear",
        call.name
    );
    let before: ListOfFaults = response
        .json()
        .await
        .unwrap_or_else(|e| panic!("{list_url} body decode failed: {e}"));
    assert!(
        !before.items.is_empty(),
        "phase5 D3 precondition not satisfied for {}: {} (owner: {})",
        call.component_id,
        scenario.precondition.reason,
        scenario.precondition.owner
    );

    let clear_url = format!("{}{}", scenario.gate.base_url, call.clear_path);
    let response = client
        .delete(&clear_url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("DELETE {clear_url} failed: {e}"));
    assert_eq!(
        response.status(),
        parse_status(call.expect_clear_status, &call.name, "expect_clear_status"),
        "{clear_url} should satisfy scenario call {} clear step",
        call.name
    );

    let response = client
        .get(&list_url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {list_url} after clear failed: {e}"));
    assert_eq!(
        response.status(),
        parse_status(call.expect_list_status, &call.name, "expect_list_status"),
        "{list_url} should stay readable after clear"
    );
    let after: ListOfFaults = response
        .json()
        .await
        .unwrap_or_else(|e| panic!("{list_url} post-clear body decode failed: {e}"));
    assert!(
        after.items.is_empty(),
        "expected {} faults to be empty after clear; still saw {} item(s)",
        call.component_id,
        after.items.len()
    );
}

#[tokio::test]
async fn phase5_hil_sovd_02_clear_faults() {
    let scenario = match preflight().await {
        Preflight::Skip(reason) => {
            eprintln!("phase5 D3 skip: {reason}");
            return;
        }
        Preflight::Fail(reason) => panic!("phase5 D3 RED: {reason}"),
        Preflight::Run(scenario) => *scenario,
    };

    assert_eq!(
        scenario.precondition.kind, "fault-lib-injection",
        "D3 precondition.kind must stay fault-lib-injection"
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");

    verify_inventory(&client, &scenario).await;

    for call in &scenario.calls {
        run_clear_call(&client, &scenario, call).await;
    }

    eprintln!(
        "phase5_hil_sovd_02_clear_faults: D3 green against {} for {:?}",
        scenario.gate.tcp_addr, scenario.fleet.expected_components
    );
}
