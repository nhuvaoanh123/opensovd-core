<!--
SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD
SPDX-License-Identifier: Apache-2.0
-->

# OpenSOVD Core — Role Boundaries

This document defines the role boundaries between the crates in this workspace.
It is a contract doc, not a tutorial. Terms and responsibilities are taken from
[Eclipse OpenSOVD design.md](../opensovd/docs/design/design.md) verbatim where
possible. Do not invent new roles — extend this file when the upstream doc moves.

Scope: `opensovd-core` workspace only. Upstream `classic-diagnostic-adapter`
(CDA), `fault-lib`, and `uds2sovd-proxy` are peripheral and appear here only to
the extent they cross our trait boundaries.

## Call direction (who calls whom)

```
   Off-board / on-board / cloud TESTER
   (uses sovd-client to speak REST)
                  |
                  | HTTPS  (SOVD REST)
                  v
          +---------------+          +-----------------------+
          |  sovd-gateway |<-- uses -| sovd-client (outbound)|
          |  (system-wide |           +-----------------------+
          |   router)     |             (gateway -> native SOVD ECU)
          +-------+-------+
                  |
      routes to one or more backends:
                  |
      +-----------+-----------+--------------------+
      v                       v                    v
 +----------+           +----------+         +----------+
 |sovd-     |           |   CDA    |         |  another |
 |server    |           |(legacy   |         |  sovd-   |
 |(native,  |           | UDS ECU) |         |  gateway |
 |one ECU)  |           +----+-----+         |(federated|
 +----+-----+                |               |  topo)   |
      |                      | UDS/DoIP      +----------+
      v                      v
 +---------+            +--------+
 |sovd-dfm |            |  ECU   |
 |(faults) |            +--------+
 +----+----+
      |
      v
 +---------+
 | sovd-db |  (SQLite via sqlx)
 +---------+
```

Also: the embedded **Fault Library** (out of scope, C shim on each ECU) pushes
faults into `sovd-dfm` via the `FaultSink` trait.

## Role definitions

### `sovd-client` (outbound requester)

- Sends SOVD REST requests over HTTPS.
- Used by off-board testers, on-board apps, cloud services, and **by
  `sovd-gateway` itself** when it needs to reach a downstream native-SOVD ECU.
- Handles client-side concerns: certificates, auth token injection, retries,
  deserialization into `sovd-interfaces` types.
- Deployment-agnostic: same crate runs inside a desktop tester and inside the
  gateway process.
- Corresponds to *SOVD Client* in design.md §In-scope components.

### `sovd-server` (one ECU's SOVD endpoint)

- Exposes the SOVD REST surface for **exactly one ECU or device view**.
- Implements the HTTP routes under `/sovd/v1/...` and the entity model
  (`components/{ecu}/faults`, `.../data`, `.../modes`, `.../operations`).
- Dispatches inward to local backends:
  - `sovd-dfm` for fault / DTC queries
  - MDD-backed DID provider for `data` reads (Phase 4)
  - Registered routine handlers for `operations` (Phase 4)
- Does **not** route across ECUs. Cross-ECU fan-out is `sovd-gateway`'s job.
- Corresponds to *SOVD Server* in design.md §In-scope components.

### `sovd-gateway` (system-wide multiplexer)

- Single system-wide entry point. Accepts SOVD requests and routes them to the
  right backend by `ComponentId`.
- Backends it knows how to route to:
  - an in-process `sovd-server` (native OpenSOVD ECU)
  - a `CDA` instance (legacy UDS ECU — adapter translates SOVD → UDS)
  - another `sovd-gateway` (federated multi-system topology)
  - `sovd-dfm` for system-wide fault queries
- Performs multi-ECU aggregation (e.g. "list DTCs across all components").
- Corresponds to *SOVD Gateway* in design.md §In-scope components.

### `sovd-dfm` (Diagnostic Fault Manager)

- Central per-system fault aggregator. One instance per system (not per ECU).
- Receives faults from **Fault Library** shims via the `FaultSink` trait
  (ADR-001 interface with S-CORE).
- Persists fault records through `sovd-db`.
- Implements debouncing, operation-cycle gating, DTC lifecycle.
- Is a `SovdBackend` from `sovd-gateway`'s perspective.
- Corresponds to *Diagnostic Fault Manager* in design.md §In-scope components.

### `sovd-db` (persistence)

- SQLite-backed store (`sqlx`) used **only by `sovd-dfm`**.
- Stores DTCs, FIDs, occurrence counts, meta-data, debounce thresholds.
- Not a SOVD backend on its own. Not reachable from the gateway directly.
- Corresponds to *Diagnostic DB* in design.md §In-scope components.

### `sovd-interfaces` (traits + DTOs)

- The contract crate. Defines:
  - types (`Dtc`, `DtcStatus`, `ComponentId`, `RoutineId`, `DataIdentifier`, ...)
  - errors (`SovdError`)
  - traits (`SovdServer`, `SovdGateway`, `SovdBackend`, `SovdClient`,
    `FaultSink`)
- **No runtime code.** Purely shapes.
- Depended on by every other crate in the workspace.

### `SovdBackend` (abstraction)

Not a crate — a trait in `sovd-interfaces`. It is the uniform shape that
`sovd-gateway` routes to. A backend can be backed by:

| Backend kind | Implemented by |
|--------------|----------------|
| `Dfm`        | `sovd-dfm`     |
| `NativeSovd` | `sovd-server`  |
| `Cda`        | adapter over upstream CDA crate |
| `Federated`  | another `sovd-gateway` reached via `sovd-client` |

This is the only place where "where does this request go?" is answered.

## Dependency direction (static)

```
sovd-interfaces  (leaf — no internal deps)
      ^
      |
  +---+------+---------+---------+--------+
  |          |         |         |        |
sovd-db  sovd-dfm  sovd-server sovd-client sovd-gateway
              ^         ^           ^          ^
              |         |           |          |
              +---- used by sovd-main binary ---+
```

- `sovd-gateway` may depend on `sovd-client` at runtime (for federated hops),
  but the compile-time edge is only added when that feature lands in Phase 4.
- No crate depends on `sovd-main` — it is the binary assembly point only.
- Nothing here depends on `classic-diagnostic-adapter`; CDA is wired in through
  the `SovdBackend` trait by an adapter shim in Phase 4.

## Non-goals for Phase 0

- No trait **implementations**. Phase 3/4 owns that.
- No HTTP routing for real SOVD entities. Phase 3 owns that.
- No wire format versioning. The `/sovd/v1/health` endpoint stays as the only
  live HTTP surface.
