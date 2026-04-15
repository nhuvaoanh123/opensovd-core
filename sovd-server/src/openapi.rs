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

//! `OpenAPI` document generated from the `utoipa::path` annotations on every
//! route handler plus the `ToSchema` impls on the spec-derived DTOs.
//!
//! The generated document is served from a dev-only
//! `GET /sovd/v1/openapi.json` endpoint (see [`crate::routes::openapi_json`]),
//! gated behind `#[cfg(debug_assertions)]` so release builds never expose it.
//!
//! The set of `paths` and `components` declared here is the authoritative
//! snapshot of what the MVP server answers; tests in
//! `integration-tests/tests/openapi_roundtrip.rs` assert that every type
//! ported under `sovd_interfaces::spec` appears in `components.schemas`.

use utoipa::OpenApi;

use crate::routes::{components, data, faults, operations};

/// Assembled `OpenAPI` document. Derive-built so the doc is in sync with
/// the annotated handlers and `ToSchema` types at compile time.
#[derive(OpenApi)]
#[openapi(
    paths(
        components::list_components,
        components::get_component,
        faults::list_faults,
        faults::get_fault,
        faults::clear_all_faults,
        faults::clear_fault,
        operations::list_operations,
        operations::start_execution,
        operations::execution_status,
        data::list_data,
    ),
    components(schemas(
        sovd_interfaces::spec::component::DiscoveredEntities,
        sovd_interfaces::spec::component::DiscoveredEntitiesWithSchema,
        sovd_interfaces::spec::component::EntityCapabilities,
        sovd_interfaces::spec::component::EntityReference,
        sovd_interfaces::spec::fault::Fault,
        sovd_interfaces::spec::fault::FaultDetails,
        sovd_interfaces::spec::fault::FaultFilter,
        sovd_interfaces::spec::fault::ListOfFaults,
        sovd_interfaces::spec::operation::OperationDescription,
        sovd_interfaces::spec::operation::OperationDetails,
        sovd_interfaces::spec::operation::OperationsList,
        sovd_interfaces::spec::operation::ExecutionStatus,
        sovd_interfaces::spec::operation::ExecutionStatusResponse,
        sovd_interfaces::spec::operation::ExecutionsList,
        sovd_interfaces::spec::operation::StartExecutionRequest,
        sovd_interfaces::spec::operation::StartExecutionAsyncResponse,
        sovd_interfaces::spec::operation::StartExecutionSyncResponse,
        sovd_interfaces::spec::operation::ApplyCapabilityRequest,
        sovd_interfaces::spec::operation::Capability,
        sovd_interfaces::spec::operation::ProximityChallenge,
        sovd_interfaces::spec::data::Severity,
        sovd_interfaces::spec::data::ValueMetadata,
        sovd_interfaces::spec::data::DataCategoryInformation,
        sovd_interfaces::spec::data::ValueGroup,
        sovd_interfaces::spec::data::DataListEntry,
        sovd_interfaces::spec::data::Datas,
        // Phase 4 D3 — the three `Value`-family types now live under
        // the `Sovd*` Rust names (`SovdValue` / `SovdListOfValues` /
        // `SovdReadValue`) with `pub type Value = SovdValue;` legacy
        // aliases preserving the Phase 3 module path. The rename
        // sidesteps utoipa 5.4's "proc-macro derive produced
        // unparsable tokens" failure that Phase 3 documented.
        sovd_interfaces::spec::data::SovdValue,
        sovd_interfaces::spec::data::SovdListOfValues,
        sovd_interfaces::spec::data::SovdReadValue,
        sovd_interfaces::spec::error::GenericError,
        sovd_interfaces::spec::error::DataError,
        // Phase 4 D4 — extras health envelope.
        sovd_interfaces::extras::health::HealthStatus,
        sovd_interfaces::traits::backend::BackendHealth,
    )),
    tags(
        (name = "discovery", description = "Entity discovery endpoints"),
        (name = "fault-handling", description = "Fault list/detail/clear endpoints"),
        (name = "operations-control", description = "Operation execution endpoints"),
        (name = "data-access", description = "Data resource access endpoints"),
    )
)]
pub struct ApiDoc;

/// Build the generated [`utoipa::openapi::OpenApi`] document.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    <ApiDoc as OpenApi>::openapi()
}
