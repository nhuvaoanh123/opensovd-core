<!--
SPDX-License-Identifier: Apache-2.0
SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)
-->

# ISO 17978-3 OpenAPI Audit (2026-04-14)

This document audits the ASAM SOVD v1.1.0-rc1 OpenAPI YAML template that was
ingested by the SOVD acquisition agent on 2026-04-14, and maps the on-disk
contents to the five MVP use cases defined in
[`H:\taktflow-opensovd\docs\REQUIREMENTS.md`](../../docs/REQUIREMENTS.md).

It is the input for Deliverable 2 of Line A (porting spec-derived DTOs to
`sovd-interfaces`). No Rust code has been written against the spec until this
audit is in place.

## 1. Source location

- Root directory:
  `H:\taktflow-opensovd\external\asam-public\ISO_17978-3_openapi\openapi-specification-1.1.0-rc1\`
- Top-level entry point:
  `sovd-api.yaml`
- Source archive sidecar:
  `H:\taktflow-opensovd\external\asam-public\ISO_17978-3_openapi.source.txt`
- Origin URL (per sidecar):
  `https://standards.iso.org/iso/17978/-3/ed-1/en/ISO%2017978-3%20ed.1%20-%20openapi-specification-1.1.0-rc1.zip`
- Downloaded: 2026-04-14
- Spec version: ASAM SOVD API v1.1.0 (rc1 OpenAPI template)

## 2. OpenAPI version

OpenAPI **3.1.0** for every YAML in the tree (verified by inspection of
`sovd-api.yaml` line 1 and a representative sample of resource files).
The top-level document declares a custom JSON Schema dialect:

```yaml
jsonSchemaDialect: https://asam.net/standards/diagnostics/sovd/v1.1/dialect
```

Because the spec is OpenAPI 3.1, schemas may use full JSON Schema 2020-12
constructs (`anyOf`, `oneOf`, `$ref` to external files, etc.). One
notable use is `commons/types.yaml#OpenApiSchema` which `$ref`s the
official OpenAPI 3.1.1 meta-schema:

```
$ref: https://raw.githubusercontent.com/OAI/OpenAPI-Specification/refs/tags/3.1.1/schemas/v3.1/schema.yaml#/$defs/schema
```

We treat this as an opaque JSON value (`serde_json::Value`) — there is no
need to model the entire OpenAPI meta-schema in Rust.

## 3. Modular structure

The spec is split into one top-level entry point (`sovd-api.yaml`) plus
21 resource subdirectories. Every path in `sovd-api.yaml` is a `$ref` to a
resource yaml in one of these subdirectories. The structure is:

| Subdirectory                | Purpose                                                                  |
| --------------------------- | ------------------------------------------------------------------------ |
| `authentication/`           | OAuth-style auth flow                                                    |
| `bulkdata/`                 | Bulk-data resource (large blobs)                                         |
| `capability-description/`   | Per-entity OpenAPI capability advertisement                              |
| `clear-data/`               | Clear cached / learned / client-defined data                             |
| `commons/`                  | Shared parameters, errors, types, responses                              |
| `communication-logs/`       | UDS communication trace logs                                             |
| `configurations/`           | Persistent configurations resource                                       |
| `cyclic-subscriptions/`     | Server-Sent-Events cyclic value subscriptions                            |
| `data/`                     | Data values / data-lists / data-groups / data-categories (VINs, voltages, etc.) |
| `discovery/`                | Entity discovery (areas, components, apps, functions)                    |
| `faults/`                   | **Fault entries (the SOVD term for DTCs)**                               |
| `locks/`                    | Resource locking                                                         |
| `logs/`                     | Application log entries                                                  |
| `meta-schema/`              | JSON Schema dialect declaration                                          |
| `modes/`                    | Modes (sessions / security access on classic ECUs)                       |
| `operations/`               | **Operations (UDS routine equivalent)**                                  |
| `restarting/`               | Start / restart / shutdown / force-restart                               |
| `scripts/`                  | Script execution                                                         |
| `software-updates/`         | OTA update package handling                                              |
| `triggers/`                 | Event triggers                                                           |

Each subdirectory typically contains:

- `<name>.yaml`        — paths
- `parameters.yaml`    — query / path parameters
- `requests.yaml`      — request bodies (where applicable)
- `responses.yaml`     — response media types
- `types.yaml`         — schemas

## 4. Endpoint count

Total HTTP method declarations (counted by grepping `get|post|put|delete|patch:`
across the resource yamls): **94 method-level entries** across **18** path
files.

Top-level path table in `sovd-api.yaml` declares **51 distinct path templates**
(many of which expose multiple methods).

## 5. Path catalogue (by resource)

The full path-by-method catalogue is included below. Methods are listed in
the order they appear in each resource yaml. `operationId` is shown when the
spec provides one (the spec is inconsistent — most paths have no
`operationId`, only a `description` and a `tags` entry).

### 5.1 discovery/discovery.yaml (6 methods)

| Path                                                | Method | operationId | Tag       | Summary                                              |
| --------------------------------------------------- | ------ | ----------- | --------- | ---------------------------------------------------- |
| `/{entity-collection}`                              | GET    | —           | discovery | List contained entities for an entity collection    |
| `/areas/{area-id}/subareas`                         | GET    | —           | discovery | Sub-entities for an area                            |
| `/components/{component-id}/subcomponents`          | GET    | —           | discovery | Sub-entities for a component                        |
| `/areas/{area-id}/related-components`               | GET    | —           | discovery | Related components of an area                      |
| `/components/{component-id}/related-apps`           | GET    | —           | discovery | Related apps for a component                       |
| `/{entity-collection}/{entity-id}`                  | GET    | —           | discovery | Entity capabilities (the per-component summary)     |

### 5.2 faults/faults.yaml (4 methods)

| Path                                                  | Method | operationId       | Tag            | Summary                                          |
| ----------------------------------------------------- | ------ | ----------------- | -------------- | ------------------------------------------------ |
| `/{entity-collection}/{entity-id}/faults`             | GET    | `getFaults`       | fault-handling | List fault entries (filterable by status/severity) |
| `/{entity-collection}/{entity-id}/faults`             | DELETE | `deleteAllFaults` | fault-handling | Delete all fault entries for the entity         |
| `/{entity-collection}/{entity-id}/faults/{fault-code}`| GET    | `getFaultById`    | fault-handling | Details for one fault                            |
| `/{entity-collection}/{entity-id}/faults/{fault-code}`| DELETE | `deleteFaultById` | fault-handling | Delete one fault entry                          |

### 5.3 operations/operations.yaml (7 methods)

| Path                                                                              | Method | Tag                | Summary                                       |
| --------------------------------------------------------------------------------- | ------ | ------------------ | --------------------------------------------- |
| `/{entity-collection}/{entity-id}/operations`                                     | GET    | operations-control | List available operations                     |
| `/{entity-collection}/{entity-id}/operations/{operation-id}`                      | GET    | operations-control | Details for one operation                     |
| `/{entity-collection}/{entity-id}/operations/{operation-id}/executions`           | POST   | operations-control | Start execution (sync 200 / async 202)        |
| `/{entity-collection}/{entity-id}/operations/{operation-id}/executions`           | GET    | operations-control | List currently existing executions            |
| `/{entity-collection}/{entity-id}/operations/{operation-id}/executions/{execution-id}` | GET    | operations-control | Status of one execution                       |
| `/{entity-collection}/{entity-id}/operations/{operation-id}/executions/{execution-id}` | DELETE | operations-control | Terminate execution                          |
| `/{entity-collection}/{entity-id}/operations/{operation-id}/executions/{execution-id}` | PUT    | operations-control | Apply capability (execute / freeze / reset)   |

### 5.4 data/data.yaml (9 methods)

| Path                                                                  | Method | Tag            | Summary                                |
| --------------------------------------------------------------------- | ------ | -------------- | -------------------------------------- |
| `/{entity-collection}/{entity-id}/data-categories`                    | GET    | data-retrieval | List data categories                   |
| `/{entity-collection}/{entity-id}/data-groups`                        | GET    | data-retrieval | List data groups                       |
| `/{entity-collection}/{entity-id}/data`                               | GET    | data-retrieval | List data resources                    |
| `/{entity-collection}/{entity-id}/data/{data-id}`                     | GET    | data-retrieval | Read one data resource (the "DID read") |
| `/{entity-collection}/{entity-id}/data/{data-id}`                     | PUT    | data-retrieval | Write one data resource                |
| `/{entity-collection}/{entity-id}/data-lists`                         | GET    | data-retrieval | List data-lists                        |
| `/{entity-collection}/{entity-id}/data-lists`                         | POST   | data-retrieval | Create a temporary data-list           |
| `/{entity-collection}/{entity-id}/data-lists/{data-list-id}`          | GET    | data-retrieval | Read a data-list                       |
| `/{entity-collection}/{entity-id}/data-lists/{data-list-id}`          | DELETE | data-retrieval | Remove a temporary data-list           |

### 5.5 clear-data/clear-data.yaml (5 methods)

| Path                                                              | Method | Tag        | Summary                                                         |
| ----------------------------------------------------------------- | ------ | ---------- | --------------------------------------------------------------- |
| `/{entity-collection}/{entity-id}/clear-data`                     | GET    | clear-data | List supported clear-data types for the entity                  |
| `/{entity-collection}/{entity-id}/clear-data/cached-data`         | PUT    | clear-data | Trigger clearing of cached data (async)                         |
| `/{entity-collection}/{entity-id}/clear-data/learned-data`        | PUT    | clear-data | Trigger clearing of learned data (async)                        |
| `/{entity-collection}/{entity-id}/clear-data/client-defined-resources` | PUT | clear-data | Trigger clearing of client-defined resources (async)            |
| `/{entity-collection}/{entity-id}/clear-data/status`              | GET    | clear-data | Status of ongoing / last completed clear-data task              |

### 5.6 Other resources (counts only — these are NOT in the MVP scope)

| Resource yaml                                          | Method count |
| ------------------------------------------------------ | -----------: |
| `authentication/authentication.yaml`                   |            2 |
| `bulkdata/bulkdata.yaml`                               |            6 |
| `capability-description/capability-description.yaml`   |            1 |
| `communication-logs/communication-logs.yaml`           |            5 |
| `configurations/configurations.yaml`                   |            3 |
| `cyclic-subscriptions/cyclic-subscriptions.yaml`       |            5 |
| `logs/logs.yaml`                                       |            5 |
| `locks/locks.yaml`                                     |            5 |
| `modes/modes.yaml`                                     |            3 |
| `restarting/restarting.yaml`                           |            6 |
| `scripts/scripts.yaml`                                 |            9 |
| `software-updates/software-updates.yaml`               |            8 |
| `triggers/triggers.yaml`                               |            5 |

## 6. Schema catalogue

There is no single `components/schemas` block — every resource subdirectory
defines its own `types.yaml`. Counts (top-level `^    Name:` schemas):

### 6.1 commons/types.yaml (14 schemas)

| Name               | One-line description                                                       |
| ------------------ | -------------------------------------------------------------------------- |
| `AnyValue`         | `anyOf` over string/number/integer/boolean/array/object — open value type  |
| `SupportedTags`    | `array<string>` — tags attached to entity / resource                       |
| `ProtocolType`     | enum: `sse` only (default for SOVD 1.1)                                    |
| `OpenApiSchema`    | `$ref` to OpenAPI 3.1.1 meta-schema (treat as opaque JSON)                 |
| `DataError`        | `{ path: json-pointer, error: GenericError }`                              |
| `EntityReference`  | `{ id, name, translation_id?, href, tags? }`                               |
| `DataCategory`     | URL-safe string pattern — predefined or `x-<oem>` extensions               |
| `Value`            | `{ id, data, metadata?, error? }` — read-result envelope                   |
| `ListOfValues`     | `{ items: Value[] }`                                                       |
| `ValueMetadata`    | `{ id, name, translation_id?, category, groups?, tags? }`                  |
| `ReadValue`        | `{ id, data, errors?, schema? }` — single-data read-back                   |
| `EventEnvelope`    | `{ timestamp, payload?, error? }` — SSE wrapper                            |
| `ProximityChallenge` | `{ challenge, valid_until }` — anti-replay for sensitive ops             |
| `Severity`         | enum: `fatal`, `error`, `warn`, `info`, `debug`                            |

### 6.2 commons/errors.yaml (2 schemas)

| Name           | One-line description                                                       |
| -------------- | -------------------------------------------------------------------------- |
| `GenericError` | `{ error_code, vendor_code?, message, translation_id?, parameters? }` — SOVD error envelope |
| `DataError`    | `{ path: json-pointer, error?: GenericError }` — partial-error wrapper     |

### 6.3 faults/types.yaml (2 schemas)

| Name           | One-line description                                                                          |
| -------------- | --------------------------------------------------------------------------------------------- |
| `ListOfFaults` | `{ items: Fault[], schema? }`                                                                 |
| `Fault`        | `{ code, scope?, display_code?, fault_name, fault_translation_id?, severity?, status?, symptom?, symptom_translation_id?, tags? }` |

The fault `status` field is an open `object` of OEM-specific key/value pairs;
the spec gives a UDS DTC-status-byte example with keys `testFailed`,
`testFailedThisOperationCycle`, `pendingDTC`, `confirmedDTC`,
`testNotCompletedSinceLastClear`, `testFailedSinceLastClear`,
`testNotCompletedThisOperationCycle`, `warningIndicatorRequested`,
`aggregatedStatus`. We model it as `serde_json::Value` per existing
`sovd-interfaces` conventions.

### 6.4 operations/types.yaml (3 schemas)

| Name                   | One-line description                                                                                            |
| ---------------------- | --------------------------------------------------------------------------------------------------------------- |
| `OperationDescription` | `{ id, name?, translation_id?, proximity_proof_required, asynchronous_execution, tags? }`                       |
| `ExecutionStatus`      | enum: `running`, `completed`, `failed`                                                                          |
| `Capability`           | enum: `execute`, `stop`, `freeze`, `reset`, `status`                                                            |

### 6.5 data/types.yaml (3 schemas)

| Name                      | One-line description                                                            |
| ------------------------- | ------------------------------------------------------------------------------- |
| `DataCategoryInformation` | `{ item: DataCategory, category_translation_id? }`                              |
| `ValueGroup`              | `{ id, category, category_translation_id?, group?, group_translation_id?, tags? }` |
| `DataListEntry`           | `{ id, tags?, items: ValueMetadata[] }`                                         |

### 6.6 clear-data/types.yaml (2 schemas)

| Name              | One-line description                                          |
| ----------------- | ------------------------------------------------------------- |
| `ClearDataStatus` | enum: `running`, `completed`, `failed`, `notRequested`        |
| `ClearDataType`   | enum: `cached-data`, `learned-data`, `client-defined-resources` |

### 6.7 Schemas in the rest of the spec

The remaining 18 resource yamls collectively define schemas for software
updates, scripts, triggers, cyclic subscriptions, logs, modes, configurations,
bulkdata, communication-logs, capability descriptions, locks, and
authentication. **None of these are in the Phase-3/4 MVP scope** so they are
not catalogued individually here; this audit focuses on what `sovd-interfaces`
needs first. Total schemas spec-wide (rough): **~188 top-level named schemas**
across all `types.yaml` / `parameters.yaml` / `requests.yaml` / `responses.yaml`
files, of which only the ~26 listed in §6.1–6.6 are MVP-relevant.

## 7. MVP use-case mapping

Use cases are taken from
[`H:\taktflow-opensovd\MASTER-PLAN.md`](../../MASTER-PLAN.md) §2.2 and refined
in [`H:\taktflow-opensovd\docs\REQUIREMENTS.md`](../../docs/REQUIREMENTS.md)
sections 3.1–3.3.

| UC  | Title                          | Spec coverage                                                                                                            | Notes                                                                                                          |
| --- | ------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| UC1 | Read DTCs                      | `GET /{entity-collection}/{entity-id}/faults` + `GET /{entity-collection}/{entity-id}/faults/{fault-code}`               | SOVD calls them "faults", not "DTCs". Schemas: `ListOfFaults`, `Fault`.                                        |
| UC2 | Report fault via Fault API     | **Not in spec.** Internal IPC path only (Fault Library shim → DFM).                                                      | Lives in `extras/fault.rs` (already in `types/fault.rs`, will move to `extras/`).                              |
| UC3 | Clear DTCs                     | `DELETE /{entity-collection}/{entity-id}/faults` (clear all) + `DELETE .../faults/{fault-code}` (clear one)              | The spec uses HTTP `DELETE`, not a `POST .../clear` action. Our existing `DtcGroup::All` semantic still applies but our hand-drafted `clear_dtcs(group)` does not match — see §9. |
| UC4 | Read UDS ECU metadata (via CDA)| `GET /{entity-collection}/{entity-id}` (entity capabilities) + `GET /data/{data-id}` for DIDs                            | Entity discovery returns `EntityReference` collection; entity detail returns the `discoveredEntityCapabilities` shape (no named schema in spec — inline in responses.yaml). |
| UC5 | Trigger diagnostic routine     | `POST /{entity-collection}/{entity-id}/operations/{operation-id}/executions`                                              | SOVD calls them "operations". Status polling is via `GET .../executions/{execution-id}`.                       |

### 7.1 Gap analysis

| Gap                                         | Severity | Mitigation                                                                                          |
| ------------------------------------------- | -------- | --------------------------------------------------------------------------------------------------- |
| UC2 (fault ingest) is not in the OpenAPI    | **None.** Expected — fault ingest is internal IPC, not REST. | Keep `FaultRecord` / `FaultSeverity` in `extras/fault.rs` per ADR-0006 `extras` convention.                            |
| UC3 — spec uses `DELETE`, not `POST .../clear` | Medium  | Trait method `clear_dtcs` keeps its name but documents that the wire mapping is `DELETE`. Group-filtered clear (`DtcGroup::Group`) is **not in the spec** — we either drop the `Group` variant or document it as a Taktflow extra.    |
| UC3 — `DTCStatusMask` is not in the spec    | Medium  | Spec uses `status[key]` query param (string match against status keys) and `severity` integer filter. We re-shape `DtcStatusMask` as Taktflow extra OR replace it with a status-key filter struct. |
| UC4 — `discoveredEntityCapabilities` has no named schema | Low | Define our `EntityCapabilities` struct as a Taktflow-named port of the inline schema; reference inline location in the doc comment. |
| `Fault.code` is **a string**, not an integer | Medium  | Existing hand-drafted `DtcId(pub u32)` is wrong for the spec; spec uses native string codes (e.g. `"0012E3"`, `"P102"`, `"modelMissing"`). Spec-derived `Fault.code` is `String`. |

Translation: our existing hand-drafted `DtcId(u32)` / `DtcGroup` / `DtcStatusMask`
**all** disagree with the spec in non-trivial ways. The spec is the truth;
the hand-drafted shapes are gone after Deliverable 2.

## 8. License

The OpenAPI YAML carries this license header on every file:

```
© by ASAM e.V., 2025
This file is informative. The normative REST API definition is published in the specification.
Any use is limited to the scope described in the ASAM license terms.
See http://www.asam.net/license.html for further details.
```

The archive itself is distributed via the ISO Standards Maintenance Portal
(`standards.iso.org`), subject to the ISO Customer Licence Agreement.

**Per the source.txt sidecar:**

> Treat as ASAM-copyrighted reference material: use for internal validation
> against our implementation, but do NOT redistribute, do NOT vendor the YAMLs
> into shipped source, and do NOT generate Apache-2.0-licensed code from them
> without legal review (per ASAM license terms).

### Operational consequences for `sovd-interfaces`

1. The YAMLs are **not** vendored into `opensovd-core`. They live in the
   sibling read-only tree `H:\taktflow-opensovd\external\asam-public\…` and
   are not published in any release artifact.
2. We port **schema shapes and field names** (which are facts about the wire
   protocol) into Apache-2.0 Rust types. We **do not** copy spec descriptions
   verbatim into doc comments — instead, our doc comments paraphrase what the
   field is for and point to the upstream YAML name and file path.
3. Each `spec/*.rs` file declares its provenance in a header comment of the
   form:

   ```
   //! Provenance: ISO 17978-3 SOVD OpenAPI v1.1.0-rc1 — `<file>#<schema>`
   //! Spec license: © ASAM e.V. 2025 (https://www.asam.net/license/)
   ```

4. `spec/mod.rs` carries the same license note plus a clean-room statement.
5. Snapshot files under `tests/snapshots/` contain JSON Schemas generated by
   our own code, not the upstream YAML — those are clean Apache-2.0.

## 9. Implications for Deliverable 2 (port plan)

| Type to port                | Spec source                                          | Notes for Rust port                                                                                                  |
| --------------------------- | ---------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `Fault`                     | `faults/types.yaml#Fault`                            | `code: String`, `severity: Option<i32>`, `status: Option<serde_json::Value>`. Drop `DtcId(u32)`.                     |
| `ListOfFaults`              | `faults/types.yaml#ListOfFaults`                     | `items: Vec<Fault>`, `schema: Option<serde_json::Value>` (opaque `OpenApiSchema`).                                  |
| `FaultDetails`              | `faults/responses.yaml#FaultDetails` (inline schema) | `item: Fault`, `environment_data?`, `errors?`, `schema?`. Name it `FaultDetails`.                                    |
| `OperationDescription`      | `operations/types.yaml#OperationDescription`         | Straight port; the `tags` field flattens into a `Vec<String>`.                                                       |
| `ExecutionStatus`           | `operations/types.yaml#ExecutionStatus`              | Tagged enum: `Running`, `Completed`, `Failed`. Serde `rename_all = "lowercase"`.                                    |
| `Capability`                | `operations/types.yaml#Capability`                   | Same shape: `Execute`, `Stop`, `Freeze`, `Reset`, `Status`.                                                          |
| `StartExecutionRequest`     | `operations/requests.yaml#StartExecution`            | `{ timeout?, parameters?, proximity_response? }`. `parameters` = `serde_json::Value`.                                |
| `StartExecutionAsyncResponse` | `operations/responses.yaml#StartExecutionAsynchronous` | `{ id, status?: ExecutionStatus }`.                                                                                |
| `GenericError`              | `commons/errors.yaml#GenericError`                   | Replace existing `SovdError` HTTP-mapping. `SovdError` (Rust enum) stays, but the wire form is `GenericError`.       |
| `DataError`                 | `commons/errors.yaml#DataError`                      | `{ path: String, error: Option<GenericError> }`.                                                                     |
| `EntityReference`           | `commons/types.yaml#EntityReference`                 | Component / app / area / function listing.                                                                           |
| `EntityCapabilities`        | `discovery/responses.yaml#discoveredEntityCapabilities` (inline) | Component detail. We name it because the spec doesn't.                                                |
| `Value`, `ValueMetadata`, `ReadValue` | `commons/types.yaml`                       | DID read result shape.                                                                                               |
| `Severity`                  | `commons/types.yaml#Severity`                        | Enum (`fatal`/`error`/`warn`/`info`/`debug`). Distinct from per-fault `severity` integer.                            |
| `ClearDataStatus`           | `clear-data/types.yaml#ClearDataStatus`              | `Running`, `Completed`, `Failed`, `NotRequested`.                                                                    |

Trait reconciliation — see `traits/server.rs::SovdServer::list_dtcs`. After
Deliverable 2 it returns `Result<spec::ListOfFaults, SovdError>` and takes a
status-key filter struct (TBD shape — exact named filter type defined when
ported).

## 10. OpenAPI 3.1 quirks observed

- `commons/types.yaml#AnyValue` uses `anyOf` over six primitive types. We
  port this to `serde_json::Value` (no Rust enum needed — `Value` is open).
- `commons/types.yaml#OpenApiSchema` uses an external `$ref` to the
  OpenAPI 3.1.1 meta-schema. We port as `Option<serde_json::Value>`.
- The discovery `discoveredEntityCapabilities` is **inline** with no
  schema name. We invent the name `EntityCapabilities` for it.
- The fault `status` field is an open `object`. We port as
  `Option<serde_json::Value>`. The example values document common UDS DTC
  status-byte keys but the spec does not constrain them.
- `Fault.severity` is `integer` with the convention `1=FATAL, 2=ERROR,
  3=WARN, 4=INFO` documented in prose (not enforced by schema). We port as
  `Option<i32>` and provide a Taktflow helper enum in `extras/`.

## 11. Risks and open questions

1. **Filter semantics drift.** The hand-drafted `DtcStatusMask(u8)` is a
   bit-mask; the spec's `status[key]` query param is a multi-string match.
   These are semantically incompatible. Open question: does the trait method
   `list_dtcs` take a Taktflow-friendly `DtcStatusMask` (extras) or a
   spec-faithful `FaultFilter { status_keys: Vec<(String, String)>, severity:
   Option<i32>, scope: Option<String> }`? **Decision for D2:** spec-faithful
   `FaultFilter`; document the bit-mask helper as an `extras/` convenience.
2. **Clear semantics drift.** The spec exposes only "clear all" (`DELETE
   .../faults`) and "clear one" (`DELETE .../faults/{code}`). The hand-drafted
   `DtcGroup::Group(u32)` (clear by group code) **is not in the spec**. It is
   plausibly a Taktflow extension to bridge UDS `0x14 ClearDiagnosticInformation
   group=u24`. **Decision for D2:** drop `DtcGroup` from the spec module; if
   later proven needed, resurrect as `extras::FaultClearByGroup`.
3. **OpenAPI snapshot tests.** Without `utoipa` in the workspace, we
   cannot regenerate OpenAPI from Rust. **Decision for D2:** add `utoipa`
   to the workspace, then write JSON snapshot files under `tests/snapshots/`.
   If `utoipa` brings in too many new transitive deps, fall back to plain
   round-trip JSON tests with handwritten fixtures.
4. **`Fault.code` as string.** Every existing trait that takes `DtcId(u32)`
   needs to switch to `&str` / `String`. This is a non-trivial refactor; it
   is not a redesign but it touches every method on `SovdServer`,
   `SovdBackend`, `SovdClient`, and the `sovd-server`/`sovd-gateway` skeletons
   that mention DTCs. Acceptable for D3 but may take more commits than UC1
   alone.

## 12. Conclusion

The OpenAPI spec is present, complete, and well-structured. All five MVP
use cases (minus UC2, which is internal-only by design) are covered by named
endpoints and named schemas. The hand-drafted `sovd-interfaces` types in
`types/dtc.rs` and friends are wrong on multiple axes (string-vs-integer
codes, mask-vs-key-filter, presence of group-clear) and are dropped in
Deliverable 2 in favour of spec-derived ports under `sovd-interfaces/src/spec/`.

License-wise, we are clear-room compliant: no YAML text is vendored, no
descriptions are copied verbatim, and field/schema names are facts about
the wire protocol, not authored prose.
