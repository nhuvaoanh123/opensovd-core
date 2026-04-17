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

//! [`RemoteHost`] — federated SOVD host reached over HTTP/REST.
//!
//! Wraps a `reqwest::Client` pointed at a remote SOVD server (another
//! `sovd-main` instance, or a native ISO 17978 compliant device). On
//! each request, the forwarder uses the ADR-0015 spec path table to
//! turn [`GatewayHost`] calls into SOVD v1 HTTP requests and decodes
//! the spec-typed response body. Failures map onto
//! [`SovdError::Transport`] so the gateway can report them uniformly
//! with local-host failures.

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use sovd_interfaces::{
    ComponentId, SovdError,
    spec::{
        component::EntityCapabilities,
        fault::{FaultDetails, FaultFilter, ListOfFaults},
        operation::{
            ExecutionStatusResponse, OperationsList, StartExecutionAsyncResponse,
            StartExecutionRequest,
        },
    },
    types::error::Result,
};
use url::Url;

use crate::GatewayHost;

/// Forwarding host that turns [`GatewayHost`] calls into SOVD v1 HTTP
/// requests against an upstream SOVD server.
#[derive(Debug, Clone)]
pub struct RemoteHost {
    name: String,
    base_url: Url,
    http: Client,
    components: Vec<ComponentId>,
}

impl RemoteHost {
    /// Build a new remote host.
    ///
    /// `base_url` should be the SOVD REST root
    /// (e.g. `http://127.0.0.1:9001/`). A trailing slash is
    /// appended if missing so subsequent path joins behave
    /// predictably.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`] if the underlying `reqwest`
    /// client cannot be built.
    pub fn new(
        name: impl Into<String>,
        base_url: Url,
        components: Vec<ComponentId>,
    ) -> Result<Self> {
        let base_url = ensure_trailing_slash(base_url);
        let http = Client::builder()
            .build()
            .map_err(|e| SovdError::Internal(format!("RemoteHost: build reqwest client: {e}")))?;
        Ok(Self {
            name: name.into(),
            base_url,
            http,
            components,
        })
    }

    /// Build a [`RemoteHost`] with a caller-supplied client — useful
    /// for tests that want to inject a recording or fault-injecting
    /// transport.
    #[must_use]
    pub fn with_client(
        name: impl Into<String>,
        base_url: Url,
        components: Vec<ComponentId>,
        http: Client,
    ) -> Self {
        Self {
            name: name.into(),
            base_url: ensure_trailing_slash(base_url),
            http,
            components,
        }
    }

    fn join(&self, path: &str) -> Result<Url> {
        // `Url::join` treats the path as relative to `base_url`. We
        // trim any leading slash so joining does not reset to the root
        // in the face of `base_url` already having a non-trivial path
        // segment.
        let trimmed = path.trim_start_matches('/');
        self.base_url
            .join(trimmed)
            .map_err(|e| SovdError::Internal(format!("RemoteHost: bad URL join: {e}")))
    }
}

fn ensure_trailing_slash(mut url: Url) -> Url {
    let path = url.path().to_owned();
    if !path.ends_with('/') {
        url.set_path(&format!("{path}/"));
    }
    url
}

/// Map an HTTP status + error body into a [`SovdError`]. Used for
/// non-2xx responses.
fn map_http_error(status: StatusCode, body: &str) -> SovdError {
    if status == StatusCode::NOT_FOUND {
        return SovdError::NotFound {
            entity: format!("remote 404: {body}"),
        };
    }
    if status == StatusCode::UNAUTHORIZED {
        return SovdError::Unauthorized;
    }
    if status.is_server_error() {
        return SovdError::Internal(format!("remote {status}: {body}"));
    }
    SovdError::Transport(format!("remote {status}: {body}"))
}

#[async_trait]
impl GatewayHost for RemoteHost {
    fn name(&self) -> &str {
        &self.name
    }

    fn components(&self) -> Vec<ComponentId> {
        self.components.clone()
    }

    async fn list_faults(
        &self,
        component: &ComponentId,
        _filter: FaultFilter,
    ) -> Result<ListOfFaults> {
        let url = self.join(&format!("sovd/v1/components/{component}/faults"))?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| SovdError::Transport(format!("list_faults: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(map_http_error(status, &body));
        }
        response
            .json::<ListOfFaults>()
            .await
            .map_err(|e| SovdError::Transport(format!("list_faults decode: {e}")))
    }

    async fn get_fault(&self, component: &ComponentId, code: &str) -> Result<FaultDetails> {
        let url = self.join(&format!("sovd/v1/components/{component}/faults/{code}"))?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| SovdError::Transport(format!("get_fault: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(map_http_error(status, &body));
        }
        response
            .json::<FaultDetails>()
            .await
            .map_err(|e| SovdError::Transport(format!("get_fault decode: {e}")))
    }

    async fn clear_all_faults(&self, component: &ComponentId) -> Result<()> {
        let url = self.join(&format!("sovd/v1/components/{component}/faults"))?;
        let response = self
            .http
            .delete(url)
            .send()
            .await
            .map_err(|e| SovdError::Transport(format!("clear_all_faults: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(map_http_error(status, &body));
        }
        Ok(())
    }

    async fn clear_fault(&self, component: &ComponentId, code: &str) -> Result<()> {
        let url = self.join(&format!("sovd/v1/components/{component}/faults/{code}"))?;
        let response = self
            .http
            .delete(url)
            .send()
            .await
            .map_err(|e| SovdError::Transport(format!("clear_fault: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(map_http_error(status, &body));
        }
        Ok(())
    }

    async fn list_operations(&self, component: &ComponentId) -> Result<OperationsList> {
        let url = self.join(&format!("sovd/v1/components/{component}/operations"))?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| SovdError::Transport(format!("list_operations: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(map_http_error(status, &body));
        }
        response
            .json::<OperationsList>()
            .await
            .map_err(|e| SovdError::Transport(format!("list_operations decode: {e}")))
    }

    async fn start_execution(
        &self,
        component: &ComponentId,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse> {
        let url = self.join(&format!(
            "sovd/v1/components/{component}/operations/{operation_id}/executions"
        ))?;
        let response = self
            .http
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(|e| SovdError::Transport(format!("start_execution: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(map_http_error(status, &body));
        }
        response
            .json::<StartExecutionAsyncResponse>()
            .await
            .map_err(|e| SovdError::Transport(format!("start_execution decode: {e}")))
    }

    async fn execution_status(
        &self,
        component: &ComponentId,
        operation_id: &str,
        execution_id: &str,
    ) -> Result<ExecutionStatusResponse> {
        let url = self.join(&format!(
            "sovd/v1/components/{component}/operations/{operation_id}/executions/{execution_id}"
        ))?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| SovdError::Transport(format!("execution_status: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(map_http_error(status, &body));
        }
        response
            .json::<ExecutionStatusResponse>()
            .await
            .map_err(|e| SovdError::Transport(format!("execution_status decode: {e}")))
    }

    async fn entity_capabilities(&self, component: &ComponentId) -> Result<EntityCapabilities> {
        let url = self.join(&format!("sovd/v1/components/{component}"))?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| SovdError::Transport(format!("entity_capabilities: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(map_http_error(status, &body));
        }
        response
            .json::<EntityCapabilities>()
            .await
            .map_err(|e| SovdError::Transport(format!("entity_capabilities decode: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        extract::Path,
        http::StatusCode as AxumStatus,
        routing::{get, post},
    };
    use sovd_interfaces::spec::{
        component::EntityCapabilities,
        fault::{Fault, FaultDetails},
        operation::{
            Capability, ExecutionStatus, ExecutionStatusResponse, OperationDescription,
            OperationsList, StartExecutionAsyncResponse,
        },
    };
    use tokio::net::TcpListener;

    use super::*;

    // --- pure unit tests -------------------------------------------------

    #[test]
    fn ensure_trailing_slash_noop_when_already_present() {
        let url = Url::parse("http://x/y/").unwrap();
        assert_eq!(ensure_trailing_slash(url.clone()), url);
    }

    #[test]
    fn ensure_trailing_slash_appends_when_missing() {
        let url = Url::parse("http://x/y").unwrap();
        assert_eq!(ensure_trailing_slash(url).as_str(), "http://x/y/");
    }

    #[test]
    fn map_http_error_variants() {
        assert!(matches!(
            map_http_error(StatusCode::NOT_FOUND, "nope"),
            SovdError::NotFound { .. }
        ));
        assert!(matches!(
            map_http_error(StatusCode::UNAUTHORIZED, "nope"),
            SovdError::Unauthorized
        ));
        assert!(matches!(
            map_http_error(StatusCode::INTERNAL_SERVER_ERROR, "boom"),
            SovdError::Internal(_)
        ));
        assert!(matches!(
            map_http_error(StatusCode::BAD_REQUEST, "bad"),
            SovdError::Transport(_)
        ));
    }

    // --- in-process mock SOVD server exercising the full wire path ------

    async fn mock_list_faults(Path(_component_id): Path<String>) -> Json<ListOfFaults> {
        Json(ListOfFaults {
            items: vec![Fault {
                code: "P0A1F".into(),
                scope: None,
                display_code: None,
                fault_name: "mock".into(),
                fault_translation_id: None,
                severity: Some(2),
                status: None,
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

    async fn mock_get_fault(
        Path((_component_id, fault_code)): Path<(String, String)>,
    ) -> Json<FaultDetails> {
        Json(FaultDetails {
            item: Fault {
                code: fault_code,
                scope: None,
                display_code: None,
                fault_name: "mock".into(),
                fault_translation_id: None,
                severity: Some(2),
                status: None,
                symptom: None,
                symptom_translation_id: None,
                tags: None,
            },
            environment_data: None,
            errors: None,
            schema: None,
            extras: None,
        })
    }

    async fn mock_clear_all_faults(Path(_component_id): Path<String>) -> AxumStatus {
        AxumStatus::NO_CONTENT
    }

    async fn mock_clear_fault(Path((_c, _f)): Path<(String, String)>) -> AxumStatus {
        AxumStatus::NO_CONTENT
    }

    async fn mock_list_operations(Path(_component_id): Path<String>) -> Json<OperationsList> {
        Json(OperationsList {
            items: vec![OperationDescription {
                id: "mock_op".into(),
                name: Some("mock op".into()),
                translation_id: None,
                proximity_proof_required: false,
                asynchronous_execution: false,
                tags: None,
            }],
            schema: None,
        })
    }

    async fn mock_start_execution(
        Path((_c, _op)): Path<(String, String)>,
    ) -> (AxumStatus, Json<StartExecutionAsyncResponse>) {
        (
            AxumStatus::ACCEPTED,
            Json(StartExecutionAsyncResponse {
                id: "mock-exec-1".into(),
                status: Some(ExecutionStatus::Running),
            }),
        )
    }

    async fn mock_execution_status(
        Path((_c, _op, _exec)): Path<(String, String, String)>,
    ) -> Json<ExecutionStatusResponse> {
        Json(ExecutionStatusResponse {
            status: Some(ExecutionStatus::Completed),
            capability: Capability::Execute,
            parameters: None,
            schema: None,
            error: None,
        })
    }

    async fn mock_entity_capabilities(
        Path(component_id): Path<String>,
    ) -> Json<EntityCapabilities> {
        Json(EntityCapabilities {
            id: component_id.clone(),
            name: format!("mock:{component_id}"),
            translation_id: None,
            variant: None,
            configurations: None,
            bulk_data: None,
            data: None,
            data_lists: None,
            faults: None,
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

    async fn mock_not_found() -> AxumStatus {
        AxumStatus::NOT_FOUND
    }

    fn mock_router() -> Router {
        Router::new()
            .route(
                "/sovd/v1/components/{component_id}",
                get(mock_entity_capabilities),
            )
            .route(
                "/sovd/v1/components/{component_id}/faults",
                get(mock_list_faults).delete(mock_clear_all_faults),
            )
            .route(
                "/sovd/v1/components/{component_id}/faults/{fault_code}",
                get(mock_get_fault).delete(mock_clear_fault),
            )
            .route(
                "/sovd/v1/components/{component_id}/operations",
                get(mock_list_operations),
            )
            .route(
                "/sovd/v1/components/{component_id}/operations/{operation_id}/executions",
                post(mock_start_execution),
            )
            .route(
                "/sovd/v1/components/{component_id}/operations/{operation_id}/executions/{execution_id}",
                get(mock_execution_status),
            )
            // A route that always 404s so the test can exercise the
            // error mapping path for a real wire failure.
            .route("/sovd/v1/missing", get(mock_not_found))
    }

    async fn start_mock_host(components: Vec<&str>) -> (RemoteHost, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let base_url = Url::parse(&format!("http://{addr}/")).expect("url");
        let handle = tokio::spawn(async move {
            axum::serve(listener, mock_router()).await.expect("serve");
        });
        let host = RemoteHost::new(
            "mock-remote",
            base_url,
            components.into_iter().map(ComponentId::new).collect(),
        )
        .expect("remote host");
        (host, handle)
    }

    #[tokio::test]
    async fn remote_list_faults_round_trip() {
        let (host, handle) = start_mock_host(vec!["cvc"]).await;
        let list = host
            .list_faults(&ComponentId::new("cvc"), FaultFilter::all())
            .await
            .expect("list faults");
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items.first().unwrap().code, "P0A1F");
        handle.abort();
    }

    #[tokio::test]
    async fn remote_get_fault_round_trip() {
        let (host, handle) = start_mock_host(vec!["cvc"]).await;
        let details = host
            .get_fault(&ComponentId::new("cvc"), "P0A1F")
            .await
            .expect("get fault");
        assert_eq!(details.item.code, "P0A1F");
        handle.abort();
    }

    #[tokio::test]
    async fn remote_clear_all_and_clear_fault_round_trip() {
        let (host, handle) = start_mock_host(vec!["cvc"]).await;
        host.clear_all_faults(&ComponentId::new("cvc"))
            .await
            .expect("clear all");
        host.clear_fault(&ComponentId::new("cvc"), "P0A1F")
            .await
            .expect("clear one");
        handle.abort();
    }

    #[tokio::test]
    async fn remote_list_operations_round_trip() {
        let (host, handle) = start_mock_host(vec!["cvc"]).await;
        let ops = host
            .list_operations(&ComponentId::new("cvc"))
            .await
            .expect("list ops");
        assert_eq!(ops.items.len(), 1);
        assert_eq!(ops.items.first().unwrap().id, "mock_op");
        handle.abort();
    }

    #[tokio::test]
    async fn remote_start_execution_and_status_round_trip() {
        let (host, handle) = start_mock_host(vec!["cvc"]).await;
        let started = host
            .start_execution(
                &ComponentId::new("cvc"),
                "mock_op",
                StartExecutionRequest {
                    timeout: None,
                    parameters: None,
                    proximity_response: None,
                },
            )
            .await
            .expect("start exec");
        assert_eq!(started.id, "mock-exec-1");
        let status = host
            .execution_status(&ComponentId::new("cvc"), "mock_op", &started.id)
            .await
            .expect("status");
        assert_eq!(status.status, Some(ExecutionStatus::Completed));
        handle.abort();
    }

    #[tokio::test]
    async fn remote_entity_capabilities_round_trip() {
        let (host, handle) = start_mock_host(vec!["cvc"]).await;
        let caps = host
            .entity_capabilities(&ComponentId::new("cvc"))
            .await
            .expect("caps");
        assert_eq!(caps.id, "cvc");
        handle.abort();
    }

    #[tokio::test]
    async fn remote_maps_transport_failure_when_server_down() {
        // Build a RemoteHost aimed at a port that is guaranteed to
        // have nothing listening. Every call should surface as
        // SovdError::Transport, not Internal or NotFound.
        let base = Url::parse("http://127.0.0.1:1/").unwrap();
        let host = RemoteHost::new("dead", base, vec![ComponentId::new("cvc")]).unwrap();
        let err = host
            .list_faults(&ComponentId::new("cvc"), FaultFilter::all())
            .await
            .unwrap_err();
        assert!(matches!(err, SovdError::Transport(_)), "{err:?}");
    }

    #[test]
    fn remote_host_name_and_components_accessors() {
        let host = RemoteHost::new(
            "accessor-test",
            Url::parse("http://x/").unwrap(),
            vec![ComponentId::new("a"), ComponentId::new("b")],
        )
        .unwrap();
        assert_eq!(host.name(), "accessor-test");
        assert_eq!(
            host.components()
                .iter()
                .map(|c| c.as_str().to_owned())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn remote_host_with_client_preserves_base_url() {
        let client = reqwest::Client::new();
        let host =
            RemoteHost::with_client("wc", Url::parse("http://example").unwrap(), vec![], client);
        assert_eq!(host.base_url.as_str(), "http://example/");
    }
}
