-- SPDX-License-Identifier: Apache-2.0
-- SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)
--
-- Phase 3 Line A initial schema for sovd-db-sqlite, per ADR-0003.
--
-- faults
--   One row per ingested FaultRecord event (append-only log). The SovdDb
--   `list_faults` implementation aggregates rows by (component, code)
--   into the spec-shaped Fault entries that appear on the wire.
--
-- operation_cycles
--   Named cycles seen by snapshot_for_operation_cycle(). Used to tag
--   fault events with the cycle window they belong to. A snapshot at
--   end-of-cycle writes one row here with the cycle id and the end
--   timestamp.

CREATE TABLE IF NOT EXISTS faults (
    -- Monotonic row id, assigned by SQLite.
    row_id          INTEGER PRIMARY KEY AUTOINCREMENT,

    -- Component that reported the fault (ComponentId string).
    component       TEXT    NOT NULL,

    -- Wire code used on the SOVD response. See spec::fault::Fault.code.
    -- For the FaultRecord ingest path this is derived from FaultId as
    -- a hex string (uppercase, 6 hex digits). Clients may see this as
    -- the `code` field in GET /faults responses.
    code            TEXT    NOT NULL,

    -- Spec severity convention: 1=FATAL, 2=ERROR, 3=WARN, 4=INFO.
    severity        INTEGER NOT NULL,

    -- Millisecond monotonic timestamp from the Fault Library shim.
    timestamp_ms    INTEGER NOT NULL,

    -- Optional JSON payload (FaultRecord.meta), serialized as TEXT.
    meta_json       TEXT,

    -- Which operation cycle this event belongs to, set at ingest time
    -- if a cycle is currently active. NULL means "no active cycle".
    operation_cycle TEXT,

    -- True once snapshot_for_operation_cycle(cycle) has tagged this row.
    -- Used so clear_faults() can decide whether to touch snapshotted
    -- history or only live faults.
    snapshotted     INTEGER NOT NULL DEFAULT 0,

    -- Row creation time in UTC ISO-8601, for audit / debugging.
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_faults_component_code
    ON faults (component, code);

CREATE INDEX IF NOT EXISTS idx_faults_operation_cycle
    ON faults (operation_cycle);

CREATE TABLE IF NOT EXISTS operation_cycles (
    cycle_id   TEXT PRIMARY KEY,
    ended_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
