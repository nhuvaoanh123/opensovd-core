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

//! Phase 5 Line A - local hybrid alias routing proof.
//!
//! This is the repo-side end-to-end proof for the new Phase 5 topology
//! (3-ECU bench, ADR-0023):
//!
//! ```text
//!   client -> OpenSOVD frontend
//!              |- local demo component: bcm
//!              `- CDA forwards: cvc, sc
//!                                |
//!                                `-> downstream CDA ids:
//!                                    cvc00000, sc00000
//! ```
//!
//! The test stands up a mock CDA under `/vehicle/v15`, registers
//! aliased `CdaBackend`s for `cvc` and `sc`, and verifies two important
//! guarantees:
//!
//! 1. `OpenSOVD` still exposes the external Taktflow ids
//!    (`cvc/sc/bcm`) to clients.
//! 2. The forwarded HTTP traffic actually targets the downstream alias
//!    ids (`cvc00000/...`) instead of the external ids.

use std::{
    sync::{Arc, Mutex},
    vec,
};

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use reqwest::StatusCode;
use sovd_interfaces::{
    ComponentId,
    spec::{
        component::{DiscoveredEntities, EntityCapabilities},
        fault::{Fault, FaultFilter, ListOfFaults},
    },
};
use sovd_server::{CdaBackend, InMemoryServer, routes};
use tokio::net::TcpListener;
use url::Url;

#[derive(Clone, Debug, Default)]
struct MockCdaState {
    seen_paths: Arc<Mutex<Vec<String>>>,
}

impl MockCdaState {
    fn record(&self, path: String) {
        self.seen_paths.lock().expect("lock seen_paths").push(path);
    }

    fn snapshot(&self) -> Vec<String> {
        self.seen_paths.lock().expect("lock seen_paths").clone()
    }
}

struct BootedServer {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl BootedServer {
    async fn start(app: Router) -> Self {
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

async fn mock_entity_capabilities(
    State(state): State<MockCdaState>,
    Path(remote_component_id): Path<String>,
) -> Json<EntityCapabilities> {
    state.record(format!("/vehicle/v15/components/{remote_component_id}"));
    Json(EntityCapabilities {
        id: remote_component_id.clone(),
        name: remote_component_id.to_ascii_uppercase(),
        translation_id: None,
        variant: None,
        configurations: None,
        bulk_data: None,
        data: Some(format!(
            "http://mock-cda.invalid/vehicle/v15/components/{remote_component_id}/data"
        )),
        data_lists: None,
        faults: Some(format!(
            "http://mock-cda.invalid/vehicle/v15/components/{remote_component_id}/faults"
        )),
        operations: Some(format!(
            "http://mock-cda.invalid/vehicle/v15/components/{remote_component_id}/operations"
        )),
        updates: None,
        modes: None,
        subareas: None,
        subcomponents: None,
        locks: None,
        depends_on: None,
        hosts: None,
        is_located_on: None,
        scripts: None,
        logs: None,
    })
}

async fn mock_faults(
    State(state): State<MockCdaState>,
    Path(remote_component_id): Path<String>,
) -> Json<ListOfFaults> {
    state.record(format!(
        "/vehicle/v15/components/{remote_component_id}/faults"
    ));
    Json(ListOfFaults {
        items: vec![Fault {
            code: "P0A1F".to_owned(),
            scope: Some("Default".to_owned()),
            display_code: Some("P0A1F".to_owned()),
            fault_name: format!("{remote_component_id} mock fault"),
            fault_translation_id: None,
            severity: Some(2),
            status: Some(serde_json::json!({
                "aggregatedStatus": "active",
                "confirmedDTC": "1",
            })),
            symptom: None,
            symptom_translation_id: None,
            tags: None,
        }],
        total: None,
        next_page: None,
        schema: None,
        extras: None,
    })
}

fn mock_cda_app(state: MockCdaState) -> Router {
    Router::new()
        .route(
            "/vehicle/v15/components/{component_id}",
            get(mock_entity_capabilities),
        )
        .route(
            "/vehicle/v15/components/{component_id}/faults",
            get(mock_faults),
        )
        .with_state(state)
}

#[tokio::test]
async fn phase5_hybrid_alias_routing_keeps_external_ids_and_uses_remote_aliases() {
    let mock_cda_state = MockCdaState::default();
    let mock_cda = BootedServer::start(mock_cda_app(mock_cda_state.clone())).await;
    let mock_cda_base =
        Url::parse(&format!("{}/", mock_cda.base_url)).expect("parse mock CDA base URL");

    let server = Arc::new(
        InMemoryServer::new_with_demo_components(["bcm"]).expect("build BCM-only local surface"),
    );
    for (local_component_id, remote_component_id) in [
        ("cvc", "cvc00000"),
        ("sc", "sc00000"),
    ] {
        let backend = CdaBackend::new_with_remote_component_and_path_prefix(
            ComponentId::new(local_component_id),
            ComponentId::new(remote_component_id),
            mock_cda_base.clone(),
            "vehicle/v15",
        )
        .expect("build aliased CDA backend");
        server
            .register_forward(Arc::new(backend))
            .await
            .expect("register aliased forward");
    }

    let frontend = BootedServer::start(routes::app_with_server(Arc::clone(&server))).await;
    let client = reqwest::Client::new();

    let response = client
        .get(frontend.url("/sovd/v1/components"))
        .send()
        .await
        .expect("GET discovered entities");
    assert_eq!(response.status(), StatusCode::OK);
    let entities: DiscoveredEntities = response.json().await.expect("parse discovered entities");
    let ids: Vec<String> = entities.items.into_iter().map(|item| item.id).collect();
    // list_entities returns in alphabetical order (bcm, cvc, sc).
    assert_eq!(
        ids,
        vec![
            "bcm".to_owned(),
            "cvc".to_owned(),
            "sc".to_owned(),
        ]
    );

    let response = client
        .get(frontend.url("/sovd/v1/components/cvc"))
        .send()
        .await
        .expect("GET aliased cvc entity capabilities");
    assert_eq!(response.status(), StatusCode::OK);
    let capabilities: EntityCapabilities =
        response.json().await.expect("parse entity capabilities");
    assert_eq!(capabilities.id, "cvc");
    assert_eq!(capabilities.name, "cvc");
    assert_eq!(
        capabilities.data.as_deref(),
        Some("/sovd/v1/components/cvc/data")
    );
    assert_eq!(
        capabilities.faults.as_deref(),
        Some("/sovd/v1/components/cvc/faults")
    );
    assert_eq!(
        capabilities.operations.as_deref(),
        Some("/sovd/v1/components/cvc/operations")
    );

    let response = client
        .get(frontend.url("/sovd/v1/components/cvc/faults"))
        .send()
        .await
        .expect("GET aliased cvc faults");
    assert_eq!(response.status(), StatusCode::OK);
    let faults: ListOfFaults = response.json().await.expect("parse cvc faults");
    assert_eq!(faults.items.len(), 1);
    let rendered_faults: Vec<(String, String)> = faults
        .items
        .iter()
        .map(|fault| (fault.code.clone(), fault.fault_name.clone()))
        .collect();
    assert_eq!(
        rendered_faults,
        vec![("P0A1F".to_owned(), "cvc00000 mock fault".to_owned())]
    );

    let seen_paths = mock_cda_state.snapshot();
    assert!(
        seen_paths.contains(&"/vehicle/v15/components/cvc00000".to_owned()),
        "expected entity-capabilities request to use remote alias; got {seen_paths:?}"
    );
    assert!(
        seen_paths.contains(&"/vehicle/v15/components/cvc00000/faults".to_owned()),
        "expected faults request to use remote alias; got {seen_paths:?}"
    );
    assert!(
        !seen_paths
            .iter()
            .any(|path| path.contains("/components/cvc/")),
        "frontend must not leak local component id into downstream CDA paths; got {seen_paths:?}"
    );

    let local_bcm_faults = server
        .dispatch_list_faults(&ComponentId::new("bcm"), FaultFilter::all())
        .await
        .expect("local BCM faults");
    assert!(
        local_bcm_faults.items.is_empty(),
        "local BCM component should still be served by the in-memory store"
    );
}
