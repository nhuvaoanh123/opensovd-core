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

//! End-to-end flow test against the in-memory SOVD server.
//!
//! Starts the full axum router on a random loopback port via
//! `TcpListener::bind("127.0.0.1:0")`, fires real HTTP requests with
//! `reqwest`, and asserts that every response deserializes back into the
//! spec-derived Rust type it was supposed to return. This proves the full
//! type-safe request/response path works end-to-end without any mocking
//! of either axum or the in-memory store.

use std::sync::Arc;

use reqwest::StatusCode;
use sovd_interfaces::spec::{
    component::DiscoveredEntities,
    fault::ListOfFaults,
    operation::{
        ExecutionStatus, ExecutionStatusResponse, StartExecutionAsyncResponse,
        StartExecutionRequest,
    },
};
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

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

impl Drop for BootedServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[tokio::test]
async fn in_memory_mvp_flow_round_trips_spec_types() {
    let booted = BootedServer::start().await;
    let client = reqwest::Client::new();

    // 1. GET /sovd/v1/components -> DiscoveredEntities, 3 demo components.
    let response = client
        .get(booted.url("/sovd/v1/components"))
        .send()
        .await
        .expect("GET components");
    assert_eq!(response.status(), StatusCode::OK);
    let entities: DiscoveredEntities = response.json().await.expect("parse DiscoveredEntities");
    let ids: Vec<String> = entities.items.iter().map(|e| e.id.clone()).collect();
    // list_entities returns in alphabetical order.
    assert_eq!(
        ids,
        vec!["bcm".to_string(), "cvc".to_string(), "sc".to_string()]
    );

    // 2. GET /sovd/v1/components/cvc/faults -> ListOfFaults with 2 items.
    let response = client
        .get(booted.url("/sovd/v1/components/cvc/faults"))
        .send()
        .await
        .expect("GET cvc faults");
    assert_eq!(response.status(), StatusCode::OK);
    let faults: ListOfFaults = response.json().await.expect("parse ListOfFaults");
    assert_eq!(faults.items.len(), 2);
    assert!(faults.items.iter().any(|f| f.code == "P0A1F"));

    // 3. POST .../executions with a StartExecutionRequest body -> 202
    //    StartExecutionAsyncResponse.
    let start_body = StartExecutionRequest {
        timeout: Some(30),
        parameters: Some(serde_json::json!({"mode": "quick"})),
        proximity_response: None,
    };
    let response = client
        .post(booted.url("/sovd/v1/components/cvc/operations/motor_self_test/executions"))
        .json(&start_body)
        .send()
        .await
        .expect("POST start execution");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let started: StartExecutionAsyncResponse = response
        .json()
        .await
        .expect("parse StartExecutionAsyncResponse");
    assert_eq!(started.status, Some(ExecutionStatus::Running));
    assert!(!started.id.is_empty());

    // 4. GET .../executions/{id} -> ExecutionStatusResponse (running).
    let exec_url = booted.url(&format!(
        "/sovd/v1/components/cvc/operations/motor_self_test/executions/{}",
        started.id
    ));
    let response = client.get(&exec_url).send().await.expect("GET exec status");
    assert_eq!(response.status(), StatusCode::OK);
    let status: ExecutionStatusResponse = response
        .json()
        .await
        .expect("parse ExecutionStatusResponse");
    assert_eq!(status.status, Some(ExecutionStatus::Running));

    // 5. DELETE /sovd/v1/components/cvc/faults -> 204.
    let response = client
        .delete(booted.url("/sovd/v1/components/cvc/faults"))
        .send()
        .await
        .expect("DELETE all faults");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Verify the fault list is now empty.
    let response = client
        .get(booted.url("/sovd/v1/components/cvc/faults"))
        .send()
        .await
        .expect("GET cvc faults after clear");
    assert_eq!(response.status(), StatusCode::OK);
    let faults: ListOfFaults = response.json().await.expect("parse ListOfFaults");
    assert!(faults.items.is_empty());

    // 6. GET /sovd/v1/health still works on the same router.
    let response = client
        .get(booted.url("/sovd/v1/health"))
        .send()
        .await
        .expect("GET health");
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("parse health");
    assert_eq!(body.get("status").and_then(|v| v.as_str()), Some("ok"));
}
