<!--
SPDX-License-Identifier: Apache-2.0
SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Apache License Version 2.0 which is available at
https://www.apache.org/licenses/LICENSE-2.0
-->

# Phase 0 Line A -- TDD Punch List (2026-04-14, second pass)

Philosophy: mirror upstream `classic-diagnostic-adapter` (CDA) wholesale. Let
broken CI and broken `cargo build` be the phase-by-phase punch list. TDD for
infrastructure -- failures tell us what to build next. Each expected failure
below is a feature, not a bug.

Reference: `H:\taktflow-opensovd\classic-diagnostic-adapter`

## Section 1 -- What we now mirror wholesale

Items restored from prior "phase-gated skip" under the new wholesale-mirror
rule:

### Workflows
- `.github/workflows/build.yml` -- replaced our lean variant with CDA's full
  file verbatim (only SPDX year 2025 -> 2026). Now contains:
  - `features` job (nightly-2025-07-14, protoc, DLT setup, 4 feature-flag
    variants: `--all-features`, `-p opensovd-cda --features mbedtls`,
    `--no-default-features`, `-p opensovd-cda --no-default-features`).
  - `build_and_test` job (Docker Buildx, CDA testcontainer image,
    ecu-sim testcontainer image, `cargo test --features integration-tests`).
  - `build_windows` job (OpenSSL env setup).
  - Package references left as `opensovd-cda` verbatim -- the missing package
    IS the punch list.
- `.github/workflows/generate_documentation.yml` -- copied verbatim (sphinx +
  rustdoc for `cda-plugin-security`). Both subjobs will fail.

### Cargo.toml -- `[workspace.dependencies]`
Declared (will not resolve or will remain unused until enabled):
- `prost`, `prost-build` -- protobuf wire format (Fault Library IPC, Phase 3).
- `flatbuffers` -- MDD format (Phase 4).
- `doip-codec`, `doip-definitions` -- DoIP protocol (Phase 4 comm).
- `xz2` -- MDD compression (Phase 4).
- `ouroboros` -- self-referential structs for MDD (Phase 4).
- `memmap2` -- mmap MDD large files (Phase 4).
- `cargo_toml` -- used in CDA build-script (Phase 4 build-script parity).
- `tracing-dlt`, `dlt-rs`, `dlt-sys` -- DLT output (Phase 6).
- `mbedtls-sys`, `mbedtls-rs` -- path deps at `comm-mbedtls/...` (do not
  exist; Phase 4 creates them).

### Cargo.toml -- `[patch.crates-io]`
- `doip-codec` -> theswiftfox fork rev `0dba319`.
- `doip-definitions` -> theswiftfox fork rev `bdeab8c`.
- `flatbuffers` -> alexmohr fork rev `0ba3307d`.
- (`aide` -> alexmohr fork rev `56355cb` was already present.)

All four patches report "was not used in the crate graph" until a member
crate pulls them in. This is the punch list for Phase 2/4.

### deny.toml
- `[advisories]` ignores: `RUSTSEC-2023-0071`, `RUSTSEC-2026-0097` (both
  scoped to jsonwebtoken / RSA CRT side-channel -- jsonwebtoken is already in
  `[workspace.dependencies]`).
- `[[licenses.exceptions]]` `webpki-roots` / `CDLA-Permissive-2.0`.
- `[[licenses.exceptions]]` `libbz2-rs-sys` / `bzip2-1.0.6`.
- `[sources].allow-git` list of 4 theswiftfox repos
  (doip-codec, doip-definitions, doip-sockets, mimalloc).

### testcontainer/ stub layout
- `testcontainer/cda/Dockerfile` -- one-line stub `# TODO Phase 2: implement`.
- `testcontainer/ecu-sim/docker/Dockerfile` -- one-line stub.
- `testcontainer/ecu-sim/.gitkeep` -- parity with CDA layout.

The Docker build stages in `build.yml` now find the Dockerfiles, but the
actual image build will fail -- intended punch list signal for Phase 2.

## Section 2 -- Expected CI failures as punch list

Parsed from the local `cargo build --workspace` run and static reading of the
mirrored workflows.

### Phase 2 fixes -- Docker testcontainer + CDA integration tests
- `build_and_test.Build CDA docker image` -- `testcontainer/cda/Dockerfile`
  is a one-line `# TODO` stub; docker build will fail.
- `build_and_test.Build ECU Sim docker image` -- same, stub Dockerfile,
  no context beyond an empty directory.
- `build_and_test.Run tests` -- `cargo test --locked --features
  integration-tests -- --show-output` will fail because no crate declares
  the `integration-tests` feature yet.

### Phase 3 fixes -- protoc + Fault Library IPC, features job
- `features.Install Protoc` will succeed (action works), but subsequent
  `cargo build --all-features` will fail: no workspace crate declares
  features, so `--all-features` has nothing to turn on and the nightly
  toolchain cannot find `-p opensovd-cda`.
- `features.Setup DLT tracing lib` -- action from
  `eclipse-opensovd/dlt-tracing-lib` will attempt to install libdlt. May
  succeed as a setup step; build will still fail because `tracing-dlt`
  declared but unused.
- `features.Build all features` -- no workspace crate with features yet.
- `features.Build mbedtls features` -- `-p opensovd-cda` does not exist.
- `features.Build minimal features` -- `cargo build --no-default-features`
  may actually pass for our current crates; this is the one step that might
  land green.
- `features.Build minimal CDA only` -- `-p opensovd-cda` does not exist.

### Phase 4 fixes -- `-p opensovd-cda` package + full features + DoIP + mbedtls
- No `opensovd-cda` package exists in the workspace. The CLI flag
  `-p opensovd-cda` fails the `features` job at multiple steps.
- Declared but unresolvable: `doip-codec`, `doip-definitions`,
  `flatbuffers` patches are unused until a member crate pulls them in.
- `mbedtls-sys`, `mbedtls-rs` path deps point at `comm-mbedtls/mbedtls-sys`
  and `comm-mbedtls/mbedtls-rs` -- directories do not exist. The moment any
  member declares `mbedtls-sys = { workspace = true }` this fails at resolve
  time, which is the Phase 4 signal.
- `flatbuffers`, `ouroboros`, `memmap2`, `xz2`, `cargo_toml` -- all mirror
  MDD/build-script parity; inert until pulled in by a phase-4 crate.

### Phase 6 fixes -- DLT tracing, OpenSSL Windows, generate_documentation
- `generate_documentation.build_documentation_and_trace_requirements` --
  `docs/Dockerfile` does not exist. Docker build fails at
  "docker/build-push-action@v5".
- `generate_documentation.build_rustdoc_security_plugin` --
  `cargo doc --no-deps -p cda-plugin-security` fails: package does not
  exist.
- `build_windows.Setup OpenSSL (Windows) Environment` -- will set env vars
  to `C:\Program Files\OpenSSL` which is not pre-installed on
  windows-latest by default; subsequent `cargo build` fails on openssl-sys
  linking unless pre-provided.
- `features.Setup DLT tracing lib` -- is a Phase 6 signal even though the
  action may succeed; the `tracing-dlt` / `dlt-rs` / `dlt-sys` workspace
  deps remain unused and will punch-list when a member enables them.

## Section 3 -- Permanent skips (true divergence)

These remain deliberately NOT mirrored because mirroring them would DIVERGE
from CDA (not converge):

- `LICENSES/` directory and `.reuse/dep5` -- CDA uses REUSE v3 / `REUSE.toml`
  inline, not REUSE v1. Adding `LICENSES/` would diverge from CDA's house
  style.
- `rust-toolchain.toml` -- CDA has none. We keep ours as a local-dev-only
  pin of `1.88.0`; CI pulls the toolchain via `dtolnay/rust-toolchain@master`
  just like CDA.
- `SECURITY.md`, `CHANGELOG.md`, `CODE_OF_CONDUCT.md` -- absent upstream.
- `.editorconfig`, `.gitattributes`, `taplo.toml` -- absent upstream.
- `.github/ISSUE_TEMPLATE/`, PR template, `dependabot.yml`, `FUNDING.yml` --
  absent upstream.

## Section 4 -- Build exit code

Local `cargo build --workspace` (stable 1.88.0, `--locked`) on
2026-04-14 after the second-pass mirror commits:

- Exit code: **0** (success).
- Warnings:
  - `Patch aide v0.16.0-alpha.1 was not used in the crate graph.`
  - `Patch doip-codec v2.0.8 was not used in the crate graph.`
  - `Patch doip-definitions v3.0.13 was not used in the crate graph.`
  - `Patch flatbuffers v25.9.23 was not used in the crate graph.`

The workspace compiles because no member crate has pulled in the CDA-specific
dependencies yet. The patch-unused warnings ARE the punch list signal --
every warning disappears when we wire in the corresponding member crate.

Verified: `cargo build -p sovd-main` still succeeds. The direct,
non-feature-flagged `/sovd/v1/health` endpoint code path is untouched and
runs.

The first "real" failure surfaces at CI time (remote), not at local
`cargo build`:
1. `features.Build all features` -- fails first (no workspace features).
2. `features.Build mbedtls features` -- fails next (`-p opensovd-cda` DNE).
3. `build_and_test.Build CDA docker image` -- fails at actual image build.
4. `build_and_test.Run tests` -- fails (`integration-tests` feature DNE).
5. `generate_documentation.*` -- both subjobs fail (no sphinx, no
   `cda-plugin-security`).

That ordered list IS the Phase 2/3/4/6 execution order.
