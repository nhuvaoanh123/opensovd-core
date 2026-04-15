<!--
SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)
SPDX-License-Identifier: Apache-2.0
-->

# Phase 5 Line A D1 — Pi full-stack deploy

This directory contains the deployment assets for running
`sovd-main` natively on the Taktflow Raspberry Pi bench host
(`taktflow-pi@192.168.0.197`). It is the green side of
`integration-tests/tests/phase5_pi_full_stack_bench.rs`.

Scope: **D1 only**. D2..D12 from
`eclipse-opensovd/docs/prompts/phase-5-line-a.md` are intentionally
out of scope for this file; they block on Line B bench readiness and
Line A follow-up work.

## Port plan (per phase-5-line-a.md)

| Service                    | Host                 | Port  | Source of truth |
|----------------------------|----------------------|-------|------------------|
| `sovd-main`                | `192.168.0.197`      | 21002 | `sovd-main.service` + `opensovd-pi.toml` |
| `ecu-sim`                  | `192.168.0.197`      | 13400 | `ecu-sim.service` (pre-existing) |
| CAN-to-DoIP proxy (L-B)    | `192.168.0.197`      | 13401 | `taktflow-can-doip-proxy.service` |
| CDA (dev host)             | `127.0.0.1`          | 20002 | phase-5-line-a.md |
| sovd-gateway (dev host)    | `127.0.0.1`          | 22002 | phase-5-line-a.md |

### :13400 conflict

Both the upstream CDA `ecu-sim` and the Phase 2 Line B CAN-to-DoIP
proxy were originally designed around port 13400. We resolve that
two ways:

1. **Port-plan level**: the proxy is moved to :13401, ecu-sim stays
   on :13400. Codified in `opensovd-pi.toml`, `sovd-main.service`
   (binds :21002 — does not participate in the :13400 conflict) and
   `taktflow-can-doip-proxy.service` (binds :13401).
2. **Systemd level**: `taktflow-can-doip-proxy.service` declares
   `Conflicts=ecu-sim.service`. If a future config re-points the
   proxy at :13400, systemd will stop ecu-sim before the proxy
   starts. Do not remove that `Conflicts=` line without re-verifying
   no port overlap.

## Deploy

From the dev host, at the top of the `opensovd-core` checkout:

```bash
./deploy/pi/phase5-full-stack.sh
```

Optional env vars:

- `PI` — override SSH target (default `taktflow-pi@192.168.0.197`)
- `TARGET_TRIPLE` — override cross-target (default
  `aarch64-unknown-linux-gnu`)
- `SOVD_MAIN_BIN` — point at a pre-built binary if you already have
  one cached
- `PROXY_BIN` — point at the Phase 2 Line B proxy binary. Default
  assumes the sibling `taktflow-embedded-production` layout. If the
  path does not resolve the script skips the proxy deploy **and**
  leaves the systemd unit disabled (D1 green still holds on
  `sovd-main` alone).

The script is idempotent: it uses `rsync -az`, `systemctl enable
--now`, and strips CRLF after transfer (same pattern as the existing
`install-ecu-sim.sh`). Safe to re-run.

After deploy, verify:

```bash
curl http://192.168.0.197:21002/sovd/v1/components
```

The response body is a `DiscoveredEntities` JSON structure with at
least the bench fleet components (`cvc`, `fzc`, `rzc`).

## Run the live test

From the dev host:

```bash
cd opensovd-core
TAKTFLOW_BENCH=1 cargo test -p integration-tests \
    --test phase5_pi_full_stack_bench -- --nocapture
```

Expected: one test, passes, prints
`phase5_pi_full_stack_bench: D1 topology green against 192.168.0.197:21002 ...`.

If `TAKTFLOW_BENCH` is unset, the test skips cleanly (passes
without touching the Pi). That is the CI default and must stay that
way.

## Roll back

```bash
ssh taktflow-pi@192.168.0.197 'sudo systemctl disable --now sovd-main.service'
ssh taktflow-pi@192.168.0.197 'sudo systemctl disable --now taktflow-can-doip-proxy.service || true'
ssh taktflow-pi@192.168.0.197 'sudo rm -f /etc/systemd/system/sovd-main.service \
                                        /etc/systemd/system/taktflow-can-doip-proxy.service'
ssh taktflow-pi@192.168.0.197 'sudo systemctl daemon-reload'
ssh taktflow-pi@192.168.0.197 'sudo rm -rf /opt/taktflow/sovd-main /opt/taktflow/proxy'
```

ecu-sim.service is untouched by rollback.

## Known limitations

- `sovd-main` on the Pi currently runs the in-memory demo server
  (`cvc`, `fzc`, `rzc`), not a DFM-backed real bench fleet.
  Promoting it to a DFM-backed config is a Phase 5 follow-up
  deliverable and out of scope for D1.
- The Phase 2 Line B CAN-to-DoIP proxy binary is an optional
  dependency — the deploy script skips it gracefully if the source
  artifact does not resolve. Full bench readiness requires Line B
  D1..D3 (physical STM32 flashing), which is not gated by D1.
- Cross-compiling `sovd-main` for `aarch64-unknown-linux-gnu` needs
  a linker. If you do not have one installed, pass `SOVD_MAIN_BIN`
  at a pre-built binary (for example, one produced on the Linux
  laptop `an-dao@192.168.0.158`).
