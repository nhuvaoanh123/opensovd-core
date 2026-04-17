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

//! Phase 5 Line A D4 - HIL scenario 03: operation execution.
//!
//! This test is the code-side companion to
//! `test/hil/scenarios/hil_sovd_03_operation_execution.yaml`.
//!
//! The live bench gate stays disabled until BOTH:
//!
//! - `TAKTFLOW_BENCH=1`
//! - `PHASE5_BENCH_READY=1`
//!
//! Once both env vars are set, the test becomes a hard red/green gate:
//! the `rzc` component must be present, `POST .../executions` must
//! return a `StartExecutionAsyncResponse`, and the follow-up poll loop
//! must reach either `completed` with parameters or `failed` with an
//! error envelope within the configured timeout budget.

mod common;

use std::{env, fs, net::SocketAddr, path::PathBuf, time::Duration};

use common::override_pi_sovd_gate;
use reqwest::StatusCode;
use serde::Deserialize;
use sovd_interfaces::spec::{
    component::DiscoveredEntities,
    operation::{
        Capability, ExecutionStatus, ExecutionStatusResponse, StartExecutionAsyncResponse,
        StartExecutionRequest,
    },
};
use tokio::net::TcpStream;

const INVENTORY_TYPE: &str = "sovd_interfaces::spec::component::DiscoveredEntities";
const START_TYPE: &str = "sovd_interfaces::spec::operation::StartExecutionAsyncResponse";
const STATUS_TYPE: &str = "sovd_interfaces::spec::operation::ExecutionStatusResponse";

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    gate: Gate,
    inventory_call: InventoryCall,
    target: Target,
    start_call: StartCall,
    status_call: StatusCall,
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
struct Target {
    component_id: String,
    operation_id: String,
}

#[derive(Debug, Deserialize)]
struct StartCall {
    method: String,
    path: String,
    body: StartExecutionRequest,
    expect_status: u16,
    expect_type: String,
    expected_initial_status: ExecutionStatus,
}

#[derive(Debug, Deserialize)]
struct StatusCall {
    method: String,
    path_template: String,
    expect_status: u16,
    expect_type: String,
    accepted_terminal_statuses: Vec<ExecutionStatus>,
    poll_interval_ms: u64,
    max_attempts: u32,
    require_parameters_on_completed: bool,
    require_error_on_failed: bool,
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
        .join("hil_sovd_03_operation_execution.yaml")
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
        "inventory_call.method must stay GET for the D4 harness"
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
            .any(|item| item.id == scenario.target.component_id),
        "bench inventory missing target component {}; saw {:?}",
        scenario.target.component_id,
        discovered
            .items
            .iter()
            .map(|item| &item.id)
            .collect::<Vec<_>>()
    );
}

async fn start_execution(
    client: &reqwest::Client,
    scenario: &Scenario,
) -> StartExecutionAsyncResponse {
    assert_eq!(
        scenario.start_call.method, "POST",
        "start_call.method must stay POST for the D4 harness"
    );
    assert_eq!(
        scenario.start_call.expect_type, START_TYPE,
        "start_call.expect_type must stay {START_TYPE}"
    );
    let start_url = format!("{}{}", scenario.gate.base_url, scenario.start_call.path);
    let response = client
        .post(&start_url)
        .json(&scenario.start_call.body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("POST {start_url} failed: {e}"));
    assert_eq!(
        response.status(),
        parse_status(
            scenario.start_call.expect_status,
            &scenario.start_call.path,
            "expect_status"
        ),
        "{start_url} should satisfy the scenario contract"
    );
    let started: StartExecutionAsyncResponse = response
        .json()
        .await
        .unwrap_or_else(|e| panic!("{start_url} body decode failed: {e}"));
    assert!(!started.id.is_empty(), "execution id must not be empty");
    assert_eq!(
        started.status,
        Some(scenario.start_call.expected_initial_status),
        "unexpected initial execution status"
    );
    started
}

async fn poll_terminal_status(
    client: &reqwest::Client,
    scenario: &Scenario,
    execution_id: &str,
) -> ExecutionStatusResponse {
    assert_eq!(
        scenario.status_call.method, "GET",
        "status_call.method must stay GET for the D4 harness"
    );
    assert_eq!(
        scenario.status_call.expect_type, STATUS_TYPE,
        "status_call.expect_type must stay {STATUS_TYPE}"
    );
    let status_url = format!(
        "{}{}",
        scenario.gate.base_url,
        scenario
            .status_call
            .path_template
            .replace("{execution_id}", execution_id)
    );

    for _ in 0..scenario.status_call.max_attempts {
        let response = client
            .get(&status_url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {status_url} failed: {e}"));
        assert_eq!(
            response.status(),
            parse_status(
                scenario.status_call.expect_status,
                &scenario.status_call.path_template,
                "expect_status"
            ),
            "{status_url} should satisfy the scenario contract"
        );
        let status: ExecutionStatusResponse = response
            .json()
            .await
            .unwrap_or_else(|e| panic!("{status_url} body decode failed: {e}"));
        if scenario
            .status_call
            .accepted_terminal_statuses
            .iter()
            .any(|accepted| status.status == Some(*accepted))
        {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(scenario.status_call.poll_interval_ms)).await;
    }

    panic!(
        "execution {execution_id} for {}/{} did not reach a terminal status within {} attempts",
        scenario.target.component_id,
        scenario.target.operation_id,
        scenario.status_call.max_attempts
    );
}

fn assert_terminal_payload(status: &ExecutionStatusResponse, scenario: &Scenario) {
    assert_eq!(
        status.capability,
        Capability::Execute,
        "terminal execution capability should remain execute"
    );
    match status.status {
        Some(ExecutionStatus::Completed) => {
            if scenario.status_call.require_parameters_on_completed {
                assert!(
                    status.parameters.is_some(),
                    "completed execution should carry a diagnostic result payload"
                );
            }
        }
        Some(ExecutionStatus::Failed) => {
            if scenario.status_call.require_error_on_failed {
                assert!(
                    status
                        .error
                        .as_ref()
                        .is_some_and(|errors| !errors.is_empty()),
                    "failed execution should carry an error envelope"
                );
            }
        }
        other => panic!("unexpected terminal status {other:?}"),
    }
}

#[tokio::test]
async fn phase5_hil_sovd_03_operation_execution() {
    let scenario = match preflight().await {
        Preflight::Skip(reason) => {
            eprintln!("phase5 D4 skip: {reason}");
            return;
        }
        Preflight::Fail(reason) => panic!("phase5 D4 RED: {reason}"),
        Preflight::Run(scenario) => *scenario,
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");

    verify_inventory(&client, &scenario).await;
    let started = start_execution(&client, &scenario).await;
    let status = poll_terminal_status(&client, &scenario, &started.id).await;
    assert_terminal_payload(&status, &scenario);

    eprintln!(
        "phase5_hil_sovd_03_operation_execution: D4 green against {} for {}/{}",
        scenario.gate.tcp_addr, scenario.target.component_id, scenario.target.operation_id
    );
}
