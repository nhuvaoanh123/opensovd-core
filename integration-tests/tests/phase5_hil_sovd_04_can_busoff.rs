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

//! Phase 5 Line A D5 - HIL scenario 04: real CAN bus-off on Pi `can0`.
//!
//! The test loads `test/hil/scenarios/hil_sovd_04_can_busoff.yaml`,
//! verifies the bench inventory, drives the Pi's `can0` interface into a
//! real BUS-OFF state over SSH, then asserts:
//!
//! - `GET /sovd/v1/components/cvc/faults` stays readable
//! - the degraded response advertises `extras.stale == true`
//! - a bus-off fault marker appears within the configured deadline
//! - the stale flag clears after the Pi CAN interface is restored

mod common;

use std::{env, fs, net::SocketAddr, path::PathBuf, process::Command, time::Duration};

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

#[derive(Clone, Debug, Deserialize)]
struct PiPlan {
    ssh_host: String,
    interface: String,
    nominal_bitrate: u32,
    bus_off_bitrate: u32,
    recovery_restart_ms: u32,
    tester_frame: String,
    burst_frames: u32,
    max_bursts: u32,
}

#[derive(Debug, Deserialize)]
struct Target {
    component_id: String,
    path: String,
    expect_status: u16,
    expect_type: String,
    bus_off_fault_deadline_ms: u64,
    recovery_deadline_ms: u64,
    require_stale_during_bus_off: bool,
    require_fresh_after_recovery: bool,
    expected_fault_markers: Vec<String>,
}

enum Preflight {
    Skip(String),
    Run(Box<Scenario>),
    Fail(String),
}

struct PiCanGuard {
    plan: PiPlan,
}

impl PiCanGuard {
    fn new(plan: &PiPlan) -> Self {
        Self { plan: plan.clone() }
    }

    fn set_nominal_no_restart(&self) -> Result<String, String> {
        run_ssh_sudo_script(
            &self.plan.ssh_host,
            &format!(
                "set -euo pipefail; \
                 ip link set {iface} down 2>/dev/null || true; \
                 ip link set {iface} type can bitrate {bitrate} restart-ms 0; \
                 ip link set {iface} up; \
                 ip -details -statistics link show {iface}",
                iface = self.plan.interface,
                bitrate = self.plan.nominal_bitrate
            ),
        )
    }

    fn trigger_bus_off(&self) -> Result<String, String> {
        run_ssh_sudo_script(
            &self.plan.ssh_host,
            &format!(
                "set -euo pipefail; \
                 ip link set {iface} down 2>/dev/null || true; \
                 ip link set {iface} type can bitrate {bitrate} restart-ms 0; \
                 ip link set {iface} up; \
                 for burst in $(seq 1 {max_bursts}); do \
                   for i in $(seq 1 {burst_frames}); do \
                     cansend {iface} {frame} >/dev/null 2>&1 || true; \
                   done; \
                   if ip -details -statistics link show {iface} | grep -Ei \"BUS-OFF|bus-off\"; then \
                     ip -details -statistics link show {iface}; \
                     exit 0; \
                   fi; \
                 done; \
                 ip -details -statistics link show {iface}; \
                 exit 1",
                iface = self.plan.interface,
                bitrate = self.plan.bus_off_bitrate,
                frame = self.plan.tester_frame,
                burst_frames = self.plan.burst_frames,
                max_bursts = self.plan.max_bursts,
            ),
        )
    }

    fn recover(&self) -> Result<String, String> {
        run_ssh_sudo_script(
            &self.plan.ssh_host,
            &format!(
                "set -euo pipefail; \
                 ip link set {iface} down 2>/dev/null || true; \
                 if ! ip link set {iface} type can bitrate {bitrate} restart-ms {restart_ms} 2>/dev/null; then \
                   ip link set {iface} type can bitrate {bitrate}; \
                 fi; \
                 ip link set {iface} up; \
                 ip -details -statistics link show {iface}",
                iface = self.plan.interface,
                bitrate = self.plan.nominal_bitrate,
                restart_ms = self.plan.recovery_restart_ms
            ),
        )
    }
}

impl Drop for PiCanGuard {
    fn drop(&mut self) {
        if let Err(e) = self.recover() {
            eprintln!(
                "phase5 D5 cleanup warning: failed to restore {} on {}: {e}",
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
        .join("hil_sovd_04_can_busoff.yaml")
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
        "inventory_call.method must stay GET for the D5 harness"
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

fn fault_matches_markers(fault: &Fault, markers: &[String]) -> bool {
    let haystacks = [
        fault.code.as_str(),
        fault.display_code.as_deref().unwrap_or_default(),
        fault.fault_name.as_str(),
    ];
    haystacks.iter().any(|value| {
        let value = value.to_ascii_lowercase();
        markers
            .iter()
            .any(|marker| value.contains(&marker.to_ascii_lowercase()))
    })
}

fn marker_fault_count(list: &ListOfFaults, markers: &[String]) -> usize {
    list.items
        .iter()
        .filter(|fault| fault_matches_markers(fault, markers))
        .count()
}

fn describe_faults(list: &ListOfFaults) -> Vec<String> {
    list.items
        .iter()
        .map(|fault| {
            format!(
                "{}:{}",
                fault.display_code.as_deref().unwrap_or(&fault.code),
                fault.fault_name
            )
        })
        .collect()
}

fn is_stale(list: &ListOfFaults) -> bool {
    list.extras.as_ref().is_some_and(|extras| extras.stale)
}

async fn poll_for_bus_off_fault(client: &reqwest::Client, scenario: &Scenario) -> ListOfFaults {
    let deadline = Duration::from_millis(scenario.target.bus_off_fault_deadline_ms);
    let start = tokio::time::Instant::now();
    while start.elapsed() < deadline {
        let faults = get_faults(client, scenario).await;
        let marker_count = marker_fault_count(&faults, &scenario.target.expected_fault_markers);
        if marker_count > 0 && (!scenario.target.require_stale_during_bus_off || is_stale(&faults))
        {
            return faults;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let last = get_faults(client, scenario).await;
    panic!(
        "bus-off fault did not appear within {} ms; stale={}, faults={:?}",
        scenario.target.bus_off_fault_deadline_ms,
        is_stale(&last),
        describe_faults(&last)
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
        "bus-off recovery did not clear stale within {} ms; faults={:?}",
        scenario.target.recovery_deadline_ms,
        describe_faults(&last)
    );
}

#[tokio::test]
async fn phase5_hil_sovd_04_can_busoff() {
    let scenario = match preflight().await {
        Preflight::Skip(reason) => {
            eprintln!("phase5 D5 skip: {reason}");
            return;
        }
        Preflight::Fail(reason) => panic!("phase5 D5 RED: {reason}"),
        Preflight::Run(scenario) => *scenario,
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");

    verify_inventory(&client, &scenario).await;

    let can_guard = PiCanGuard::new(&scenario.pi);
    let nominal = can_guard
        .set_nominal_no_restart()
        .unwrap_or_else(|e| panic!("set nominal can0 before D5: {e}"));
    eprintln!("phase5 D5 nominal can0:\n{nominal}");

    let baseline = get_faults(&client, &scenario).await;
    assert!(
        !is_stale(&baseline),
        "D5 baseline must be fresh before injection; saw stale faults {:?}",
        describe_faults(&baseline)
    );
    assert_eq!(
        marker_fault_count(&baseline, &scenario.target.expected_fault_markers),
        0,
        "D5 baseline already contains bus-off markers; reset the bench first: {:?}",
        describe_faults(&baseline)
    );

    let injected = can_guard
        .trigger_bus_off()
        .unwrap_or_else(|e| panic!("drive can0 into BUS-OFF: {e}"));
    assert!(
        injected.contains("BUS-OFF") || injected.to_ascii_lowercase().contains("bus-off"),
        "D5 injection command completed without BUS-OFF evidence:\n{injected}"
    );
    eprintln!("phase5 D5 injected can0 BUS-OFF:\n{injected}");

    let during_bus_off = poll_for_bus_off_fault(&client, &scenario).await;
    assert!(
        marker_fault_count(&during_bus_off, &scenario.target.expected_fault_markers) > 0,
        "bus-off faults missing after injection: {:?}",
        describe_faults(&during_bus_off)
    );
    if scenario.target.require_stale_during_bus_off {
        assert!(
            is_stale(&during_bus_off),
            "D5 degraded path must advertise stale=true during BUS-OFF"
        );
    }

    let recovered_link = can_guard
        .recover()
        .unwrap_or_else(|e| panic!("restore can0 after BUS-OFF: {e}"));
    eprintln!("phase5 D5 recovered can0:\n{recovered_link}");

    let recovered_faults = poll_for_recovery(&client, &scenario).await;
    if scenario.target.require_fresh_after_recovery {
        assert!(
            !is_stale(&recovered_faults),
            "D5 recovery should clear stale=true once can0 is back"
        );
    }

    eprintln!(
        "phase5_hil_sovd_04_can_busoff: D5 green against {} via {} on {}",
        scenario.gate.tcp_addr, scenario.pi.interface, scenario.pi.ssh_host
    );
}
