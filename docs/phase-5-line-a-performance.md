<!--
SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)
SPDX-License-Identifier: Apache-2.0
-->

# Phase 5 Line A Performance Validation

Date: 2026-04-15

## Scope

This note records the Phase 5 Line A D10 live measurement pass against
the Raspberry Pi bench host at `192.0.2.10`.

- Target service: `sovd-main.service`
- Target endpoint: `GET /sovd/v1/components/cvc/faults`
- Pi listener: `0.0.0.0:21002`
- Measurement client: Windows dev host on the same bench LAN
- Warmup: 20 requests
- Sample size: 500 sequential requests

This run happened after repairing the Pi deploy so
`/opt/taktflow/sovd-main` is writable by the `taktflow-pi` service
user and after pinning `backend.sqlite_path` to
`/opt/taktflow/sovd-main/dfm.db` in `deploy/pi/opensovd-pi.toml`.

Important scope note: the full D2-D9 HIL bench fleet is still gated on
`PHASE5_BENCH_READY=1`. The live target measured here is the current
Pi-hosted `sovd-main` deployment used by D1 and D10, which exposes the
`cvc`, `fzc`, `rzc`, and `dfm` component surface and answers the
`/cvc/faults` read path over the LAN.

## Verification

The live D1 topology test was rerun first:

```text
TAKTFLOW_BENCH=1 cargo test -p integration-tests --test phase5_pi_full_stack_bench -- --nocapture
test phase5_pi_full_stack_bench ... ok
```

The Pi runtime state during measurement:

- `sovd-main.service`: `active`
- `taktflow-can-doip-proxy.service`: `inactive`
- `ecu-sim.service`: `failed`

## Results

Primary measurement used a persistent HTTP/1.1 client
(`python http.client.HTTPConnection`) so the sample reflects steady
state bench traffic with connection reuse, matching the integration
tests' reuse of a single `reqwest::Client`.

| Metric | Target | Observed | Verdict |
|--------|--------|----------|---------|
| `/faults` median latency | `< 100 ms` | `7.823 ms` | PASS |
| `/faults` P99 latency | `< 500 ms` | `249.664 ms` | PASS |
| OpenSOVD Pi RAM footprint | `< 200 MB` | `VmRSS 9.424 MB` | PASS |

Supporting numbers from the same 500-request run:

- average: `46.353 ms`
- minimum: `3.959 ms`
- maximum: `324.116 ms`
- Pi process virtual size: `VmSize 354056 kB`
- Pi process resident high-water mark: `VmHWM 9424 kB`

## Notes

A one-shot reconnect sample using Python `urllib.request.urlopen`
re-opened transport for each request and was materially slower:

- median: `135.562 ms`
- P99: `527.374 ms`

That reconnect-heavy sample is useful as a stress hint, but it is not
the official D10 pass/fail number because the normal bench harness does
not create a brand-new TCP client for every read.
