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

//! Assert that the debug-only `GET /sovd/v1/openapi.json` endpoint serves
//! a parseable `OpenAPI` 3.x document and that every spec-derived Rust type
//! that we registered via `components(schemas(...))` appears under the
//! document's `components.schemas` object.
//!
//! The dev endpoint is only mounted in debug builds (`cfg(debug_assertions)`),
//! which matches how `cargo test` runs.

use std::sync::Arc;

use reqwest::StatusCode;
use sovd_server::{InMemoryServer, routes};
use tokio::net::TcpListener;

struct BootedServer {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl BootedServer {
    async fn start() -> Self {
        let server = Arc::new(InMemoryServer::new_with_demo_data());
        let app = routes::app_with_server(server);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind random port");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{addr}");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("server terminated unexpectedly");
        });
        Self { base_url, handle }
    }
}

impl Drop for BootedServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Types that are registered via `components(schemas(...))` in
/// `sovd-server/src/openapi.rs`. If this list drifts from what the derive
/// actually registers, the test fails. See the doc comment on `ApiDoc` for
/// why `Value`, `ListOfValues`, and `ReadValue` are currently excluded.
const EXPECTED_SCHEMAS: &[&str] = &[
    "DiscoveredEntities",
    "DiscoveredEntitiesWithSchema",
    "EntityCapabilities",
    "EntityReference",
    "Fault",
    "FaultDetails",
    "FaultFilter",
    "ListOfFaults",
    "OperationDescription",
    "OperationDetails",
    "OperationsList",
    "ExecutionStatus",
    "ExecutionStatusResponse",
    "ExecutionsList",
    "StartExecutionRequest",
    "StartExecutionAsyncResponse",
    "StartExecutionSyncResponse",
    "ApplyCapabilityRequest",
    "Capability",
    "ProximityChallenge",
    "Severity",
    "ValueMetadata",
    "DataCategoryInformation",
    "ValueGroup",
    "DataListEntry",
    "GenericError",
    "DataError",
];

#[tokio::test]
async fn openapi_endpoint_exposes_every_registered_schema() {
    let booted = BootedServer::start().await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/sovd/v1/openapi.json", booted.base_url))
        .send()
        .await
        .expect("GET openapi.json");
    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("parse openapi json");

    // 1. Must declare an OpenAPI 3.x document.
    let openapi_version = body
        .get("openapi")
        .and_then(serde_json::Value::as_str)
        .expect("openapi field");
    assert!(
        openapi_version.starts_with("3."),
        "unexpected OpenAPI version: {openapi_version}"
    );

    // 2. Must contain `components.schemas`.
    let schemas = body
        .get("components")
        .and_then(|c| c.get("schemas"))
        .and_then(serde_json::Value::as_object)
        .expect("components.schemas object");

    // 3. Every type we registered must be present.
    let mut missing: Vec<&str> = Vec::new();
    for expected in EXPECTED_SCHEMAS {
        if !schemas.contains_key(*expected) {
            missing.push(expected);
        }
    }
    assert!(
        missing.is_empty(),
        "schemas missing from generated OpenAPI: {missing:?}"
    );

    // 4. `paths` must contain the nine MVP endpoints.
    let paths = body
        .get("paths")
        .and_then(serde_json::Value::as_object)
        .expect("paths object");
    for expected_path in [
        "/sovd/v1/components",
        "/sovd/v1/components/{component_id}",
        "/sovd/v1/components/{component_id}/faults",
        "/sovd/v1/components/{component_id}/faults/{fault_code}",
        "/sovd/v1/components/{component_id}/operations",
        "/sovd/v1/components/{component_id}/operations/{operation_id}/executions",
        "/sovd/v1/components/{component_id}/operations/{operation_id}/executions/{execution_id}",
    ] {
        assert!(
            paths.contains_key(expected_path),
            "path missing from generated OpenAPI: {expected_path}"
        );
    }
}
