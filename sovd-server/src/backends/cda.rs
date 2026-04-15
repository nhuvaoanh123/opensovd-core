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

/// Default downstream REST root served by the upstream `cda-sovd`
/// binary (see `classic-diagnostic-adapter/cda-sovd/src/sovd/mod.rs`).
///
/// Today upstream CDA mounts its entire REST surface under
/// `/vehicle/v15/*` (e.g. `/vehicle/v15/authorize`,
/// `/vehicle/v15/components/{id}/faults`). Any `CdaBackend` built via
/// [`CdaBackend::new`] or [`CdaBackend::with_client`] inherits this
/// default so it forwards to upstream as-shipped (ADR-0006
/// track-upstream-as-it-is).
///
/// If/when upstream CDA migrates to serving `/sovd/v1/*` natively, flip
/// this single constant and re-run the Phase 2 / Phase 4 benches. Call
/// sites that need a different prefix in the meantime should use
/// [`CdaBackend::new_with_path_prefix`].
pub const DEFAULT_CDA_PATH_PREFIX: &str = "vehicle/v15";

/// Forwarding backend that turns [`SovdBackend`] trait calls into SOVD
/// HTTP requests against an upstream CDA.
///
/// Construct with [`CdaBackend::new`] (which uses
/// [`DEFAULT_CDA_PATH_PREFIX`]) or with
/// [`CdaBackend::new_with_path_prefix`] for an explicit downstream REST
/// root. Register with the dispatcher via
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
    /// Downstream REST path prefix joined under [`Self::base_url`],
    /// e.g. `"vehicle/v15"`. Normalised on construction: no leading
    /// slash, no trailing slash. Empty string is allowed and means
    /// "join directly under base_url".
    path_prefix: String,
    /// Shared reqwest client. Safe to clone across requests.
    http: Client,
}

impl CdaBackend {
    /// Build a new [`CdaBackend`] for `component_id` that forwards to
    /// `base_url` using the default [`DEFAULT_CDA_PATH_PREFIX`].
    ///
    /// `base_url` should be the upstream CDA's SOVD REST root, e.g.
    /// `http://127.0.0.1:20002/`. A trailing slash is appended if missing
    /// so subsequent path joins behave predictably.
    ///
    /// The default path prefix is [`DEFAULT_CDA_PATH_PREFIX`]
    /// (`vehicle/v15`) to match the current upstream `cda-sovd` REST
    /// root. Callers that need a different prefix — for instance tests
    /// that target a mock CDA speaking `/sovd/v1/*`, or a future
    /// upstream that has migrated to `/sovd/v1/*` natively — should use
    /// [`CdaBackend::new_with_path_prefix`].
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`] if the underlying `reqwest` client
    /// cannot be constructed (typically a TLS backend initialization
    /// failure).
    pub fn new(component_id: ComponentId, base_url: Url) -> Result<Self> {
        Self::new_with_path_prefix(component_id, base_url, DEFAULT_CDA_PATH_PREFIX)
    }

    /// Build a [`CdaBackend`] with an explicit downstream REST path
    /// prefix (for example `"vehicle/v15"` or `"sovd/v1"`).
    ///
    /// Leading and trailing slashes on `path_prefix` are stripped, so
    /// `"/vehicle/v15"`, `"vehicle/v15/"`, `"/vehicle/v15/"` and
    /// `"vehicle/v15"` all produce the same final URL. Passing `""` is
    /// allowed and means "no prefix — join `components/...` directly
    /// under `base_url`".
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`] if the underlying `reqwest` client
    /// cannot be constructed.
    pub fn new_with_path_prefix(
        component_id: ComponentId,
        base_url: Url,
        path_prefix: &str,
    ) -> Result<Self> {
        let base_url = ensure_trailing_slash(base_url);
        let http = Client::builder()
            .build()
            .map_err(|e| SovdError::Internal(format!("build reqwest client: {e}")))?;
        Ok(Self {
            component_id,
            base_url,
            path_prefix: normalise_prefix(path_prefix),
            http,
        })
    }

    /// Construct a [`CdaBackend`] with a caller-supplied [`reqwest::Client`],
    /// mostly useful for tests that want to inject a custom transport or
    /// timeout profile. Uses [`DEFAULT_CDA_PATH_PREFIX`].
    #[must_use]
    pub fn with_client(component_id: ComponentId, base_url: Url, http: Client) -> Self {
        Self::with_client_and_path_prefix(component_id, base_url, http, DEFAULT_CDA_PATH_PREFIX)
    }

    /// Construct a [`CdaBackend`] with a caller-supplied
    /// [`reqwest::Client`] and explicit downstream REST path prefix.
    #[must_use]
    pub fn with_client_and_path_prefix(
        component_id: ComponentId,
        base_url: Url,
        http: Client,
        path_prefix: &str,
    ) -> Self {
        let base_url = ensure_trailing_slash(base_url);
        Self {
            component_id,
            base_url,
            path_prefix: normalise_prefix(path_prefix),
            http,
        }
    }

    /// Borrow the CDA base URL this backend is configured against.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Borrow the downstream REST path prefix (without surrounding
    /// slashes), e.g. `"vehicle/v15"`.
    #[must_use]
    pub fn path_prefix(&self) -> &str {
        &self.path_prefix
    }

    /// Probe the configured downstream REST root to verify the
    /// [`Self::path_prefix`] actually matches what the remote CDA is
    /// serving. Issues a `GET {base_url}{path_prefix}/components` and
    /// accepts any response status that proves the route is mounted
    /// (2xx, 401/403 if CDA enforces auth, or any 4xx other than 404
    /// — a 404 on the `components` collection endpoint is treated as
    /// a prefix mismatch).
    ///
    /// This catches the "CDA is reachable but serving a different
    /// route prefix" case early so integration tests can fail with a
    /// clear `SovdError::InvalidRequest` instead of a mid-test 404.
    ///
    /// # Errors
    ///
    /// - [`SovdError::BackendUnavailable`] if the HTTP probe cannot
    ///   connect at all.
    /// - [`SovdError::InvalidRequest`] if the probe reaches the server
    ///   but the prefix is clearly wrong (404 on `components`).
    /// - [`SovdError::Transport`] on any other unexpected response.
    pub async fn preflight(&self) -> Result<()> {
        let joined = if self.path_prefix.is_empty() {
            "components".to_owned()
        } else {
            format!("{}/components", self.path_prefix)
        };
        let url = self
            .base_url
            .join(&joined)
            .map_err(|e| SovdError::InvalidRequest(format!("bad CDA URL: {e}")))?;
        let resp = match self.http.get(url.clone()).send().await {
            Ok(r) => r,
            Err(e) => {
                if e.is_connect() || e.is_timeout() || e.is_request() {
                    return Err(SovdError::BackendUnavailable(self.component_id.clone()));
                }
                return Err(SovdError::Transport(format!(
                    "CDA preflight {url}: {e}"
                )));
            }
        };
        let status = resp.status();
        if status.is_success()
            || status == StatusCode::UNAUTHORIZED
            || status == StatusCode::FORBIDDEN
        {
            return Ok(());
        }
        if status == StatusCode::NOT_FOUND {
            return Err(SovdError::InvalidRequest(format!(
                "CDA preflight: {url} returned 404 — path_prefix {:?} likely does not \
                 match the upstream CDA REST root. Expected e.g. \"vehicle/v15\" (current \
                 upstream) or \"sovd/v1\" (future). See CdaBackend::new_with_path_prefix.",
                self.path_prefix
            )));
        }
        // Any other 4xx is a soft pass — route exists but rejected
        // the probe shape. 5xx is a wire failure worth surfacing.
        if status.is_client_error() {
            return Ok(());
        }
        Err(SovdError::Transport(format!(
            "CDA preflight {url}: unexpected status {status}"
        )))
    }

    /// Build the downstream component sub-path for this backend's
    /// component under the configured [`Self::path_prefix`], e.g.
    /// `vehicle/v15/components/cvc/faults`. The returned [`Url`] is
    /// ready to pass to [`reqwest::Client::get`] etc.
    fn component_url(&self, tail: &str) -> Result<Url> {
        let joined = if self.path_prefix.is_empty() {
            format!("components/{}/{}", self.component_id, tail)
        } else {
            format!(
                "{}/components/{}/{}",
                self.path_prefix, self.component_id, tail
            )
        };
        self.base_url
            .join(&joined)
            .map_err(|e| SovdError::InvalidRequest(format!("bad CDA URL: {e}")))
    }
}

/// Strip any leading and trailing `/` from `prefix` so
/// `"/vehicle/v15/"`, `"vehicle/v15"`, `"vehicle/v15/"` and
/// `"/vehicle/v15"` all normalise to `"vehicle/v15"`. Empty input
/// remains empty.
fn normalise_prefix(prefix: &str) -> String {
    prefix.trim_matches('/').to_owned()
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
        let joined = if self.path_prefix.is_empty() {
            format!("components/{}", self.component_id)
        } else {
            format!("{}/components/{}", self.path_prefix, self.component_id)
        };
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
    fn component_url_default_prefix_matches_upstream_cda() {
        // D2: default prefix tracks upstream cda-sovd reality at
        // /vehicle/v15/* (ADR-0006 track-upstream-as-it-is).
        let url = Url::parse("http://localhost:20002/").expect("parse");
        let backend = CdaBackend::new(ComponentId::new("cvc"), url).expect("construct");
        let got = backend.component_url("faults").expect("join");
        assert_eq!(got.path(), "/vehicle/v15/components/cvc/faults");
    }

    #[test]
    fn component_url_honours_explicit_prefix_override() {
        // D1: callers may opt into a different REST root (e.g. for tests
        // hitting a mock CDA that speaks /sovd/v1/*, or for forward
        // compat when upstream migrates).
        let url = Url::parse("http://localhost:20002/").expect("parse");
        let backend = CdaBackend::new_with_path_prefix(
            ComponentId::new("cvc"),
            url,
            "sovd/v1",
        )
        .expect("construct with prefix");
        let got = backend.component_url("faults").expect("join");
        assert_eq!(got.path(), "/sovd/v1/components/cvc/faults");
    }

    #[test]
    fn component_url_normalises_prefix_slashes() {
        // Tolerate leading / and trailing / on the caller-supplied
        // prefix so both "/vehicle/v15" and "vehicle/v15/" build the
        // same final URL.
        let url = Url::parse("http://localhost:20002/").expect("parse");
        for raw in ["/vehicle/v15", "vehicle/v15/", "/vehicle/v15/", "vehicle/v15"] {
            let backend = CdaBackend::new_with_path_prefix(
                ComponentId::new("cvc"),
                url.clone(),
                raw,
            )
            .expect("construct with prefix");
            let got = backend.component_url("faults").expect("join");
            assert_eq!(
                got.path(),
                "/vehicle/v15/components/cvc/faults",
                "prefix {raw:?} did not normalise",
            );
        }
    }

    #[test]
    fn component_id_round_trips() {
        let url = Url::parse("http://localhost:20002/").expect("parse");
        let backend = CdaBackend::new(ComponentId::new("cvc"), url).expect("construct");
        assert_eq!(backend.component_id(), ComponentId::new("cvc"));
        assert_eq!(backend.kind(), BackendKind::Cda);
    }
}
