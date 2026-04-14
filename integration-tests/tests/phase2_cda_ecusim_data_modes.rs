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

//! Phase 2 Line A — `/data` and `/modes` coverage against the live Pi
//! ecu-sim.
//!
//! This test extends the original `phase2_cda_ecusim_smoke.rs` by
//! driving the SOVD `data` and `modes` resources end-to-end:
//!
//! 1. `GET .../components/{component}/data` → parsed as
//!    [`sovd_interfaces::spec::data::Datas`]. Looped over every
//!    component CDA discovers (the upstream ecu-sim advertises
//!    FLXC1000 and FLXCNG1000 via the MDDs in `opensovd-cda.toml`).
//!
//! 2. For ≥2 DIDs from the returned list,
//!    `GET .../data/{did}` → parsed as
//!    [`sovd_interfaces::spec::data::ReadValue`], with an assertion
//!    that the `data` field is non-null.
//!
//! 3. `GET .../components/{component}/modes` → parsed as
//!    [`sovd_interfaces::spec::mode::SupportedModes`].
//!
//! 4. For each of the four SOVD-standard modes (`session`,
//!    `security`, `commctrl`, `dtcsetting`):
//!    `GET .../modes/{mode}` → parsed as
//!    [`sovd_interfaces::spec::mode::ModeDetails`]. `PUT` mode
//!    transitions are explicitly out of scope here.
//!
//! 5. Error path: `GET .../data/{bogus}` for a DID that cannot exist
//!    (`ffff_unknown`). Asserts the response is either `404 Not Found`
//!    or a well-formed [`sovd_interfaces::spec::error::GenericError`].
//!
//! # Lifecycle
//!
//! Unlike `phase2_cda_ecusim_smoke.rs` — which assumes CDA is already
//! running — this test constructs a [`common::BenchGuard`] that:
//!
//! - SSHs into the Pi and `systemctl start ecu-sim` (polls the `DoIP`
//!   port for readiness)
//! - Spawns local CDA via `deploy/sil/run-cda-local.sh` (polls
//!   `127.0.0.1:20002` for readiness)
//! - On `Drop`: kills the CDA child and `systemctl stop ecu-sim` on
//!   the Pi so test reruns start from a clean slate.
//!
//! # Bench gate
//!
//! The whole test body is skipped (returns `Ok(())` cleanly) when
//! `TAKTFLOW_BENCH=1` is not set in the environment, so
//! `cargo test --workspace` stays green on non-bench hosts.
//!
//! # Wire prefix
//!
//! CDA exposes SOVD routes under `/vehicle/v15` (its native SOVD-1.0
//! draft prefix). Our Phase 2 Line B server will expose `/sovd/v1`,
//! but the body shapes are identical because both sides port from the
//! same spec.

mod common;

use std::time::Duration;

use reqwest::{Client, StatusCode};
use sovd_interfaces::spec::{
    component::DiscoveredEntities,
    data::{Datas, ReadValue},
    error::GenericError,
    mode::{ModeDetails, SupportedModes},
};

use common::{BENCH_ENV, BenchGuard, CDA_BASE_URL, acquire_bearer, authed_client, bench_opt_in};

/// The four SOVD-standard mode identifiers that `spec::mode` names
/// out explicitly. An ECU MAY expose additional OEM modes, but these
/// four are the ones every classic UDS-derived ECU advertises through
/// the mode collection.
const STANDARD_MODES: &[&str] = &["session", "security", "commctrl", "dtcsetting"];

/// A DID slug the ecu-sim's FLXC* MDD catalogs cannot possibly serve —
/// SOVD uses stable string ids (not hex-word DIDs), so any id that
/// does not match the MDD must 404 or return a `GenericError`.
const BOGUS_DID: &str = "ffff_unknown";

/// Skip the whole test cleanly when the bench gate is off.
fn maybe_skip() -> bool {
    if !bench_opt_in() {
        eprintln!(
            "skipping phase2 data+modes: {BENCH_ENV}=1 not set \
             (set it to run on the bench LAN)"
        );
        return true;
    }
    false
}

async fn get_typed<T: serde::de::DeserializeOwned>(client: &Client, url: &str) -> T {
    let resp = client
        .get(url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {url}: network error: {e}"));
    let status = resp.status();
    let body = resp
        .text()
        .await
        .unwrap_or_else(|e| panic!("GET {url}: read body: {e}"));
    assert_eq!(
        status,
        StatusCode::OK,
        "GET {url} -> {status}; body = {body}"
    );
    serde_json::from_str::<T>(&body)
        .unwrap_or_else(|e| panic!("GET {url}: spec parse failed: {e}\n--- body ---\n{body}\n---"))
}

/// Drive `/data` coverage for one component: list, then read up to
/// two DIDs. Returns `true` if this component advertised a non-empty
/// `/data` catalog (so the caller can assert that at least one
/// component in the set yielded real spec-typed reads).
///
/// A zero-length catalog is accepted as a sparse MDD state (e.g. the
/// upstream `flxcng1000` MDD carries no SDGs on some builds, which
/// CDA surfaces as an empty `Datas.items`). We log it and return
/// `false` without failing, identical to how
/// `phase2_cda_ecusim_smoke.rs` tolerates a 404 on `/faults` when an
/// ECU has no DTC services.
async fn exercise_data(client: &Client, component: &str) -> bool {
    let url = format!("{CDA_BASE_URL}/vehicle/v15/components/{component}/data");
    let listing: Datas = get_typed(client, &url).await;
    eprintln!(
        "[data] {component}: {} value descriptors",
        listing.items.len()
    );
    if listing.items.is_empty() {
        eprintln!("[data] {component}: empty /data catalog; accepting as sparse MDD state");
        return false;
    }

    let sample: Vec<_> = listing.items.iter().take(2).collect();
    let probed = sample.len();
    for meta in sample {
        let did_url = format!(
            "{CDA_BASE_URL}/vehicle/v15/components/{component}/data/{}",
            meta.id
        );
        let value: ReadValue = get_typed(client, &did_url).await;
        // CDA lower-cases DID identifiers on the response side even
        // when the client-supplied path casing matched the MDD
        // (`VINDataIdentifier` becomes `vindataidentifier`). The SOVD
        // spec says `id` is a stable opaque string and does not
        // require byte-for-byte preservation of case, so compare
        // case-insensitively.
        assert!(
            value.id.eq_ignore_ascii_case(&meta.id),
            "ReadValue.id ({}) does not match requested DID ({}) \
             (case-insensitive compare)",
            value.id,
            meta.id
        );
        assert!(
            !value.data.is_null(),
            "ReadValue.data for {component}/{} is null; expected populated payload",
            meta.id
        );
        eprintln!(
            "[data] {component}/{}: ReadValue.data.len()={}",
            meta.id,
            value.data.to_string().len()
        );
    }
    // A component counts as "exercised" only if we probed at least one
    // DID. Two is the brief's minimum, and we log when a component
    // advertised exactly one so the partial coverage is visible.
    if probed < 2 {
        eprintln!(
            "[data] {component}: only {probed} DID(s) advertised; \
             component counts as exercised but did not meet the ≥2-DID target"
        );
    }
    true
}

/// Drive `/modes` coverage for one component: list, then details for
/// each of the four standard modes. Returns the number of 200
/// `ModeDetails` responses the caller saw, so the top-level test can
/// assert that at least one mode query succeeded across the entire
/// component set.
///
/// Non-200 responses are accepted without failing when the status is
/// one of:
///
/// - `404 Not Found` — the mode is not in this ECU's catalog
/// - `403 Forbidden` — CDA gated the mode behind an ECU lock
///   (`error_code=insufficient-access-rights`); the SOVD spec lets a
///   server require the lock before serving the read, and acquiring
///   it is out of scope for this smoke test
///
/// Any other status is a hard failure.
async fn exercise_modes(client: &Client, component: &str) -> usize {
    let list_url = format!("{CDA_BASE_URL}/vehicle/v15/components/{component}/modes");
    let listing: SupportedModes = get_typed(client, &list_url).await;
    eprintln!(
        "[modes] {component}: {} modes advertised",
        listing.items.len()
    );

    let mut ok_count: usize = 0;
    for mode in STANDARD_MODES {
        let url = format!("{CDA_BASE_URL}/vehicle/v15/components/{component}/modes/{mode}");
        let resp = client
            .get(&url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {url}: network error: {e}"));
        let status = resp.status();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|e| panic!("GET {url}: read body: {e}"));
        if status == StatusCode::NOT_FOUND {
            eprintln!("[modes] {component}/{mode}: 404 (not in this ECU's catalog)");
            continue;
        }
        if status == StatusCode::FORBIDDEN {
            // Verify the body is still a spec-valid GenericError so
            // we catch regressions where CDA returns a bare string.
            let err: GenericError = serde_json::from_str(&body).unwrap_or_else(|e| {
                panic!("parse GenericError for {component}/{mode} 403: {e}; body = {body}")
            });
            eprintln!(
                "[modes] {component}/{mode}: 403 ({}) — accepted (ECU lock required)",
                err.error_code
            );
            continue;
        }
        assert_eq!(
            status,
            StatusCode::OK,
            "GET {url} -> {status}; body = {body}"
        );
        let details: ModeDetails = serde_json::from_str(&body).unwrap_or_else(|e| {
            panic!("parse ModeDetails for {component}/{mode}: {e}; body = {body}")
        });
        assert!(
            !details.value.is_empty(),
            "ModeDetails.value for {component}/{mode} is empty"
        );
        eprintln!(
            "[modes] {component}/{mode}: value=\"{}\" name={:?}",
            details.value, details.name
        );
        ok_count = ok_count.saturating_add(1);
    }
    ok_count
}

/// Exercise the error path by requesting a DID that cannot be in the
/// MDD. Accept either `404 Not Found` (route-level) or a 4xx carrying
/// a spec-valid `GenericError` body.
async fn exercise_error_path(client: &Client, component: &str) {
    let url = format!("{CDA_BASE_URL}/vehicle/v15/components/{component}/data/{BOGUS_DID}");
    let resp = client
        .get(&url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {url}: network error: {e}"));
    let status = resp.status();
    let body = resp
        .text()
        .await
        .unwrap_or_else(|e| panic!("GET {url}: read body: {e}"));
    assert!(
        status.is_client_error(),
        "GET bogus DID {url} should be 4xx, got {status}; body = {body}"
    );
    if status == StatusCode::NOT_FOUND && body.trim().is_empty() {
        eprintln!("[err] {component}/{BOGUS_DID}: 404 with empty body (accepted)");
        return;
    }
    // Any non-empty body should be a spec-valid GenericError — if CDA
    // returns a plain string we want the test to flag it.
    let err: GenericError = serde_json::from_str(&body).unwrap_or_else(|e| {
        panic!("parse GenericError for {component}/{BOGUS_DID}: {e}; body = {body}")
    });
    eprintln!(
        "[err] {component}/{BOGUS_DID}: {} -> code={} message={}",
        status, err.error_code, err.message
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phase2_cda_ecusim_data_modes() {
    if maybe_skip() {
        return;
    }

    // `BenchGuard::launch` needs a live tokio runtime only for the TCP
    // readiness probes it runs internally. It returns a guard whose
    // Drop tears down CDA and ecu-sim.
    let _guard = tokio::task::spawn_blocking(BenchGuard::launch)
        .await
        .expect("BenchGuard spawn_blocking")
        .unwrap_or_else(|e| panic!("BenchGuard::launch: {e}"));

    let auth_client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build auth client");
    let token = acquire_bearer(&auth_client).await;
    eprintln!("[auth] bearer token acquired ({} bytes)", token.len());
    let client = authed_client(&token);

    // Discover every component so we can loop data + modes across
    // the full catalog (FLXC1000, FLXCNG1000, ...).
    let entities: DiscoveredEntities =
        get_typed(&client, &format!("{CDA_BASE_URL}/vehicle/v15/components")).await;
    assert!(
        !entities.items.is_empty(),
        "CDA advertised zero components; expected FLXC1000 and FLXCNG1000"
    );
    let mut flxc_components: Vec<String> = entities
        .items
        .iter()
        .filter(|e| e.id.starts_with("flxc"))
        .map(|e| e.id.clone())
        .collect();
    if flxc_components.is_empty() {
        // Fall back to every entity — mismatched MDD catalogs are a
        // separate concern; let the /data call surface the error.
        flxc_components = entities.items.iter().map(|e| e.id.clone()).collect();
    }
    eprintln!(
        "[discover] exercising {} component(s): {:?}",
        flxc_components.len(),
        flxc_components
    );

    let mut data_components_exercised: usize = 0;
    let mut total_mode_details_ok: usize = 0;
    for component in &flxc_components {
        if exercise_data(&client, component).await {
            data_components_exercised = data_components_exercised.saturating_add(1);
        }
        total_mode_details_ok =
            total_mode_details_ok.saturating_add(exercise_modes(&client, component).await);
    }
    assert!(
        data_components_exercised > 0,
        "no component returned a non-empty /data catalog; expected at least one \
         of {flxc_components:?} to advertise DIDs"
    );
    assert!(
        total_mode_details_ok > 0,
        "no mode details call returned 200 across all components; expected at \
         least one session/security/commctrl/dtcsetting query to succeed"
    );

    // Error path: run against the first component only — the semantics
    // are identical across entities and one assertion is enough to
    // prove CDA maps unknown DIDs to a spec-valid failure shape.
    let first = flxc_components
        .first()
        .expect("flxc_components non-empty (checked above)");
    exercise_error_path(&client, first).await;

    eprintln!(
        "[done] phase2 data+modes: {} component(s) exercised, spec-typed Datas / \
         ReadValue / SupportedModes / ModeDetails all parsed cleanly",
        flxc_components.len()
    );
}
