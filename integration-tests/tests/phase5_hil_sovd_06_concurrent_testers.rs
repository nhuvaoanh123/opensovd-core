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

//! Phase 5 Line A D7 - HIL scenario 06: concurrent testers.
//!
//! This test runs two independent reqwest clients in parallel, each
//! executing a different read+clear sequence over the fault endpoints.
//! The test fails on any deadlock-like timeout, non-200/204 response, or
//! missing state transition under concurrent load.

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
    testers: Vec<Tester>,
    faults_call_contract: FaultsCallContract,
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
struct Tester {
    name: String,
    components: Vec<TesterComponent>,
}

#[derive(Clone, Debug, Deserialize)]
struct TesterComponent {
    id: String,
    list_path: String,
    clear_path: String,
}

#[derive(Debug, Deserialize)]
struct FaultsCallContract {
    list_method: String,
    clear_method: String,
    expect_list_status: u16,
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
        .join("hil_sovd_06_concurrent_testers.yaml")
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
        "inventory_call.method must stay GET for the D7 harness"
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

async fn run_tester_sequence(
    client: reqwest::Client,
    base_url: String,
    tester: Tester,
    contract: FaultsCallContract,
    expected_components: Vec<String>,
    precondition: Precondition,
) {
    for component in tester.components {
        assert_eq!(
            contract.list_method, "GET",
            "D7 list_method must stay GET for {}",
            tester.name
        );
        assert_eq!(
            contract.clear_method, "DELETE",
            "D7 clear_method must stay DELETE for {}",
            tester.name
        );
        assert_eq!(
            contract.expect_type, FAULTS_TYPE,
            "D7 expect_type must stay {FAULTS_TYPE}"
        );
        assert!(
            expected_components
                .iter()
                .any(|expected| expected == &component.id),
            "tester {} references non-fleet component {}",
            tester.name,
            component.id
        );

        let list_url = format!("{base_url}{}", component.list_path);
        let response = client
            .get(&list_url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {list_url} for {} failed: {e}", tester.name));
        assert_eq!(
            response.status(),
            parse_status(
                contract.expect_list_status,
                &tester.name,
                "expect_list_status"
            ),
            "{list_url} should satisfy {} before clear",
            tester.name
        );
        let before: ListOfFaults = response
            .json()
            .await
            .unwrap_or_else(|e| panic!("{list_url} decode for {} failed: {e}", tester.name));
        assert!(
            !before.items.is_empty(),
            "phase5 D7 precondition not satisfied for {} via {}: {} (owner: {})",
            component.id,
            tester.name,
            precondition.reason,
            precondition.owner
        );

        let clear_url = format!("{base_url}{}", component.clear_path);
        let response = client
            .delete(&clear_url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("DELETE {clear_url} for {} failed: {e}", tester.name));
        assert_eq!(
            response.status(),
            parse_status(
                contract.expect_clear_status,
                &tester.name,
                "expect_clear_status"
            ),
            "{clear_url} should satisfy {} clear step",
            tester.name
        );

        let response = client.get(&list_url).send().await.unwrap_or_else(|e| {
            panic!("GET {list_url} after clear for {} failed: {e}", tester.name)
        });
        assert_eq!(
            response.status(),
            parse_status(
                contract.expect_list_status,
                &tester.name,
                "expect_list_status"
            ),
            "{list_url} should stay readable after clear for {}",
            tester.name
        );
        let after: ListOfFaults = response.json().await.unwrap_or_else(|e| {
            panic!(
                "{list_url} post-clear decode for {} failed: {e}",
                tester.name
            )
        });
        assert!(
            after.items.is_empty(),
            "expected {} faults to be empty after {} clear; still saw {} item(s)",
            component.id,
            tester.name,
            after.items.len()
        );
    }
}

#[tokio::test]
async fn phase5_hil_sovd_06_concurrent_testers() {
    let scenario = match preflight().await {
        Preflight::Skip(reason) => {
            eprintln!("phase5 D7 skip: {reason}");
            return;
        }
        Preflight::Fail(reason) => panic!("phase5 D7 RED: {reason}"),
        Preflight::Run(scenario) => *scenario,
    };

    assert_eq!(
        scenario.precondition.kind, "fault-lib-injection",
        "D7 precondition.kind must stay fault-lib-injection"
    );
    assert_eq!(
        scenario.testers.len(),
        2,
        "D7 currently expects exactly two concurrent tester sequences"
    );
    let mut testers = scenario.testers.iter().cloned();
    let tester_a = testers
        .next()
        .expect("D7 checked that tester_a exists above");
    let tester_b = testers
        .next()
        .expect("D7 checked that tester_b exists above");

    let inventory_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");
    verify_inventory(&inventory_client, &scenario).await;

    let client_a = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client A");
    let client_b = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client B");
    let contract_a = FaultsCallContract {
        list_method: scenario.faults_call_contract.list_method.clone(),
        clear_method: scenario.faults_call_contract.clear_method.clone(),
        expect_list_status: scenario.faults_call_contract.expect_list_status,
        expect_clear_status: scenario.faults_call_contract.expect_clear_status,
        expect_type: scenario.faults_call_contract.expect_type.clone(),
    };
    let contract_b = FaultsCallContract {
        list_method: scenario.faults_call_contract.list_method.clone(),
        clear_method: scenario.faults_call_contract.clear_method.clone(),
        expect_list_status: scenario.faults_call_contract.expect_list_status,
        expect_clear_status: scenario.faults_call_contract.expect_clear_status,
        expect_type: scenario.faults_call_contract.expect_type.clone(),
    };
    let expected_components_a = scenario.fleet.expected_components.clone();
    let expected_components_b = scenario.fleet.expected_components.clone();
    let precondition_a = Precondition {
        kind: scenario.precondition.kind.clone(),
        owner: scenario.precondition.owner.clone(),
        reason: scenario.precondition.reason.clone(),
    };
    let precondition_b = Precondition {
        kind: scenario.precondition.kind.clone(),
        owner: scenario.precondition.owner.clone(),
        reason: scenario.precondition.reason.clone(),
    };

    let base_url_a = scenario.gate.base_url.clone();
    let base_url_b = scenario.gate.base_url.clone();

    tokio::join!(
        run_tester_sequence(
            client_a,
            base_url_a,
            tester_a,
            contract_a,
            expected_components_a,
            precondition_a,
        ),
        run_tester_sequence(
            client_b,
            base_url_b,
            tester_b,
            contract_b,
            expected_components_b,
            precondition_b,
        ),
    );

    eprintln!(
        "phase5_hil_sovd_06_concurrent_testers: D7 green against {}",
        scenario.gate.tcp_addr
    );
}
