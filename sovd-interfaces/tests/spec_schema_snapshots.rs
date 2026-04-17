/*
 * SPDX-License-Identifier: Apache-2.0
 * SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)
 *
 * See the NOTICE file(s) distributed with this work for additional
 * information regarding copyright ownership.
 *
 * This program and the accompanying materials are made available under the
 * terms of the Apache License Version 2.0 which is available at
 * https://www.apache.org/licenses/LICENSE-2.0
 */

//! Snapshot tests for spec-derived DTOs.
//!
//! Each snapshot is a JSON file under `tests/snapshots/` containing the
//! `OpenAPI` schema that `utoipa::ToSchema` derives for one of our
//! `crate::spec::*` types. The first run (with `UPDATE_SNAPSHOTS=1`)
//! writes the file; every subsequent run compares the freshly generated
//! schema byte-for-byte against the on-disk snapshot. Drift between our
//! Rust types and the upstream ASAM SOVD v1.1.0-rc1 schema shapes shows
//! up as a `git diff` in `tests/snapshots/`, which is exactly what we
//! want from a "golden file" gate.
//!
//! To regenerate after an intentional change:
//!
//! ```sh
//! UPDATE_SNAPSHOTS=1 cargo test -p sovd-interfaces --test spec_schema_snapshots
//! ```
//!
//! See `docs/openapi-audit-2026-04-14.md` for provenance of each type.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use sovd_interfaces::spec::{
    component::{
        DiscoveredEntities, DiscoveredEntitiesWithSchema, EntityCapabilities, EntityCollection,
        EntityReference,
    },
    data::{
        DataCategoryInformation, DataListEntry, Datas, ListOfValues, ReadValue, Severity, Value,
        ValueGroup, ValueMetadata,
    },
    error::{DataError, GenericError},
    fault::{Fault, FaultDetails, FaultFilter, ListOfFaults},
    mode::{ControlStates, ModeCollectionItem, ModeDetails, SupportedModes},
    operation::{
        ApplyCapabilityRequest, Capability, ExecutionStatus, ExecutionStatusResponse,
        ExecutionsList, OperationDescription, OperationDetails, OperationsList, ProximityChallenge,
        StartExecutionAsyncResponse, StartExecutionRequest, StartExecutionSyncResponse,
    },
};
use utoipa::PartialSchema;

fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
}

fn check_snapshot<T: PartialSchema>(name: &str) {
    let schema = T::schema();
    let actual = serde_json::to_string_pretty(&schema)
        .expect("serialize schema to JSON")
        // Normalise line endings so the snapshot is identical across
        // git autocrlf settings on Windows checkouts.
        .replace("\r\n", "\n");

    let path = snapshots_dir().join(format!("{name}.json"));

    if env::var_os("UPDATE_SNAPSHOTS").is_some() || !path.exists() {
        fs::create_dir_all(snapshots_dir()).expect("create snapshots dir");
        fs::write(&path, format!("{actual}\n")).expect("write snapshot");
        return;
    }

    let path_display = path.display();
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read snapshot {path_display}: {e}"))
        .replace("\r\n", "\n");

    let expected_trimmed = expected.trim_end_matches('\n');
    let actual_trimmed = actual.trim_end_matches('\n');

    assert_eq!(
        expected_trimmed, actual_trimmed,
        "schema for {name} drifted from snapshot {path_display}; \
         re-run with UPDATE_SNAPSHOTS=1 to refresh after an intentional change"
    );
}

// ---- error ---------------------------------------------------------------

#[test]
fn snapshot_generic_error() {
    check_snapshot::<GenericError>("GenericError");
}

#[test]
fn snapshot_data_error() {
    check_snapshot::<DataError>("DataError");
}

// ---- fault ---------------------------------------------------------------

#[test]
fn snapshot_fault() {
    check_snapshot::<Fault>("Fault");
}

#[test]
fn snapshot_list_of_faults() {
    check_snapshot::<ListOfFaults>("ListOfFaults");
}

#[test]
fn snapshot_fault_details() {
    check_snapshot::<FaultDetails>("FaultDetails");
}

#[test]
fn snapshot_fault_filter() {
    check_snapshot::<FaultFilter>("FaultFilter");
}

// ---- component / discovery ----------------------------------------------

#[test]
fn snapshot_entity_collection() {
    check_snapshot::<EntityCollection>("EntityCollection");
}

#[test]
fn snapshot_entity_reference() {
    check_snapshot::<EntityReference>("EntityReference");
}

#[test]
fn snapshot_discovered_entities() {
    check_snapshot::<DiscoveredEntities>("DiscoveredEntities");
}

#[test]
fn snapshot_discovered_entities_with_schema() {
    check_snapshot::<DiscoveredEntitiesWithSchema>("DiscoveredEntitiesWithSchema");
}

#[test]
fn snapshot_entity_capabilities() {
    check_snapshot::<EntityCapabilities>("EntityCapabilities");
}

// ---- operation ----------------------------------------------------------

#[test]
fn snapshot_operation_description() {
    check_snapshot::<OperationDescription>("OperationDescription");
}

#[test]
fn snapshot_execution_status() {
    check_snapshot::<ExecutionStatus>("ExecutionStatus");
}

#[test]
fn snapshot_capability() {
    check_snapshot::<Capability>("Capability");
}

#[test]
fn snapshot_operations_list() {
    check_snapshot::<OperationsList>("OperationsList");
}

#[test]
fn snapshot_operation_details() {
    check_snapshot::<OperationDetails>("OperationDetails");
}

#[test]
fn snapshot_start_execution_request() {
    check_snapshot::<StartExecutionRequest>("StartExecutionRequest");
}

#[test]
fn snapshot_apply_capability_request() {
    check_snapshot::<ApplyCapabilityRequest>("ApplyCapabilityRequest");
}

#[test]
fn snapshot_start_execution_async_response() {
    check_snapshot::<StartExecutionAsyncResponse>("StartExecutionAsyncResponse");
}

#[test]
fn snapshot_start_execution_sync_response() {
    check_snapshot::<StartExecutionSyncResponse>("StartExecutionSyncResponse");
}

#[test]
fn snapshot_execution_status_response() {
    check_snapshot::<ExecutionStatusResponse>("ExecutionStatusResponse");
}

#[test]
fn snapshot_executions_list() {
    check_snapshot::<ExecutionsList>("ExecutionsList");
}

#[test]
fn snapshot_proximity_challenge() {
    check_snapshot::<ProximityChallenge>("ProximityChallenge");
}

// ---- data ---------------------------------------------------------------

#[test]
fn snapshot_severity() {
    check_snapshot::<Severity>("Severity");
}

#[test]
fn snapshot_value_metadata() {
    check_snapshot::<ValueMetadata>("ValueMetadata");
}

#[test]
fn snapshot_value() {
    check_snapshot::<Value>("Value");
}

#[test]
fn snapshot_list_of_values() {
    check_snapshot::<ListOfValues>("ListOfValues");
}

#[test]
fn snapshot_read_value() {
    check_snapshot::<ReadValue>("ReadValue");
}

#[test]
fn snapshot_data_category_information() {
    check_snapshot::<DataCategoryInformation>("DataCategoryInformation");
}

#[test]
fn snapshot_value_group() {
    check_snapshot::<ValueGroup>("ValueGroup");
}

#[test]
fn snapshot_data_list_entry() {
    check_snapshot::<DataListEntry>("DataListEntry");
}

#[test]
fn snapshot_datas() {
    check_snapshot::<Datas>("Datas");
}

// ---- mode ---------------------------------------------------------------

#[test]
fn snapshot_mode_collection_item() {
    check_snapshot::<ModeCollectionItem>("ModeCollectionItem");
}

#[test]
fn snapshot_supported_modes() {
    check_snapshot::<SupportedModes>("SupportedModes");
}

#[test]
fn snapshot_mode_details() {
    check_snapshot::<ModeDetails>("ModeDetails");
}

#[test]
fn snapshot_control_states() {
    check_snapshot::<ControlStates>("ControlStates");
}
