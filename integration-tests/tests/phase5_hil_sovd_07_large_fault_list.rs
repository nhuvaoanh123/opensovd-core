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

//! Phase 5 Line A D8 - HIL scenario 07: large fault list pagination.
//!
//! The test loads `test/hil/scenarios/hil_sovd_07_large_fault_list.yaml`,
//! verifies that `cvc` is visible in the bench inventory, then walks the
//! live `/faults` pagination cursor chain until exhaustion. The harness
//! hard-fails on truncation, overlapping pages, or a missing `total`
//! field once the bench has been seeded with 51+ CVC faults.

mod common;

use std::{collections::BTreeSet, env, fs, net::SocketAddr, path::PathBuf, time::Duration};

use common::override_pi_sovd_gate;
use reqwest::StatusCode;
use serde::Deserialize;
use sovd_interfaces::spec::{
    component::DiscoveredEntities,
    fault::{Fault, ListOfFaults},
};
use tokio::net::TcpStream;

const INVENTORY_TYPE: &str = "sovd_interfaces::spec::component::DiscoveredEntities";
const FAULTS_TYPE: &str = "sovd_interfaces::spec::fault::ListOfFaults";

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    gate: Gate,
    inventory_call: InventoryCall,
    precondition: Precondition,
    target: Target,
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
struct InventoryCall {
    name: String,
    method: String,
    path: String,
    expect_status: u16,
    expect_type: String,
}

#[derive(Debug, Deserialize)]
struct Precondition {
    kind: String,
    owner: String,
    minimum_total: u64,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct Target {
    component_id: String,
    path: String,
    page_size: usize,
    expect_status: u16,
    expect_type: String,
    require_total_field: bool,
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
        .join("hil_sovd_07_large_fault_list.yaml")
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
        "inventory_call.method must stay GET for the D8 harness"
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
    assert!(
        discovered
            .items
            .iter()
            .any(|component| component.id == scenario.target.component_id),
        "bench inventory missing {}; saw {:?}",
        scenario.target.component_id,
        discovered
            .items
            .iter()
            .map(|component| component.id.as_str())
            .collect::<Vec<_>>()
    );
}

async fn get_page(client: &reqwest::Client, scenario: &Scenario, page: u32) -> ListOfFaults {
    let url = format!(
        "{}{}?page={page}&page-size={}",
        scenario.gate.base_url, scenario.target.path, scenario.target.page_size
    );
    let response = client
        .get(&url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {url} failed: {e}"));
    assert_eq!(
        response.status(),
        parse_status(
            scenario.target.expect_status,
            &scenario.target.component_id,
            "expect_status"
        ),
        "{url} should satisfy the scenario contract"
    );
    response
        .json()
        .await
        .unwrap_or_else(|e| panic!("decode ListOfFaults from {url}: {e}"))
}

fn fault_row_key(fault: &Fault) -> String {
    serde_json::to_string(fault).unwrap_or_else(|e| panic!("serialize fault row: {e}"))
}

#[tokio::test]
async fn phase5_hil_sovd_07_large_fault_list() {
    let scenario = match preflight().await {
        Preflight::Skip(reason) => {
            eprintln!("phase5 D8 skip: {reason}");
            return;
        }
        Preflight::Fail(reason) => panic!("phase5 D8 RED: {reason}"),
        Preflight::Run(scenario) => *scenario,
    };

    assert_eq!(
        scenario.precondition.kind, "fault-lib-injection",
        "D8 precondition kind drifted from the intended bench seeding path"
    );
    assert_eq!(
        scenario.target.expect_type, FAULTS_TYPE,
        "target.expect_type must stay {FAULTS_TYPE}"
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");

    verify_inventory(&client, &scenario).await;

    let first_page = get_page(&client, &scenario, 1).await;
    let total = first_page.total.unwrap_or_else(|| {
        panic!(
            "D8 requires a `total` field on the live ListOfFaults response for {}",
            scenario.target.component_id
        )
    });
    assert!(
        !scenario.target.require_total_field || total >= scenario.precondition.minimum_total,
        "phase5 D8 precondition not satisfied for {}: total={total}, minimum_total={} (owner: {}, reason: {})",
        scenario.target.component_id,
        scenario.precondition.minimum_total,
        scenario.precondition.owner,
        scenario.precondition.reason
    );
    assert_eq!(
        first_page.items.len(),
        scenario.target.page_size,
        "first D8 page must be full once total exceeds page_size"
    );
    assert_eq!(
        first_page.next_page,
        Some(2),
        "first D8 page must advertise the next cursor when total > page_size"
    );

    let mut seen = BTreeSet::new();
    let mut counted_rows = 0usize;
    for fault in &first_page.items {
        assert!(
            seen.insert(fault_row_key(fault)),
            "duplicate fault row appeared on D8 page 1: {}",
            fault.code
        );
        counted_rows += 1;
    }

    let mut page = first_page.next_page;
    while let Some(current_page) = page {
        let current = get_page(&client, &scenario, current_page).await;
        assert_eq!(
            current.total,
            Some(total),
            "every D8 page must report the same total count"
        );
        assert!(
            !current.items.is_empty(),
            "D8 page {current_page} must not be empty while the cursor is live"
        );
        assert!(
            current.items.len() <= scenario.target.page_size,
            "D8 page {current_page} exceeded page_size {}",
            scenario.target.page_size
        );
        if current.next_page.is_some() {
            assert_eq!(
                current.items.len(),
                scenario.target.page_size,
                "every non-final D8 page must be full"
            );
        }
        for fault in &current.items {
            assert!(
                seen.insert(fault_row_key(fault)),
                "duplicate fault row appeared on D8 page {current_page}: {}",
                fault.code
            );
            counted_rows += 1;
        }
        page = current.next_page;
    }

    assert_eq!(
        u64::try_from(counted_rows).expect("counted_rows fits into u64"),
        total,
        "pagination chain truncated or repeated rows for {}",
        scenario.target.component_id
    );

    eprintln!(
        "phase5_hil_sovd_07_large_fault_list: D8 green against {} with total={total}",
        scenario.gate.tcp_addr
    );
}
