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

//! [`CdaBackend`] — SOVD backend that forwards to an upstream Classic
//! Diagnostic Adapter over HTTP/REST.
//!
//! This is the Phase 2 Line A glue between [`sovd-server`] and the upstream
//! CDA binary. One `CdaBackend` serves exactly one [`ComponentId`]; the
//! hybrid dispatcher in
//! [`crate::in_memory::InMemoryServer`] holds one instance per forwarded
//! component and routes requests to CDA at `base_url` using the SOVD
//! v1 REST paths mirror-ported from upstream `cda-sovd`.
//!
//! # Why this forwards to CDA at all
//!
//! In Phase 2 Line A the SOVD Gateway pattern is cut down to its simplest
//! useful shape: our [`InMemoryServer`](crate::InMemoryServer) serves the
//! native demo components (`bcm`/`icu`/`tcu` or whatever the caller
//! configures) from local state, and forwards the legacy multi-ECU
//! components (`cvc`/`fzc`/`rzc` via upstream `ecu-sim`) to CDA over HTTP.
//! See `docs/prompts/phase-2-line-a.md` for the topology diagram.
//!
//! # Wire-boundary rule (ADR-0015)
//!
//! Every type crossing the HTTP boundary is imported from
//! `sovd_interfaces::spec`. We do not hand-draft DTOs — if the CDA ever
//! drifts from the spec, `reqwest::Response::json::<SpecType>()` fails
//! loudly and the caller sees the mismatch as a `SovdError::Transport`.

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use sovd_interfaces::{
    ComponentId, SovdError,
    spec::{
        component::EntityCapabilities,
        fault::{FaultFilter, ListOfFaults},
        operation::{StartExecutionAsyncResponse, StartExecutionRequest},
    },
    traits::backend::{BackendKind, SovdBackend},
    types::error::Result,
};
use url::Url;

/// Forwarding backend that turns [`SovdBackend`] trait calls into SOVD v1
/// HTTP requests against an upstream CDA.
///
/// Construct with [`CdaBackend::new`] and register with the dispatcher via
/// [`InMemoryServer::register_forward`](crate::in_memory::InMemoryServer::register_forward).
#[derive(Debug, Clone)]
pub struct CdaBackend {
    /// Which component this backend is bound to. Stored on construction
    /// so `SovdBackend::component_id` is cheap and infallible.
    component_id: ComponentId,
    /// Base URL of the upstream CDA SOVD REST root, e.g.
    /// `http://127.0.0.1:20002/`. Must end with a trailing slash; we
    /// normalize on construction.
    base_url: Url,
    /// Shared reqwest client. Safe to clone across requests.
    http: Client,
}

impl CdaBackend {
    /// Build a new [`CdaBackend`] for `component_id` that forwards to
    /// `base_url`.
    ///
    /// `base_url` should be the upstream CDA's SOVD REST root, e.g.
    /// `http://127.0.0.1:20002/`. A trailing slash is appended if missing
    /// so subsequent path joins behave predictably.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`] if the underlying `reqwest` client
    /// cannot be constructed (typically a TLS backend initialization
    /// failure).
    pub fn new(component_id: ComponentId, base_url: Url) -> Result<Self> {
        let base_url = ensure_trailing_slash(base_url);
        let http = Client::builder()
            .build()
            .map_err(|e| SovdError::Internal(format!("build reqwest client: {e}")))?;
        Ok(Self {
            component_id,
            base_url,
            http,
        })
    }

    /// Construct a [`CdaBackend`] with a caller-supplied [`reqwest::Client`],
    /// mostly useful for tests that want to inject a custom transport or
    /// timeout profile.
    #[must_use]
    pub fn with_client(component_id: ComponentId, base_url: Url, http: Client) -> Self {
        let base_url = ensure_trailing_slash(base_url);
        Self {
            component_id,
            base_url,
            http,
        }
    }

    /// Borrow the CDA base URL this backend is configured against.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Build the SOVD v1 component sub-path for this backend's component,
    /// e.g. `sovd/v1/components/cvc/faults`. The returned [`Url`] is
    /// ready to pass to [`reqwest::Client::get`] etc.
    fn component_url(&self, tail: &str) -> Result<Url> {
        let joined = format!("sovd/v1/components/{}/{}", self.component_id, tail);
        self.base_url
            .join(&joined)
            .map_err(|e| SovdError::InvalidRequest(format!("bad CDA URL: {e}")))
    }
}

/// Ensure `url` ends with `/` so [`Url::join`] treats it as a directory.
fn ensure_trailing_slash(mut url: Url) -> Url {
    if !url.path().ends_with('/') {
        let new_path = format!("{}/", url.path());
        url.set_path(&new_path);
    }
    url
}

/// Translate a `reqwest` error into a [`SovdError`]. 404 becomes
/// [`SovdError::NotFound`]; connection / IO errors become
/// [`SovdError::BackendUnavailable`]; everything else is
/// [`SovdError::Transport`].
fn map_reqwest_err(component: &ComponentId, err: &reqwest::Error) -> SovdError {
    if err.is_connect() || err.is_timeout() || err.is_request() {
        return SovdError::BackendUnavailable(component.clone());
    }
    if let Some(status) = err.status() {
        if status == StatusCode::NOT_FOUND {
            return SovdError::NotFound {
                entity: format!("cda:{component}"),
            };
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return SovdError::Unauthorized;
        }
    }
    SovdError::Transport(err.to_string())
}

#[async_trait]
impl SovdBackend for CdaBackend {
    fn component_id(&self) -> ComponentId {
        self.component_id.clone()
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Cda
    }

    async fn list_faults(&self, filter: FaultFilter) -> Result<ListOfFaults> {
        let mut url = self.component_url("faults")?;
        // Apply the spec-defined filter fields as query params. We leave
        // them out entirely on `FaultFilter::all()` so CDA returns the
        // unfiltered list (upstream semantics).
        if let Some(sev) = filter.severity {
            url.query_pairs_mut()
                .append_pair("severity", &sev.to_string());
        }
        if let Some(scope) = filter.scope {
            url.query_pairs_mut().append_pair("scope", &scope);
        }
        for (k, v) in &filter.status_keys {
            url.query_pairs_mut()
                .append_pair(&format!("status[{k}]"), v);
        }

        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?
            .error_for_status()
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?;
        resp.json::<ListOfFaults>()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))
    }

    async fn clear_all_faults(&self) -> Result<()> {
        let url = self.component_url("faults")?;
        self.http
            .delete(url)
            .send()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?
            .error_for_status()
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?;
        Ok(())
    }

    async fn clear_fault(&self, code: &str) -> Result<()> {
        let url = self.component_url(&format!("faults/{code}"))?;
        self.http
            .delete(url)
            .send()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?
            .error_for_status()
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?;
        Ok(())
    }

    async fn start_execution(
        &self,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse> {
        let url = self.component_url(&format!("operations/{operation_id}/executions"))?;
        let resp = self
            .http
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?
            .error_for_status()
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?;
        resp.json::<StartExecutionAsyncResponse>()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))
    }

    async fn entity_capabilities(&self) -> Result<EntityCapabilities> {
        let joined = format!("sovd/v1/components/{}", self.component_id);
        let url = self
            .base_url
            .join(&joined)
            .map_err(|e| SovdError::InvalidRequest(format!("bad CDA URL: {e}")))?;
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?
            .error_for_status()
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?;
        resp.json::<EntityCapabilities>()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_slash_is_added_when_missing() {
        let url = Url::parse("http://localhost:20002").expect("parse");
        let backend = CdaBackend::new(ComponentId::new("cvc"), url).expect("construct");
        assert!(backend.base_url().path().ends_with('/'));
    }

    #[test]
    fn component_url_builds_nested_path() {
        let url = Url::parse("http://localhost:20002/").expect("parse");
        let backend = CdaBackend::new(ComponentId::new("cvc"), url).expect("construct");
        let got = backend.component_url("faults").expect("join");
        assert_eq!(got.path(), "/sovd/v1/components/cvc/faults");
    }

    #[test]
    fn component_id_round_trips() {
        let url = Url::parse("http://localhost:20002/").expect("parse");
        let backend = CdaBackend::new(ComponentId::new("cvc"), url).expect("construct");
        assert_eq!(backend.component_id(), ComponentId::new("cvc"));
        assert_eq!(backend.kind(), BackendKind::Cda);
    }
}
