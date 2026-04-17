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
# Deploy the upstream CDA ecu-sim Kotlin simulator onto the Raspberry Pi
# bench host as a Docker container managed by systemd.
#
# Prerequisites on the Pi:
#   - Docker Engine installed and running
#   - Current user (taktflow-pi) has passwordless sudo for systemctl
#
# This script runs from the dev machine. It rsyncs the ecu-sim source, runs
# `docker build` on the Pi (aarch64 — we do NOT build the amd64 image here),
# installs the systemd unit, and verifies the service is listening on 13400.

set -euo pipefail

PI=${PI:-bench-pi@192.0.2.10}
REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
CDA_ECUSIM=${CDA_ECUSIM:-$REPO_ROOT/../classic-diagnostic-adapter/testcontainer/ecu-sim}
SYSTEMD_UNIT=$REPO_ROOT/deploy/pi/ecu-sim.service

if [ ! -d "$CDA_ECUSIM" ]; then
    echo "error: CDA ecu-sim source not found at $CDA_ECUSIM" >&2
    exit 1
fi

echo "[1/4] Syncing ecu-sim source to $PI:~/ecu-sim/"
rsync -az --delete "$CDA_ECUSIM/" "$PI:~/ecu-sim/"

# Upstream CDA is typically checked out on Windows with core.autocrlf=true,
# so rsync carries CRLF line endings into the Pi. That breaks the docker
# entrypoint (#!/bin/bash -ex gets read as the literal "bash -ex\r").
# Normalize all shell scripts to LF on the Pi before docker build.
echo "[1b/4] Normalizing line endings of shell scripts on the Pi (CRLF -> LF)"
ssh "$PI" 'cd ~/ecu-sim && find . -type f \( -name "*.sh" -o -name gradlew \) -print0 | xargs -0 sed -i "s/\r$//"'

echo "[2/4] Building docker image taktflow/cda-ecu-sim:latest on the Pi (aarch64)"
ssh "$PI" 'cd ~/ecu-sim && docker build -f docker/Dockerfile -t taktflow/cda-ecu-sim:latest .'

echo "[3/4] Installing systemd unit"
scp "$SYSTEMD_UNIT" "$PI:/tmp/ecu-sim.service"
ssh "$PI" 'sudo mv /tmp/ecu-sim.service /etc/systemd/system/ecu-sim.service \
           && sudo systemctl daemon-reload \
           && sudo systemctl enable --now ecu-sim.service'

echo "[4/4] Verification"
ssh "$PI" 'sudo systemctl status ecu-sim.service --no-pager || true; docker ps | grep -E "ecu-sim" || echo "ecu-sim container not yet running"'

echo
echo "done — ecu-sim should be listening on $PI:13400"
