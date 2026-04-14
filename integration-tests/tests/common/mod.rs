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

//! Shared helpers for bench-gated Phase 2 Line A tests.
//!
//! Two classes of helper live here:
//!
//! 1. Gate + bearer: `bench_opt_in`, `tcp_reachable`, `acquire_bearer`,
//!    `authed_client` — used by any test that talks to a pre-existing
//!    CDA on `127.0.0.1:20002`.
//!
//! 2. Bench lifecycle: `BenchGuard` — a RAII handle that starts the Pi
//!    ecu-sim via `ssh systemctl start ecu-sim`, launches the local CDA
//!    process via `deploy/sil/run-cda-local.sh`, polls both for
//!    readiness, and on `Drop` kills CDA and stops the Pi service.
//!
//! Tests that need the full lifecycle construct a `BenchGuard` and let
//! it fall out of scope at test end. Tests that assume CDA is already
//! running (like the original `phase2_cda_ecusim_smoke.rs`) use only
//! the gate helpers.
//!
//! Everything in this module is `#[allow(dead_code)]` because only a
//! subset is used per test binary, and cargo builds each test file
//! independently — any unused helper would otherwise warn.

#![allow(dead_code)]

use std::{
    env,
    net::SocketAddr,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::Deserialize;
use tokio::net::TcpStream;

/// Bench CDA endpoint — CDA runs locally on Windows pointing at the Pi
/// ecu-sim, so from the test client's perspective CDA is at loopback.
pub const CDA_BASE_URL: &str = "http://127.0.0.1:20002";

/// CDA loopback address in `host:port` form (for TCP readiness probes).
pub const CDA_TCP_ADDR: &str = "127.0.0.1:20002";

/// Pi `DoIP` port — used as a readiness probe for the ecu-sim container.
/// We do NOT speak `DoIP` from the test, only a TCP SYN.
pub const PI_DOIP_ADDR: &str = "192.168.0.197:13400";

/// Pi SSH host used for ecu-sim lifecycle control.
pub const PI_SSH_HOST: &str = "taktflow-pi@192.168.0.197";

/// Env var that opts the worker into running bench-gated tests.
pub const BENCH_ENV: &str = "TAKTFLOW_BENCH";

/// True if the caller opted in to bench-gated tests.
#[must_use]
pub fn bench_opt_in() -> bool {
    env::var(BENCH_ENV).ok().as_deref() == Some("1")
}

/// Try to open a TCP connection to `addr` within `timeout`. Returns `true`
/// on success, `false` otherwise (error or timeout both collapse).
pub async fn tcp_reachable(addr: &str, timeout: Duration) -> bool {
    let Ok(sock): Result<SocketAddr, _> = addr.parse() else {
        return false;
    };
    matches!(
        tokio::time::timeout(timeout, TcpStream::connect(sock)).await,
        Ok(Ok(_))
    )
}

/// Poll `addr` every 500 ms until a TCP connect succeeds or `deadline`
/// elapses. Returns `true` on success.
pub async fn wait_for_tcp(addr: &str, deadline: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if tcp_reachable(addr, Duration::from_millis(750)).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

/// Minimal view of CDA's `/vehicle/v15/authorize` response body.
#[derive(Deserialize)]
struct AuthBody {
    access_token: String,
}

/// Acquire a Bearer token from CDA's upstream default security plugin.
/// CDA without the `auth` feature accepts any `client_id`/`client_secret`
/// pair and returns a JWT.
pub async fn acquire_bearer(client: &Client) -> String {
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

/// Build a reqwest client with the `Authorization: Bearer <token>`
/// header pre-set and a 10-second timeout.
pub fn authed_client(token: &str) -> Client {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).expect("valid auth header"),
    );
    Client::builder()
        .timeout(Duration::from_secs(10))
        .default_headers(headers)
        .build()
        .expect("build authed reqwest client")
}

// ---- lifecycle -----------------------------------------------------------

/// Run `ssh taktflow-pi@... sudo -n systemctl <verb> ecu-sim` and return
/// the command exit status. Uses `-n` (non-interactive) so a missing
/// sudoers rule surfaces immediately instead of hanging.
fn ecu_sim_systemctl(verb: &str) -> std::io::Result<std::process::ExitStatus> {
    Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            PI_SSH_HOST,
            &format!("sudo -n systemctl {verb} ecu-sim"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
}

/// Start the Pi `ecu-sim.service` via ssh, then wait for its `DoIP`
/// port to accept TCP connections. Returns an error if either step
/// fails.
pub fn start_ecu_sim() -> Result<(), String> {
    eprintln!("[bench] ssh start ecu-sim on {PI_SSH_HOST}");
    match ecu_sim_systemctl("start") {
        Ok(s) if s.success() => {}
        Ok(s) => return Err(format!("ssh systemctl start ecu-sim -> {s}")),
        Err(e) => return Err(format!("ssh systemctl start ecu-sim: {e}")),
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("build probe runtime: {e}"))?;
    let ready = rt.block_on(wait_for_tcp(PI_DOIP_ADDR, Duration::from_secs(30)));
    if !ready {
        return Err(format!(
            "ecu-sim started but {PI_DOIP_ADDR} did not accept TCP within 30s"
        ));
    }
    eprintln!("[bench] ecu-sim DoIP port reachable");
    Ok(())
}

/// Stop the Pi `ecu-sim.service` via ssh. Errors are logged but not
/// propagated — cleanup must be best-effort.
pub fn stop_ecu_sim() {
    eprintln!("[bench] ssh stop ecu-sim");
    match ecu_sim_systemctl("stop") {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("[bench] ssh systemctl stop ecu-sim -> {s}"),
        Err(e) => eprintln!("[bench] ssh systemctl stop ecu-sim: {e}"),
    }
}

/// Locate `deploy/sil/run-cda-local.sh` relative to the integration-tests
/// crate manifest. Returns an error if the path does not exist.
fn cda_launch_script() -> Result<PathBuf, String> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let script = PathBuf::from(manifest)
        .parent()
        .ok_or_else(|| format!("no parent for {manifest}"))?
        .join("deploy")
        .join("sil")
        .join("run-cda-local.sh");
    if !script.exists() {
        return Err(format!(
            "cda launch script not found at {}",
            script.display()
        ));
    }
    Ok(script)
}

/// Spawn the local CDA process via `deploy/sil/run-cda-local.sh` under
/// `bash`, then wait for `127.0.0.1:20002` to accept TCP connections.
/// Returns the `Child` handle so the caller can kill it on teardown.
pub fn start_cda() -> Result<Child, String> {
    let script = cda_launch_script()?;
    eprintln!("[bench] launching CDA via {}", script.display());
    // Use `bash` explicitly so this works whether we're invoked from
    // git-bash, MSYS, or cmd.
    let child = Command::new("bash")
        .arg(
            script
                .to_str()
                .ok_or_else(|| format!("non-utf8 path {}", script.display()))?,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn cda launch script: {e}"))?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("build probe runtime: {e}"))?;
    let ready = rt.block_on(wait_for_tcp(CDA_TCP_ADDR, Duration::from_secs(30)));
    if !ready {
        return Err(format!(
            "CDA spawned but {CDA_TCP_ADDR} did not accept TCP within 30s"
        ));
    }
    eprintln!("[bench] CDA loopback port reachable");
    Ok(child)
}

/// Kill a CDA child process. Best-effort — errors are logged.
pub fn stop_cda(mut child: Child) {
    eprintln!("[bench] killing CDA child pid={}", child.id());
    if let Err(e) = child.kill() {
        eprintln!("[bench] kill cda: {e}");
    }
    match child.wait() {
        Ok(s) => eprintln!("[bench] cda exited: {s}"),
        Err(e) => eprintln!("[bench] cda wait: {e}"),
    }
}

/// RAII guard that owns the bench lifecycle: on `Drop` it kills the
/// local CDA child and stops the Pi ecu-sim.
///
/// Construct via [`BenchGuard::launch`]. If construction fails, the
/// error is returned and no resources leak.
pub struct BenchGuard {
    cda: Option<Child>,
}

impl BenchGuard {
    /// Start ecu-sim on the Pi, then launch CDA locally, waiting for
    /// both to be reachable. Returns a guard that cleans them up on
    /// drop.
    pub fn launch() -> Result<Self, String> {
        start_ecu_sim()?;
        match start_cda() {
            Ok(child) => Ok(Self { cda: Some(child) }),
            Err(e) => {
                // CDA failed — undo the ecu-sim start before returning.
                stop_ecu_sim();
                Err(e)
            }
        }
    }
}

impl Drop for BenchGuard {
    fn drop(&mut self) {
        if let Some(child) = self.cda.take() {
            stop_cda(child);
        }
        stop_ecu_sim();
    }
}
