# Phase 4 Line A D9 Live Bench Pass — 2026-04-15

This document records the first **live-green** run of
`phase4_sovd_gateway_cda_ecusim_bench` against the real upstream CDA
binary and the Pi-hosted ecu-sim after the `CdaBackend` path-prefix fix.

## Commit under test

```
c84891f fix(phase-4): target real CDA component flxc1000 in D9 bench
d3fc180 test(phase-4): D3 preflight asserts CDA route prefix matches
3934d03 feat(cda-backend): D1+D2-green -- configurable path prefix, default /vehicle/v15
29fc8cb test(cda-backend): D1-red -- path prefix configurable
```

Branch: `auto/line-a/cda-backend-path-config-2026-04-15`, based on
`feature/phase-0-scaffold`.

## Topology

```
cargo test (Windows dev box)
    └── sovd-server in-process
            ├── DFM forward          (dfm component, SQLite temp)
            └── CdaBackend forward   (flxc1000)
                    ↓ HTTP http://127.0.0.1:20002/vehicle/v15/*
                CDA native binary
                    └── DoIP 192.0.2.10:13400
                            └── Pi ecu-sim (docker, host net, wlan0)
```

## Runner

- Pi ecu-sim: `sudo systemctl reset-failed ecu-sim && sudo systemctl start ecu-sim`
  → active, DoIP Local Address `wlan0:13400`.
- CDA: `CDA_CONFIG_FILE=H:/eclipse-opensovd/opensovd-core/deploy/sil/opensovd-cda.toml
  H:/eclipse-opensovd/classic-diagnostic-adapter/target/release/opensovd-cda.exe`
  (background), listening on `127.0.0.1:20002`.
- Test invocation:
  `TAKTFLOW_BENCH=1 cargo test -p integration-tests --test phase4_sovd_gateway_cda_ecusim_bench -- --nocapture`

## Preflight evidence

The D3 `CdaBackend::preflight()` guard ran first and confirmed the
path prefix matches upstream before the harness booted:

```
phase4 full-chain bench preflight ok: http://127.0.0.1:20002/ + path_prefix="vehicle/v15"
```

Raw CDA component listing observed during manual sanity probe:

```
GET http://127.0.0.1:20002/vehicle/v15/components
200 OK
{"items":[{"href":"http://localhost:20002/Vehicle/v15/components/flxcng1000",
"id":"flxcng1000","name":"flxcng1000"},{"href":"http://localhost:20002/Vehicle/v15/
components/flxc1000","id":"flxc1000","name":"flxc1000"}],
"x-sovd2uds-lin-ecus":[],"x-sovd2uds-can-ecus":[]}
```

A `GET /vehicle/v15/components/flxc1000/faults` returns `401 Unauthorized`,
which the bench test's assertion explicitly accepts (CDA has auth enabled
and the test's goal is to prove the forward reached CDA, not to run
authenticated operations — see the inline comment at step 4 of the
bench test).

## Test output

```
running 1 test
phase4 full-chain bench preflight ok: http://127.0.0.1:20002/ + path_prefix="vehicle/v15"
phase4_sovd_gateway_cda_ecusim_bench: 5 MVP use cases green against 192.0.2.10:13400
test phase4_sovd_gateway_cda_ecusim_bench ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
```

## Five MVP use cases exercised

1. `GET /sovd/v1/components/dfm/faults` — 200, `ListOfFaults` deserialised,
   at least one item (the injected `0xDEAD` record).
2. `GET /sovd/v1/components/dfm/faults/{first_code}` — 200, single fault.
3. `DELETE /sovd/v1/components/dfm/faults` — 204 no content.
4. `GET /sovd/v1/components/flxc1000/faults` — forwarded through
   `CdaBackend` → `/vehicle/v15/components/flxc1000/faults` on CDA.
   Response observed: 401 Unauthorized (accepted — CDA auth is on; the
   fact that CDA responded with a semantically meaningful status, not
   a 404, proves the `DEFAULT_CDA_PATH_PREFIX` matches upstream).
5. `GET /sovd/v1/components` — 200, `DiscoveredEntities` with both
   `dfm` and `flxc1000` visible.

## Why this was previously failing

Before this branch, `CdaBackend::component_url` hardcoded
`sovd/v1/components/...`, which produced downstream requests like
`GET http://127.0.0.1:20002/sovd/v1/components/ecu-sim/faults`. Upstream
`cda-sovd` (see
`classic-diagnostic-adapter/cda-sovd/src/sovd/mod.rs`) only mounts its
REST surface under `/vehicle/v15/*` and has no router entry for
`/sovd/v1/*`, so every request 404'd at the HTTP layer before any UDS
translation happened. Phase 4 Line A's D9 verification correctly caught
this as `CDA forward status: 404 Not Found`.

The fix (ADR-0006 max-sync rationale):

1. `CdaBackend` gained a `path_prefix` field with a
   `DEFAULT_CDA_PATH_PREFIX = "vehicle/v15"` constant, documented as the
   single place to flip if upstream ever migrates to `/sovd/v1/*`
   natively.
2. `CdaBackend::new_with_path_prefix(...)` lets the few call sites that
   need a different prefix (currently only
   `phase2_sovd_over_cda_ecusim` against its mock `InMemoryServer`)
   opt in explicitly.
3. `CdaBackend::preflight()` surfaces prefix mismatches loudly, via
   `SovdError::InvalidRequest`, so future drift fails at preflight and
   not mid-test.

## Gates

- `cargo build --workspace` — clean.
- `cargo test --workspace` — all tests green (including the two new
  `phase4_cda_backend_preflight` cases and the five revised
  `cda.rs` unit tests).
- `TAKTFLOW_BENCH=1 cargo test -p integration-tests --test
  phase4_sovd_gateway_cda_ecusim_bench -- --nocapture` — live-green,
  output above.
