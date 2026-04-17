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

//! Phase 5 Line A D6 - HIL scenario 05: components metadata.
//!
//! This test is the code-side companion to
//! `test/hil/scenarios/hil_sovd_05_components_metadata.yaml`.
//!
//! The current `/sovd/v1/components` wire type is
//! `DiscoveredEntities`, so D6 verifies the spec-defined discovery
//! metadata the endpoint actually exposes today: all four bench
//! components must be present with stable ids, names, and component hrefs.

mod common;

use std::{env, fs, net::SocketAddr, path::PathBuf, time::Duration};

use common::override_pi_sovd_gate;
use reqwest::StatusCode;
use serde::Deserialize;
use sovd_interfaces::spec::component::DiscoveredEntities;
use tokio::net::TcpStream;

const INVENTORY_TYPE: &str = "sovd_interfaces::spec::component::DiscoveredEntities";

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    gate: Gate,
    inventory_call: InventoryCall,
    expected_components: Vec<ExpectedComponent>,
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
    expect_exact_count: usize,
}

#[derive(Debug, Deserialize)]
struct ExpectedComponent {
    id: String,
    name: String,
    href_suffix: String,
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
        .join("hil_sovd_05_components_metadata.yaml")
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

#[tokio::test]
async fn phase5_hil_sovd_05_components_metadata() {
    let scenario = match preflight().await {
        Preflight::Skip(reason) => {
            eprintln!("phase5 D6 skip: {reason}");
            return;
        }
        Preflight::Fail(reason) => panic!("phase5 D6 RED: {reason}"),
        Preflight::Run(scenario) => *scenario,
    };

    assert_eq!(
        scenario.inventory_call.method, "GET",
        "inventory_call.method must stay GET for the D6 harness"
    );
    assert_eq!(
        scenario.inventory_call.expect_type, INVENTORY_TYPE,
        "inventory_call.expect_type must stay {INVENTORY_TYPE}"
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");

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
    assert_eq!(
        discovered.items.len(),
        scenario.inventory_call.expect_exact_count,
        "unexpected bench component count; saw {:?}",
        discovered
            .items
            .iter()
            .map(|item| (&item.id, &item.href))
            .collect::<Vec<_>>()
    );
    assert!(
        discovered.extras.is_none(),
        "D6 should read nominal discovery metadata, not a degraded extras payload"
    );

    for expected in &scenario.expected_components {
        let found = discovered
            .items
            .iter()
            .find(|item| item.id == expected.id)
            .unwrap_or_else(|| {
                panic!(
                    "bench inventory missing expected component {}; saw {:?}",
                    expected.id,
                    discovered
                        .items
                        .iter()
                        .map(|item| &item.id)
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            found.name, expected.name,
            "unexpected name for {}",
            expected.id
        );
        assert!(
            found.href.ends_with(&expected.href_suffix),
            "unexpected href for {}: {}",
            expected.id,
            found.href
        );
    }

    eprintln!(
        "phase5_hil_sovd_05_components_metadata: D6 green against {} for {:?}",
        scenario.gate.tcp_addr,
        scenario
            .expected_components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<Vec<_>>()
    );
}
