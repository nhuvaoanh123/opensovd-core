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

//! Phase 5 Line A D8 - local pagination contract for `/faults`.
//!
//! D8 is a HIL deliverable, but the route-level pagination behaviour
//! should still be proven locally on every machine. This test mounts the
//! full axum router with a mock forward backend that returns 120 faults,
//! then verifies:
//!
//! - default paging uses `page=1&page-size=50`
//! - `total` and `next_page` are populated on the wire
//! - page 3 returns the tail 20 items without truncation
//! - invalid pagination params map to HTTP 400 + `request.invalid`

use std::{collections::BTreeSet, sync::Arc};

use async_trait::async_trait;
use reqwest::StatusCode;
use sovd_interfaces::{
    ComponentId, SovdError,
    spec::{
        component::EntityCapabilities,
        error::GenericError,
        fault::{Fault, FaultDetails, FaultFilter, ListOfFaults},
        operation::{StartExecutionAsyncResponse, StartExecutionRequest},
    },
    traits::backend::{BackendHealth, BackendKind, SovdBackend},
};
use sovd_server::{InMemoryServer, routes};
use tokio::net::TcpListener;

struct BootedServer {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl BootedServer {
    async fn start(server: Arc<InMemoryServer>) -> Self {
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

struct PaginationBackend {
    component: ComponentId,
    items: Vec<Fault>,
}

impl PaginationBackend {
    fn new(component: &str, item_count: usize) -> Self {
        Self {
            component: ComponentId::new(component),
            items: build_faults(item_count),
        }
    }
}

fn build_faults(item_count: usize) -> Vec<Fault> {
    (0..item_count)
        .map(|index| Fault {
            code: format!("FAULT_{index:03}"),
            scope: Some("Default".into()),
            display_code: Some(format!("P{index:04}")),
            fault_name: format!("Synthetic fault {index}"),
            fault_translation_id: None,
            severity: Some(2),
            status: Some(serde_json::json!({
                "aggregatedStatus": "active",
                "ordinal": index,
            })),
            symptom: None,
            symptom_translation_id: None,
            tags: Some(vec!["phase5-d8".into()]),
        })
        .collect()
}

#[async_trait]
impl SovdBackend for PaginationBackend {
    fn component_id(&self) -> ComponentId {
        self.component.clone()
    }

    fn kind(&self) -> BackendKind {
        BackendKind::NativeSovd
    }

    async fn list_faults(&self, _filter: FaultFilter) -> Result<ListOfFaults, SovdError> {
        Ok(ListOfFaults {
            items: self.items.clone(),
            total: None,
            next_page: None,
            schema: None,
            extras: None,
        })
    }

    async fn get_fault(&self, code: &str) -> Result<FaultDetails, SovdError> {
        let item = self
            .items
            .iter()
            .find(|fault| fault.code == code)
            .cloned()
            .ok_or_else(|| SovdError::NotFound {
                entity: format!("fault \"{code}\""),
            })?;
        Ok(FaultDetails {
            item,
            environment_data: None,
            errors: None,
            schema: None,
            extras: None,
        })
    }

    async fn clear_all_faults(&self) -> Result<(), SovdError> {
        Err(SovdError::InvalidRequest(
            "clear_all_faults not used in pagination contract test".into(),
        ))
    }

    async fn clear_fault(&self, _code: &str) -> Result<(), SovdError> {
        Err(SovdError::InvalidRequest(
            "clear_fault not used in pagination contract test".into(),
        ))
    }

    async fn start_execution(
        &self,
        _operation_id: &str,
        _request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse, SovdError> {
        Err(SovdError::InvalidRequest(
            "start_execution not used in pagination contract test".into(),
        ))
    }

    async fn entity_capabilities(&self) -> Result<EntityCapabilities, SovdError> {
        Ok(EntityCapabilities {
            id: self.component.as_str().to_owned(),
            name: format!("mock:{}", self.component),
            translation_id: None,
            variant: None,
            configurations: None,
            bulk_data: None,
            data: None,
            data_lists: None,
            faults: Some(format!("/sovd/v1/components/{}/faults", self.component)),
            operations: None,
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

    async fn health_probe(&self) -> BackendHealth {
        BackendHealth::Ok
    }
}

async fn boot_pagination_server(item_count: usize) -> BootedServer {
    let server = Arc::new(InMemoryServer::new_with_demo_data());
    server
        .register_forward(Arc::new(PaginationBackend::new("cvc", item_count)))
        .await
        .expect("register mock backend");
    BootedServer::start(server).await
}

async fn get_faults(client: &reqwest::Client, booted: &BootedServer, suffix: &str) -> ListOfFaults {
    let response = client
        .get(booted.url(suffix))
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {suffix} failed: {e}"));
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "unexpected status for {suffix}"
    );
    response
        .json()
        .await
        .unwrap_or_else(|e| panic!("decode ListOfFaults for {suffix}: {e}"))
}

#[tokio::test]
async fn faults_endpoint_paginates_large_lists_and_reports_total() {
    let booted = boot_pagination_server(120).await;
    let client = reqwest::Client::new();

    let first = get_faults(&client, &booted, "/sovd/v1/components/cvc/faults").await;
    assert_eq!(first.total, Some(120));
    assert_eq!(first.next_page, Some(2));
    assert_eq!(first.items.len(), 50);
    assert_eq!(
        first.items.first().map(|fault| fault.code.as_str()),
        Some("FAULT_000")
    );
    assert_eq!(
        first.items.last().map(|fault| fault.code.as_str()),
        Some("FAULT_049")
    );

    let second = get_faults(
        &client,
        &booted,
        "/sovd/v1/components/cvc/faults?page=2&page-size=50",
    )
    .await;
    assert_eq!(second.total, Some(120));
    assert_eq!(second.next_page, Some(3));
    assert_eq!(second.items.len(), 50);
    assert_eq!(
        second.items.first().map(|fault| fault.code.as_str()),
        Some("FAULT_050")
    );
    assert_eq!(
        second.items.last().map(|fault| fault.code.as_str()),
        Some("FAULT_099")
    );

    let third = get_faults(
        &client,
        &booted,
        "/sovd/v1/components/cvc/faults?page=3&page-size=50",
    )
    .await;
    assert_eq!(third.total, Some(120));
    assert_eq!(third.next_page, None);
    assert_eq!(third.items.len(), 20);
    assert_eq!(
        third.items.first().map(|fault| fault.code.as_str()),
        Some("FAULT_100")
    );
    assert_eq!(
        third.items.last().map(|fault| fault.code.as_str()),
        Some("FAULT_119")
    );

    let all_rows = first
        .items
        .iter()
        .chain(&second.items)
        .chain(&third.items)
        .map(|fault| serde_json::to_string(fault).expect("serialize fault"))
        .collect::<Vec<_>>();
    let unique_rows = all_rows.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(all_rows.len(), 120, "pagination dropped or duplicated rows");
    assert_eq!(
        unique_rows.len(),
        120,
        "page boundaries overlapped or repeated fault rows"
    );
}

#[tokio::test]
async fn faults_endpoint_rejects_invalid_pagination_params() {
    let booted = boot_pagination_server(120).await;
    let client = reqwest::Client::new();

    for suffix in [
        "/sovd/v1/components/cvc/faults?page=0",
        "/sovd/v1/components/cvc/faults?page-size=0",
        "/sovd/v1/components/cvc/faults?page=banana",
        "/sovd/v1/components/cvc/faults?page-size=banana",
    ] {
        let response = client
            .get(booted.url(suffix))
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {suffix} failed: {e}"));
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{suffix} must map to 400"
        );
        let err: GenericError = response
            .json()
            .await
            .unwrap_or_else(|e| panic!("decode GenericError for {suffix}: {e}"));
        assert_eq!(err.error_code, "request.invalid");
        assert!(
            err.message.contains("page"),
            "invalid pagination error should mention the bad parameter, got: {}",
            err.message
        );
    }
}
