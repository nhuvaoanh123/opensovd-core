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
    use super::*;

    #[test]
    fn ensure_trailing_slash_noop_when_already_present() {
        let url = Url::parse("http://x/y/").unwrap();
        assert_eq!(ensure_trailing_slash(url.clone()), url);
    }

    #[test]
    fn ensure_trailing_slash_appends_when_missing() {
        let url = Url::parse("http://x/y").unwrap();
        assert_eq!(
            ensure_trailing_slash(url).as_str(),
            "http://x/y/"
        );
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
        // 502 is a 5xx and therefore maps to Internal per the rule
        // in `map_http_error`. We use BAD_REQUEST here to cover the
        // generic Transport mapping for non-{401, 404, 5xx} failures.
        assert!(matches!(
            map_http_error(StatusCode::BAD_REQUEST, "bad"),
            SovdError::Transport(_)
        ));
    }
}
