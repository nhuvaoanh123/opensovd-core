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

//! Phase 5 Line A D9 - HIL scenario 08: error handling and degraded mode.
//!
//! The test loads `test/hil/scenarios/hil_sovd_08_error_handling.yaml`,
//! snapshots a fresh baseline fault list for `cvc`, forces the Pi's
//! `can0` interface down over SSH, then asserts:
//!
//! - `GET /sovd/v1/components/cvc/faults` stays HTTP 200
//! - the response flips to `extras.stale == true`
//! - the returned fault rows match the last-known baseline list
//! - bringing `can0` back up clears the stale marker again

mod common;

use std::{
    collections::BTreeSet, env, fs, net::SocketAddr, path::PathBuf, process::Command,
    time::Duration,
};

use common::{override_pi_sovd_gate, override_pi_ssh_host};
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
    pi: PiPlan,
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
    minimum_faults: usize,
    reason: String,
}

#[derive(Clone, Debug, Deserialize)]
struct PiPlan {
    ssh_host: String,
    interface: String,
    nominal_bitrate: u32,
    recovery_restart_ms: u32,
}

#[derive(Debug, Deserialize)]
struct Target {
    component_id: String,
    path: String,
    expect_status: u16,
    expect_type: String,
    degraded_deadline_ms: u64,
    recovery_deadline_ms: u64,
    require_stale_during_disconnect: bool,
    require_fresh_after_recovery: bool,
}

enum Preflight {
    Skip(String),
    Run(Box<Scenario>),
    Fail(String),
}

struct PiLinkGuard {
    plan: PiPlan,
}

impl PiLinkGuard {
    fn new(plan: &PiPlan) -> Self {
        Self { plan: plan.clone() }
    }

    fn set_nominal_up(&self) -> Result<String, String> {
        run_ssh_sudo_script(
            &self.plan.ssh_host,
            &format!(
                "set -euo pipefail; \
                 ip link set {iface} down 2>/dev/null || true; \
                 ip link set {iface} type can bitrate {bitrate} restart-ms {restart_ms}; \
                 ip link set {iface} up; \
                 ip -details -statistics link show {iface}",
                iface = self.plan.interface,
                bitrate = self.plan.nominal_bitrate,
                restart_ms = self.plan.recovery_restart_ms
            ),
        )
    }

    fn disconnect(&self) -> Result<String, String> {
        run_ssh_sudo_script(
            &self.plan.ssh_host,
            &format!(
                "set -euo pipefail; \
                 ip link set {iface} down; \
                 ip -details -statistics link show {iface}",
                iface = self.plan.interface
            ),
        )
    }

    fn recover(&self) -> Result<String, String> {
        self.set_nominal_up()
    }
}

impl Drop for PiLinkGuard {
    fn drop(&mut self) {
        if let Err(e) = self.recover() {
            eprintln!(
                "phase5 D9 cleanup warning: failed to restore {} on {}: {e}",
                self.plan.interface, self.plan.ssh_host
            );
        }
    }
}

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("integration-tests has workspace parent")
        .join("test")
        .join("hil")
        .join("scenarios")
        .join("hil_sovd_08_error_handling.yaml")
}

fn load_scenario() -> Scenario {
    let path = scenario_path();
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut scenario: Scenario =
        serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    override_pi_sovd_gate(&mut scenario.gate.tcp_addr, &mut scenario.gate.base_url);
    override_pi_ssh_host(&mut scenario.pi.ssh_host);
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

fn run_ssh_sudo_script(host: &str, script: &str) -> Result<String, String> {
    let remote = format!("sudo -n bash -lc '{script}'");
    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            host,
            &remote,
        ])
        .output()
        .map_err(|e| format!("spawn ssh to {host}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let merged = if stderr.trim().is_empty() {
        stdout
    } else if stdout.trim().is_empty() {
        stderr
    } else {
        format!("{stdout}\n{stderr}")
    };
    if output.status.success() {
        Ok(merged)
    } else {
        Err(format!(
            "ssh {host} -> {}; output:\n{merged}",
            output.status
        ))
    }
}

async fn verify_inventory(client: &reqwest::Client, scenario: &Scenario) {
    assert_eq!(
        scenario.inventory_call.method, "GET",
        "inventory_call.method must stay GET for the D9 harness"
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

async fn get_faults(client: &reqwest::Client, scenario: &Scenario) -> ListOfFaults {
    assert_eq!(
        scenario.target.expect_type, FAULTS_TYPE,
        "target.expect_type must stay {FAULTS_TYPE}"
    );
    let url = format!("{}{}", scenario.gate.base_url, scenario.target.path);
    let response = client
        .get(&url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {url} failed: {e}"));
    assert_eq!(
        response.status(),
        parse_status(
            scenario.target.expect_status,
            &scenario.target.path,
            "expect_status"
        ),
        "{url} should satisfy the scenario contract"
    );
    response
        .json()
        .await
        .unwrap_or_else(|e| panic!("{url} body decode failed: {e}"))
}

fn is_stale(list: &ListOfFaults) -> bool {
    list.extras.as_ref().is_some_and(|extras| extras.stale)
}

fn fault_row_key(fault: &Fault) -> String {
    serde_json::to_string(fault).unwrap_or_else(|e| panic!("serialize fault row: {e}"))
}

fn fault_rows(list: &ListOfFaults) -> BTreeSet<String> {
    list.items.iter().map(fault_row_key).collect()
}

async fn poll_for_degraded(
    client: &reqwest::Client,
    scenario: &Scenario,
    baseline_rows: &BTreeSet<String>,
    baseline_total: Option<u64>,
) -> ListOfFaults {
    let deadline = Duration::from_millis(scenario.target.degraded_deadline_ms);
    let start = tokio::time::Instant::now();
    while start.elapsed() < deadline {
        let faults = get_faults(client, scenario).await;
        if (!scenario.target.require_stale_during_disconnect || is_stale(&faults))
            && fault_rows(&faults) == *baseline_rows
            && faults.total == baseline_total
        {
            return faults;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let last = get_faults(client, scenario).await;
    panic!(
        "D9 degraded read did not settle within {} ms; stale={}, total={:?}, rows={:?}",
        scenario.target.degraded_deadline_ms,
        is_stale(&last),
        last.total,
        last.items
            .iter()
            .map(|fault| (&fault.code, &fault.fault_name))
            .collect::<Vec<_>>()
    );
}

async fn poll_for_recovery(client: &reqwest::Client, scenario: &Scenario) -> ListOfFaults {
    let deadline = Duration::from_millis(scenario.target.recovery_deadline_ms);
    let start = tokio::time::Instant::now();
    while start.elapsed() < deadline {
        let faults = get_faults(client, scenario).await;
        if !scenario.target.require_fresh_after_recovery || !is_stale(&faults) {
            return faults;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let last = get_faults(client, scenario).await;
    panic!(
        "D9 recovery did not clear stale within {} ms; rows={:?}",
        scenario.target.recovery_deadline_ms,
        last.items
            .iter()
            .map(|fault| (&fault.code, &fault.fault_name))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn phase5_hil_sovd_08_error_handling() {
    let scenario = match preflight().await {
        Preflight::Skip(reason) => {
            eprintln!("phase5 D9 skip: {reason}");
            return;
        }
        Preflight::Fail(reason) => panic!("phase5 D9 RED: {reason}"),
        Preflight::Run(scenario) => *scenario,
    };

    assert_eq!(
        scenario.precondition.kind, "readable-fault-baseline",
        "D9 precondition kind drifted from the intended baseline setup"
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");

    verify_inventory(&client, &scenario).await;

    let link_guard = PiLinkGuard::new(&scenario.pi);
    let nominal = link_guard
        .set_nominal_up()
        .unwrap_or_else(|e| panic!("set nominal can0 before D9: {e}"));
    eprintln!("phase5 D9 nominal can0:\n{nominal}");

    let baseline = get_faults(&client, &scenario).await;
    assert!(
        !is_stale(&baseline),
        "D9 baseline must be fresh before disconnect"
    );
    assert!(
        baseline.items.len() >= scenario.precondition.minimum_faults,
        "phase5 D9 precondition not satisfied for {}: saw {} faults, need at least {} (owner: {}, reason: {})",
        scenario.target.component_id,
        baseline.items.len(),
        scenario.precondition.minimum_faults,
        scenario.precondition.owner,
        scenario.precondition.reason
    );
    let baseline_rows = fault_rows(&baseline);

    let disconnected = link_guard
        .disconnect()
        .unwrap_or_else(|e| panic!("disconnect can0 for D9: {e}"));
    eprintln!("phase5 D9 disconnected can0:\n{disconnected}");

    let degraded = poll_for_degraded(&client, &scenario, &baseline_rows, baseline.total).await;
    if scenario.target.require_stale_during_disconnect {
        assert!(
            is_stale(&degraded),
            "D9 degraded path must advertise stale=true while can0 is down"
        );
    }
    assert_eq!(
        fault_rows(&degraded),
        baseline_rows,
        "D9 degraded path must preserve the last-known fault list"
    );
    assert_eq!(
        degraded.total, baseline.total,
        "D9 degraded path must preserve pagination metadata for the cached list"
    );

    let recovered_link = link_guard
        .recover()
        .unwrap_or_else(|e| panic!("restore can0 after D9: {e}"));
    eprintln!("phase5 D9 restored can0:\n{recovered_link}");

    let recovered = poll_for_recovery(&client, &scenario).await;
    if scenario.target.require_fresh_after_recovery {
        assert!(
            !is_stale(&recovered),
            "D9 recovery should clear stale=true once can0 is back"
        );
    }

    eprintln!(
        "phase5_hil_sovd_08_error_handling: D9 green against {} for {}",
        scenario.gate.tcp_addr, scenario.target.component_id
    );
}
