#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)
#
# See the NOTICE file(s) distributed with this work for additional
# information regarding copyright ownership.
#
# Phase 5 Line A D1 - full-stack deploy of sovd-main (and optionally
# the Phase 2 Line B CAN-to-DoIP proxy) to the Raspberry Pi bench
# host. Idempotent: safe to re-run.
#
# Prerequisites on the dev host:
#   - Rust toolchain that cross-compiles to aarch64-unknown-linux-gnu
#     OR a cached release binary from a prior Pi build
#   - rsync + ssh to $PI (default bench-pi@192.0.2.10)
#   - Passwordless sudo on the Pi for systemctl
#
# What this script does (all rsync-based, all idempotent):
#   1. Build (or locate) sovd-main release binary for aarch64
#   2. rsync binary + resolved sovd-main config to /opt/taktflow/sovd-main/
#   3. rsync systemd unit and enable --now
#   4. Optionally rsync Phase 2 Line B proxy binary if it is resolvable
#      on the dev host, then enable its unit. If not resolvable, log
#      the skip and leave the unit disabled. sovd-main-only is still a
#      valid D1 green state per README-phase5.md.
#   5. Normalise CRLF -> LF on shell scripts (same CRLF stripping
#      pattern as install-ecu-sim.sh)
#   6. Verify sovd-main answers GET /sovd/v1/components on
#      $PI_HTTP_HOST:21002
#
# Non-goals (expansions blocked on Line B bench readiness or on other
# D-deliverables of phase-5-line-a.md):
#   - Flashing STM32 targets (Line B D1..D3)
#   - HIL scenarios D2..D9
#   - Performance envelope D10
#
# Port plan (per phase-5-line-a.md):
#   sovd-main      0.0.0.0:21002  on Pi
#   ecu-sim         :13400        on Pi (pre-existing, install-ecu-sim.sh)
#   proxy           :13401        on Pi (this script if bin resolvable)
#
# :13400 conflict: ecu-sim and the proxy BOTH historically targeted
# :13400. We resolve it at the port-plan level (proxy -> :13401) and
# enforce it at the systemd level (proxy unit has Conflicts=ecu-sim
# so any future move to :13400 still guarantees mutual exclusion).
# See taktflow-can-doip-proxy.service for the unit-level note.

set -euo pipefail

PI=${PI:-bench-pi@192.0.2.10}
PI_HTTP_HOST=${PI_HTTP_HOST:-${PI##*@}}
REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DEPLOY_DIR=$REPO_ROOT/deploy/pi
REMOTE_SOVD_DIR=/opt/taktflow/sovd-main
REMOTE_PROXY_DIR=/opt/taktflow/proxy
TARGET_TRIPLE=${TARGET_TRIPLE:-aarch64-unknown-linux-gnu}
SOVD_MAIN_BIN=${SOVD_MAIN_BIN:-$REPO_ROOT/target/$TARGET_TRIPLE/release/sovd-main}
SOVD_CONFIG_FILE=${SOVD_CONFIG_FILE:-$DEPLOY_DIR/opensovd-pi.toml}
PHASE5_CDA_BASE_URL=${PHASE5_CDA_BASE_URL:-}
PHASE5_CDA_PLACEHOLDER=${PHASE5_CDA_PLACEHOLDER:-http://198.51.100.10:20002}
# Phase 2 Line B proxy binary. The Line B repo lives as a sibling
# workspace (taktflow-embedded-production) per phase-2-line-b.md;
# override PROXY_BIN if your layout differs. If the path does not
# resolve we skip the proxy rsync - it is an optional dependency for
# D1, NOT a hard prerequisite.
PROXY_BIN=${PROXY_BIN:-$REPO_ROOT/../../taktflow-embedded-production/posix/build/taktflow-can-doip-proxy}

log() { printf '[phase5-full-stack] %s\n' "$*"; }
warn() { printf '[phase5-full-stack][WARN] %s\n' "$*" >&2; }
err() { printf '[phase5-full-stack][ERR ] %s\n' "$*" >&2; }

CONFIG_RENDER=
cleanup() {
    if [ -n "${CONFIG_RENDER:-}" ] && [ -f "$CONFIG_RENDER" ]; then
        rm -f "$CONFIG_RENDER"
    fi
}
trap cleanup EXIT

resolve_sovd_config_source() {
    if [ ! -f "$SOVD_CONFIG_FILE" ]; then
        err "SOVD_CONFIG_FILE=$SOVD_CONFIG_FILE does not exist"
        exit 1
    fi

    if [ -n "$PHASE5_CDA_BASE_URL" ]; then
        CONFIG_RENDER=$(mktemp)
        sed "s|$PHASE5_CDA_PLACEHOLDER|$PHASE5_CDA_BASE_URL|g" \
            "$SOVD_CONFIG_FILE" > "$CONFIG_RENDER"
        log "rendered sovd-main config from $SOVD_CONFIG_FILE with PHASE5_CDA_BASE_URL=$PHASE5_CDA_BASE_URL"
        printf '%s' "$CONFIG_RENDER"
        return
    fi

    if grep -Fq "$PHASE5_CDA_PLACEHOLDER" "$SOVD_CONFIG_FILE"; then
        warn "SOVD_CONFIG_FILE=$SOVD_CONFIG_FILE still contains the public-safe CDA placeholder $PHASE5_CDA_PLACEHOLDER"
        warn "       Live cda_forward traffic will not work until PHASE5_CDA_BASE_URL points at the real CDA host."
    fi

    printf '%s' "$SOVD_CONFIG_FILE"
}

# ---------------------------------------------------------------
# 1. Resolve or build the sovd-main release binary
# ---------------------------------------------------------------
if [ ! -x "$SOVD_MAIN_BIN" ]; then
    log "sovd-main aarch64 release binary not found at $SOVD_MAIN_BIN"
    log "attempting cross-compile: cargo build -p sovd-main --release --target $TARGET_TRIPLE"
    if ! (cd "$REPO_ROOT" && cargo build -p sovd-main --release --target "$TARGET_TRIPLE"); then
        err "cross-compile failed. Either install the aarch64 toolchain"
        err "(rustup target add $TARGET_TRIPLE + a linker) or point"
        err "SOVD_MAIN_BIN at a pre-built binary and rerun."
        exit 1
    fi
fi

if [ ! -x "$SOVD_MAIN_BIN" ]; then
    err "SOVD_MAIN_BIN=$SOVD_MAIN_BIN still missing after build attempt"
    exit 1
fi
log "using sovd-main binary: $SOVD_MAIN_BIN"
SOVD_CONFIG_SOURCE=$(resolve_sovd_config_source)
log "using sovd-main config source: $SOVD_CONFIG_FILE"

# ---------------------------------------------------------------
# 2. rsync sovd-main + config to the Pi
# ---------------------------------------------------------------
log "[1/6] preparing /opt/taktflow on $PI"
ssh "$PI" "sudo mkdir -p $REMOTE_SOVD_DIR $REMOTE_PROXY_DIR \
           && sudo chown -R taktflow-pi:taktflow-pi /opt/taktflow"

log "[2/6] rsync sovd-main binary -> $PI:$REMOTE_SOVD_DIR/"
rsync -az --chmod=F755 "$SOVD_MAIN_BIN" "$PI:$REMOTE_SOVD_DIR/sovd-main"

log "[3/6] rsync sovd-main config -> $PI:$REMOTE_SOVD_DIR/opensovd.toml"
rsync -az "$SOVD_CONFIG_SOURCE" "$PI:$REMOTE_SOVD_DIR/opensovd.toml"
# CRLF -> LF (matches install-ecu-sim.sh pattern)
ssh "$PI" "sed -i 's/\r\$//' $REMOTE_SOVD_DIR/opensovd.toml"
log "[3b/6] repairing ownership under /opt/taktflow"
ssh "$PI" "sudo chown -R taktflow-pi:taktflow-pi $REMOTE_SOVD_DIR $REMOTE_PROXY_DIR"

# ---------------------------------------------------------------
# 3. Install + start sovd-main systemd unit
# ---------------------------------------------------------------
log "[4/6] installing sovd-main.service"
scp "$DEPLOY_DIR/systemd/sovd-main.service" "$PI:/tmp/sovd-main.service"
ssh "$PI" "sudo mv /tmp/sovd-main.service /etc/systemd/system/sovd-main.service \
           && sudo sed -i 's/\r\$//' /etc/systemd/system/sovd-main.service \
           && sudo systemctl daemon-reload \
           && sudo systemctl enable --now sovd-main.service"

# ---------------------------------------------------------------
# 4. Optional: Phase 2 Line B proxy
# ---------------------------------------------------------------
if [ -x "$PROXY_BIN" ]; then
    log "[5/6] proxy binary found at $PROXY_BIN, deploying to $PI:$REMOTE_PROXY_DIR/"
    rsync -az --chmod=F755 "$PROXY_BIN" "$PI:$REMOTE_PROXY_DIR/taktflow-can-doip-proxy"
    log "[5a/6] rsync proxy config -> $PI:$REMOTE_PROXY_DIR/proxy.toml"
    rsync -az "$DEPLOY_DIR/opensovd-proxy.toml" "$PI:$REMOTE_PROXY_DIR/proxy.toml"
    scp "$DEPLOY_DIR/systemd/taktflow-can-doip-proxy.service" \
        "$PI:/tmp/taktflow-can-doip-proxy.service"
    ssh "$PI" "sudo mv /tmp/taktflow-can-doip-proxy.service \
                   /etc/systemd/system/taktflow-can-doip-proxy.service \
               && sed -i 's/\r\$//' $REMOTE_PROXY_DIR/proxy.toml \
               && sudo sed -i 's/\r\$//' /etc/systemd/system/taktflow-can-doip-proxy.service \
               && sudo systemctl daemon-reload \
               && sudo systemctl enable --now taktflow-can-doip-proxy.service"
else
    warn "[5/6] proxy binary not found at $PROXY_BIN - skipping proxy deploy."
    warn "       D1 green holds on sovd-main alone; the proxy unit is"
    warn "       shipped to /etc/systemd/system/ but NOT enabled until"
    warn "       Line B bench readiness (see README-phase5.md)."
fi

# ---------------------------------------------------------------
# 5. Verification
# ---------------------------------------------------------------
log "[6/6] verification"
ssh "$PI" 'sudo systemctl --no-pager status sovd-main.service || true'

# Give axum a moment to bind the socket after enable --now.
sleep 2
if curl -fsS --max-time 5 "http://$PI_HTTP_HOST:21002/sovd/v1/components" >/dev/null; then
    log "sovd-main answering GET /sovd/v1/components on $PI_HTTP_HOST:21002 - D1 green"
else
    err "sovd-main is NOT answering on $PI_HTTP_HOST:21002"
    err "check 'journalctl -u sovd-main.service -n 100' on $PI"
    exit 2
fi

log "done"
