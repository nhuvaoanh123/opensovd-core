#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)
#
# See the NOTICE file(s) distributed with this work for additional
# information regarding copyright ownership.
#
# This program and the accompanying materials are made available under the
# terms of the Apache License Version 2.0 which is available at
# https://www.apache.org/licenses/LICENSE-2.0
#
# Launch the Classic Diagnostic Adapter natively on this Windows dev
# machine, pointing at the Pi-hosted upstream ecu-sim over the bench LAN.
#
# Usage:
#   ./deploy/sil/run-cda-local.sh                 # foreground
#   ./deploy/sil/run-cda-local.sh &                # background, sleep 3, curl health

set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
CDA_ROOT=${CDA_ROOT:-$REPO_ROOT/../classic-diagnostic-adapter}
CDA_BIN=${CDA_BIN:-$CDA_ROOT/target/release/opensovd-cda.exe}
CDA_CONFIG=${CDA_CONFIG:-$REPO_ROOT/deploy/sil/opensovd-cda.toml}
PI_HOST=${PI_HOST:-192.0.2.10}
PI_USER=${PI_USER:-taktflow-pi}

if [ ! -x "$CDA_BIN" ] && [ ! -f "$CDA_BIN" ]; then
    echo "error: CDA binary not found at $CDA_BIN" >&2
    echo "hint: build with" >&2
    echo "      cd $CDA_ROOT && cargo build --release --no-default-features --features health,openssl-vendored -p opensovd-cda" >&2
    exit 1
fi

if [ ! -f "$CDA_CONFIG" ]; then
    echo "error: CDA config not found at $CDA_CONFIG" >&2
    exit 1
fi

echo "[preflight] pinging Pi $PI_HOST"
if command -v ping >/dev/null 2>&1; then
    if ping -n 1 -w 2000 "$PI_HOST" >/dev/null 2>&1 || ping -c 1 -W 2 "$PI_HOST" >/dev/null 2>&1; then
        echo "[preflight] Pi reachable"
    else
        echo "warning: Pi $PI_HOST did not respond to ping (non-fatal on some LANs)" >&2
    fi
fi

echo "[preflight] checking ecu-sim on Pi via ssh"
if ssh -o BatchMode=yes -o ConnectTimeout=5 "$PI_USER@$PI_HOST" 'docker ps | grep -q ecu-sim' 2>/dev/null; then
    echo "[preflight] ecu-sim container running on Pi"
else
    echo "error: ecu-sim not running on $PI_HOST" >&2
    echo "hint: run deploy/pi/install-ecu-sim.sh from this machine" >&2
    exit 2
fi

echo "[launch] starting CDA"
echo "  binary: $CDA_BIN"
echo "  config: $CDA_CONFIG"
# CDA loads its TOML config via the CDA_CONFIG_FILE env variable
# (see cda-main/src/config/mod.rs::load_config). CLI flags would override
# individual fields but there is no --config-file switch upstream.
export CDA_CONFIG_FILE="$CDA_CONFIG"
cd "$REPO_ROOT"
exec "$CDA_BIN"
