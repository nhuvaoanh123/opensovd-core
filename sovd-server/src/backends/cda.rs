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

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use sovd_interfaces::{
    ComponentId, SovdError,
    spec::{
        component::EntityCapabilities,
        error::{DataError, GenericError},
        fault::{FaultFilter, ListOfFaults},
        operation::{
            Capability, ExecutionStatus, ExecutionStatusResponse, StartExecutionAsyncResponse,
            StartExecutionRequest,
        },
    },
    traits::backend::{BackendKind, SovdBackend},
    types::error::Result,
};
use tokio::sync::Mutex;
use url::Url;
use uuid::Uuid;

/// ADR-0018 rule 2: retry policy for `CdaBackend` wire calls. The
/// numbers are inlined here (not a generic middleware) so the
/// policy stays visible at the call site — the alternative
/// "`RetryMiddleware` on every backend" was explicitly rejected in
/// the ADR's Alternatives section.
const CDA_MAX_ATTEMPTS: u32 = 3;
const CDA_TOTAL_BUDGET: Duration = Duration::from_millis(2_000);
const CDA_INITIAL_BACKOFF: Duration = Duration::from_millis(50);

/// Classify whether a `reqwest::Error` is worth retrying. Connection
/// resets, timeouts, and 5xx responses are transient. 4xx (except
/// 408/429) is a client-side problem that retrying will not fix.
fn is_retryable(err: &reqwest::Error) -> bool {
    if err.is_connect() || err.is_timeout() || err.is_request() {
        return true;
    }
    if let Some(status) = err.status() {
        if status.is_server_error() {
            return true;
        }
        if status == StatusCode::REQUEST_TIMEOUT || status == StatusCode::TOO_MANY_REQUESTS {
            return true;
        }
    }
    false
}

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
const LOCAL_COMPONENT_BASE_PATH_PREFIX: &str = "/sovd/v1/components";
const CDA_FORWARD_AUTH_CLIENT_ID: &str = "opensovd-cda-forward";
const CDA_FORWARD_AUTH_CLIENT_SECRET: &str = "secret";

#[derive(Debug, Deserialize)]
struct CdaAuthBody {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct CdaSyncExecutionResponse {
    #[serde(default)]
    parameters: Option<serde_json::Value>,
    #[serde(default)]
    errors: Vec<DataError>,
}

#[derive(Debug, Clone)]
struct CdaExecutionRecord {
    operation_id: String,
    status: ExecutionStatusResponse,
}

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
    /// Which downstream CDA component this backend should talk to.
    /// Usually the same as `component_id`, but Phase 5 can alias local
    /// `cvc`/`fzc`/`rzc` onto generated CDA ids like `cvc00000`.
    remote_component_id: ComponentId,
    /// Base URL of the upstream CDA SOVD REST root, e.g.
    /// `http://127.0.0.1:20002/`. Must end with a trailing slash; we
    /// normalize on construction.
    base_url: Url,
    /// Downstream REST path prefix joined under [`Self::base_url`],
    /// e.g. `"vehicle/v15"`. Normalised on construction: no leading
    /// slash, no trailing slash. Empty string is allowed and means
    /// "join directly under `base_url`".
    path_prefix: String,
    /// Shared reqwest client. Safe to clone across requests.
    http: Client,
    /// Cached downstream CDA Bearer token. Populated lazily on the
    /// first forwarded request that discovers CDA auth is enabled.
    auth_token: Arc<Mutex<Option<String>>>,
    /// Synchronous downstream CDA operations are bridged into the
    /// SOVD async start/poll contract by caching their final result
    /// under a generated execution id.
    executions: Arc<Mutex<HashMap<String, CdaExecutionRecord>>>,
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
        Self::new_with_remote_component_and_path_prefix(
            component_id.clone(),
            component_id,
            base_url,
            DEFAULT_CDA_PATH_PREFIX,
        )
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
        Self::new_with_remote_component_and_path_prefix(
            component_id.clone(),
            component_id,
            base_url,
            path_prefix,
        )
    }

    /// Build a [`CdaBackend`] with a distinct downstream component id.
    ///
    /// This is used by Phase 5 hybrid routing, where `OpenSOVD` should
    /// expose `cvc`/`fzc`/`rzc` locally while CDA speaks to generated
    /// bench MDD ids such as `cvc00000`.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`] if the underlying `reqwest` client
    /// cannot be constructed.
    pub fn new_with_remote_component_and_path_prefix(
        component_id: ComponentId,
        remote_component_id: ComponentId,
        base_url: Url,
        path_prefix: &str,
    ) -> Result<Self> {
        let base_url = ensure_trailing_slash(base_url);
        let http = Client::builder()
            .build()
            .map_err(|e| SovdError::Internal(format!("build reqwest client: {e}")))?;
        Ok(Self {
            component_id,
            remote_component_id,
            base_url,
            path_prefix: normalise_prefix(path_prefix),
            http,
            auth_token: Arc::new(Mutex::new(None)),
            executions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Construct a [`CdaBackend`] with a caller-supplied [`reqwest::Client`],
    /// mostly useful for tests that want to inject a custom transport or
    /// timeout profile. Uses [`DEFAULT_CDA_PATH_PREFIX`].
    #[must_use]
    pub fn with_client(component_id: ComponentId, base_url: Url, http: Client) -> Self {
        Self::with_client_and_remote_component_and_path_prefix(
            component_id.clone(),
            component_id,
            base_url,
            http,
            DEFAULT_CDA_PATH_PREFIX,
        )
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
        Self::with_client_and_remote_component_and_path_prefix(
            component_id.clone(),
            component_id,
            base_url,
            http,
            path_prefix,
        )
    }

    /// Construct a [`CdaBackend`] with a caller-supplied
    /// [`reqwest::Client`], explicit downstream REST path prefix, and a
    /// distinct downstream component id.
    #[must_use]
    pub fn with_client_and_remote_component_and_path_prefix(
        component_id: ComponentId,
        remote_component_id: ComponentId,
        base_url: Url,
        http: Client,
        path_prefix: &str,
    ) -> Self {
        let base_url = ensure_trailing_slash(base_url);
        Self {
            component_id,
            remote_component_id,
            base_url,
            path_prefix: normalise_prefix(path_prefix),
            http,
            auth_token: Arc::new(Mutex::new(None)),
            executions: Arc::new(Mutex::new(HashMap::new())),
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

    /// Borrow the downstream CDA component id this backend targets.
    #[must_use]
    pub fn remote_component_id(&self) -> &ComponentId {
        &self.remote_component_id
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
                return Err(SovdError::Transport(format!("CDA preflight {url}: {e}")));
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
            format!("components/{}/{}", self.remote_component_id, tail)
        } else {
            format!(
                "{}/components/{}/{}",
                self.path_prefix, self.remote_component_id, tail
            )
        };
        self.base_url
            .join(&joined)
            .map_err(|e| SovdError::InvalidRequest(format!("bad CDA URL: {e}")))
    }

    fn authorize_url(&self) -> Result<Url> {
        let joined = if self.path_prefix.is_empty() {
            "authorize".to_owned()
        } else {
            format!("{}/authorize", self.path_prefix)
        };
        self.base_url
            .join(&joined)
            .map_err(|e| SovdError::InvalidRequest(format!("bad CDA URL: {e}")))
    }

    async fn cached_auth_token(&self) -> Option<String> {
        self.auth_token.lock().await.clone()
    }

    async fn acquire_auth_token(&self) -> Result<String> {
        let url = self.authorize_url()?;
        let resp = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "client_id": CDA_FORWARD_AUTH_CLIENT_ID,
                "client_secret": CDA_FORWARD_AUTH_CLIENT_SECRET,
            }))
            .send()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?
            .error_for_status()
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?;
        let auth = resp
            .json::<CdaAuthBody>()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?;
        let mut guard = self.auth_token.lock().await;
        *guard = Some(auth.access_token.clone());
        Ok(auth.access_token)
    }

    fn faults_request(&self, url: Url, token: Option<&str>) -> reqwest::RequestBuilder {
        let request = self.http.get(url);
        match token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    fn empty_faults_list() -> ListOfFaults {
        ListOfFaults {
            items: Vec::new(),
            total: Some(0),
            next_page: None,
            schema: None,
            extras: None,
        }
    }

    async fn send_with_auth_retry_response<F>(&self, build: F) -> Result<reqwest::Response>
    where
        F: Fn(Option<&str>) -> reqwest::RequestBuilder,
    {
        let mut token = self.cached_auth_token().await;
        let mut auth_refreshed = false;
        loop {
            let response = build(token.as_deref())
                .send()
                .await
                .map_err(|e| map_reqwest_err(&self.component_id, &e))?;
            let status = response.status();
            if (status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN)
                && !auth_refreshed
            {
                token = Some(self.acquire_auth_token().await?);
                auth_refreshed = true;
                continue;
            }
            return Ok(response);
        }
    }

    async fn send_with_auth_retry<F>(&self, build: F) -> Result<reqwest::Response>
    where
        F: Fn(Option<&str>) -> reqwest::RequestBuilder,
    {
        self.send_with_auth_retry_response(build)
            .await?
            .error_for_status()
            .map_err(|e| map_reqwest_err(&self.component_id, &e))
    }

    fn failed_execution_status_from_http(
        &self,
        status: StatusCode,
        body: &[u8],
    ) -> ExecutionStatusResponse {
        let error = serde_json::from_slice::<GenericError>(body)
            .ok()
            .unwrap_or_else(|| {
                let message = String::from_utf8_lossy(body);
                GenericError {
                    error_code: "transport.error".to_owned(),
                    vendor_code: None,
                    message: if message.trim().is_empty() {
                        format!(
                            "CDA start_execution for component {} returned {} with an empty body",
                            self.component_id, status
                        )
                    } else {
                        format!(
                            "CDA start_execution for component {} returned {}: {}",
                            self.component_id,
                            status,
                            message.trim()
                        )
                    },
                    translation_id: None,
                    parameters: None,
                }
            });
        ExecutionStatusResponse {
            status: Some(ExecutionStatus::Failed),
            capability: Capability::Execute,
            parameters: None,
            schema: None,
            error: Some(vec![DataError {
                path: "/".to_owned(),
                error: Some(error),
            }]),
        }
    }

    async fn store_execution_status(
        &self,
        operation_id: &str,
        status: ExecutionStatusResponse,
    ) -> String {
        let execution_id = Uuid::new_v4().to_string();
        self.executions.lock().await.insert(
            execution_id.clone(),
            CdaExecutionRecord {
                operation_id: operation_id.to_owned(),
                status,
            },
        );
        execution_id
    }
}

fn rewrite_component_capabilities(
    capabilities: &mut EntityCapabilities,
    local_component_id: &ComponentId,
    remote_component_id: &ComponentId,
) {
    local_component_id.as_str().clone_into(&mut capabilities.id);
    if capabilities
        .name
        .eq_ignore_ascii_case(remote_component_id.as_str())
    {
        local_component_id
            .as_str()
            .clone_into(&mut capabilities.name);
    }

    for field in [
        &mut capabilities.configurations,
        &mut capabilities.bulk_data,
        &mut capabilities.data,
        &mut capabilities.data_lists,
        &mut capabilities.faults,
        &mut capabilities.operations,
        &mut capabilities.updates,
        &mut capabilities.modes,
        &mut capabilities.subareas,
        &mut capabilities.subcomponents,
        &mut capabilities.locks,
        &mut capabilities.depends_on,
        &mut capabilities.hosts,
        &mut capabilities.is_located_on,
        &mut capabilities.scripts,
        &mut capabilities.logs,
    ] {
        rewrite_component_href(field, local_component_id, remote_component_id);
    }
}

fn rewrite_component_href(
    href: &mut Option<String>,
    local_component_id: &ComponentId,
    remote_component_id: &ComponentId,
) {
    let Some(value) = href.as_deref() else {
        return;
    };

    let lower = value.to_ascii_lowercase();
    let marker = format!(
        "/components/{}",
        remote_component_id.as_str().to_ascii_lowercase()
    );
    let Some(marker_index) = lower.find(&marker) else {
        return;
    };
    let Some(suffix_start) = marker_index.checked_add(marker.len()) else {
        return;
    };
    let suffix = &value[suffix_start..];
    *href = Some(format!(
        "{LOCAL_COMPONENT_BASE_PATH_PREFIX}/{local_component_id}{suffix}"
    ));
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

        // ADR-0018 rule 2: retry transient 5xx / connection errors
        // with exponential backoff, capped at 3 attempts and 2 s
        // total elapsed. On budget exhaustion surface
        // SovdError::Degraded so the caller sees a soft-fail, never
        // a raw Transport propagating as a 5xx.
        let start = Instant::now();
        let mut backoff = CDA_INITIAL_BACKOFF;
        let mut last_status: Option<StatusCode> = None;
        let mut transient_attempts: u32 = 0;
        let mut auth_refreshed = false;
        let mut token = self.cached_auth_token().await;
        loop {
            let result = self
                .faults_request(url.clone(), token.as_deref())
                .send()
                .await;
            match result {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        return response
                            .json::<ListOfFaults>()
                            .await
                            .map_err(|e| map_reqwest_err(&self.component_id, &e));
                    }
                    if (status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN)
                        && !auth_refreshed
                    {
                        token = Some(self.acquire_auth_token().await?);
                        auth_refreshed = true;
                        continue;
                    }
                    if status == StatusCode::NOT_FOUND {
                        tracing::info!(
                            backend = "cda",
                            operation = "list_faults",
                            component_id = %self.component_id,
                            remote_component_id = %self.remote_component_id,
                            "CdaBackend fault list missing upstream SID 0x19 route; returning empty ListOfFaults"
                        );
                        return Ok(Self::empty_faults_list());
                    }
                    last_status = Some(status);
                    if status.is_server_error()
                        || status == StatusCode::REQUEST_TIMEOUT
                        || status == StatusCode::TOO_MANY_REQUESTS
                    {
                        tracing::warn!(
                            backend = "cda",
                            operation = "list_faults",
                            component_id = %self.component_id,
                            error_kind = "transient_http",
                            status = %status,
                            attempt = transient_attempts,
                            "CdaBackend transient failure; will retry within budget"
                        );
                    } else {
                        // Non-retryable (4xx) — map through the
                        // existing reqwest error path.
                        return response
                            .error_for_status()
                            .map_err(|e| map_reqwest_err(&self.component_id, &e))?
                            .json::<ListOfFaults>()
                            .await
                            .map_err(|e| map_reqwest_err(&self.component_id, &e));
                    }
                }
                Err(err) => {
                    if !is_retryable(&err) {
                        return Err(map_reqwest_err(&self.component_id, &err));
                    }
                    tracing::warn!(
                        backend = "cda",
                        operation = "list_faults",
                        component_id = %self.component_id,
                        error_kind = "transient_reqwest",
                        attempt = transient_attempts,
                        "CdaBackend transient reqwest error: {err}"
                    );
                }
            }
            transient_attempts = transient_attempts.saturating_add(1);
            if transient_attempts >= CDA_MAX_ATTEMPTS {
                break;
            }
            if start.elapsed().saturating_add(backoff) >= CDA_TOTAL_BUDGET {
                break;
            }
            tokio::time::sleep(backoff).await;
            backoff = backoff.saturating_mul(2);
        }
        let reason = last_status.map_or_else(
            || "cda retry budget exceeded".to_owned(),
            |s| format!("cda retry budget exceeded (last status {s})"),
        );
        tracing::warn!(
            backend = "cda",
            operation = "list_faults",
            component_id = %self.component_id,
            error_kind = "retry_budget_exhausted",
            "CdaBackend: {reason}"
        );
        Err(SovdError::Degraded { reason })
    }

    async fn clear_all_faults(&self) -> Result<()> {
        let url = self.component_url("faults")?;
        self.send_with_auth_retry(|token| {
            let request = self.http.delete(url.clone());
            match token {
                Some(token) => request.bearer_auth(token),
                None => request,
            }
        })
        .await?;
        Ok(())
    }

    async fn clear_fault(&self, code: &str) -> Result<()> {
        let url = self.component_url(&format!("faults/{code}"))?;
        self.send_with_auth_retry(|token| {
            let request = self.http.delete(url.clone());
            match token {
                Some(token) => request.bearer_auth(token),
                None => request,
            }
        })
        .await?;
        Ok(())
    }

    async fn start_execution(
        &self,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse> {
        let url = self.component_url(&format!("operations/{operation_id}/executions"))?;
        let resp = self
            .send_with_auth_retry_response(|token| {
                let request_builder = self.http.post(url.clone()).json(&request);
                match token {
                    Some(token) => request_builder.bearer_auth(token),
                    None => request_builder,
                }
            })
            .await?;
        let status = resp.status();
        let body = resp
            .bytes()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?;
        if status == StatusCode::NOT_FOUND {
            return Err(SovdError::NotFound {
                entity: format!("cda:{}:operation:{operation_id}", self.component_id),
            });
        }
        if status.is_server_error() || status == StatusCode::REQUEST_TIMEOUT {
            let execution_id = self
                .store_execution_status(
                    operation_id,
                    self.failed_execution_status_from_http(status, body.as_ref()),
                )
                .await;
            return Ok(StartExecutionAsyncResponse {
                id: execution_id,
                status: Some(ExecutionStatus::Running),
            });
        }
        if !status.is_success() {
            let text = String::from_utf8_lossy(body.as_ref());
            return Err(SovdError::Transport(format!(
                "CDA start_execution for component {} returned {}: {}",
                self.component_id,
                status,
                text.trim()
            )));
        }
        if body.is_empty() {
            let execution_id = self
                .store_execution_status(
                    operation_id,
                    ExecutionStatusResponse {
                        status: Some(ExecutionStatus::Completed),
                        capability: Capability::Execute,
                        parameters: None,
                        schema: None,
                        error: None,
                    },
                )
                .await;
            return Ok(StartExecutionAsyncResponse {
                id: execution_id,
                status: Some(ExecutionStatus::Running),
            });
        }

        if let Ok(async_started) = serde_json::from_slice::<StartExecutionAsyncResponse>(&body) {
            return Ok(async_started);
        }

        let sync_started =
            serde_json::from_slice::<CdaSyncExecutionResponse>(&body).map_err(|e| {
                SovdError::Transport(format!(
                    "CDA start_execution response decode for component {}: {e}",
                    self.component_id
                ))
            })?;
        let final_status = if sync_started.errors.is_empty() {
            ExecutionStatus::Completed
        } else {
            ExecutionStatus::Failed
        };
        let execution_id = self
            .store_execution_status(
                operation_id,
                ExecutionStatusResponse {
                    status: Some(final_status),
                    capability: Capability::Execute,
                    parameters: sync_started.parameters,
                    schema: None,
                    error: (!sync_started.errors.is_empty()).then_some(sync_started.errors),
                },
            )
            .await;
        Ok(StartExecutionAsyncResponse {
            id: execution_id,
            status: Some(ExecutionStatus::Running),
        })
    }

    async fn execution_status(
        &self,
        operation_id: &str,
        execution_id: &str,
    ) -> Result<ExecutionStatusResponse> {
        let guard = self.executions.lock().await;
        let Some(record) = guard.get(execution_id) else {
            return Err(SovdError::NotFound {
                entity: format!("execution \"{execution_id}\""),
            });
        };
        if record.operation_id != operation_id {
            return Err(SovdError::NotFound {
                entity: format!("execution \"{execution_id}\" of operation \"{operation_id}\""),
            });
        }
        Ok(record.status.clone())
    }

    async fn entity_capabilities(&self) -> Result<EntityCapabilities> {
        let joined = if self.path_prefix.is_empty() {
            format!("components/{}", self.remote_component_id)
        } else {
            format!(
                "{}/components/{}",
                self.path_prefix, self.remote_component_id
            )
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
        let mut capabilities = resp
            .json::<EntityCapabilities>()
            .await
            .map_err(|e| map_reqwest_err(&self.component_id, &e))?;
        if self.remote_component_id != self.component_id {
            rewrite_component_capabilities(
                &mut capabilities,
                &self.component_id,
                &self.remote_component_id,
            );
        }
        Ok(capabilities)
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode as HttpStatusCode, header::AUTHORIZATION},
        routing::get,
    };
    use tokio::net::TcpListener;

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
        let backend = CdaBackend::new_with_path_prefix(ComponentId::new("cvc"), url, "sovd/v1")
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
        for raw in [
            "/vehicle/v15",
            "vehicle/v15/",
            "/vehicle/v15/",
            "vehicle/v15",
        ] {
            let backend =
                CdaBackend::new_with_path_prefix(ComponentId::new("cvc"), url.clone(), raw)
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
        assert_eq!(backend.remote_component_id(), &ComponentId::new("cvc"));
        assert_eq!(backend.kind(), BackendKind::Cda);
    }

    #[test]
    fn component_url_uses_remote_component_when_configured() {
        let url = Url::parse("http://localhost:20002/").expect("parse");
        let backend = CdaBackend::new_with_remote_component_and_path_prefix(
            ComponentId::new("cvc"),
            ComponentId::new("cvc00000"),
            url,
            "vehicle/v15",
        )
        .expect("construct with remote component");
        let got = backend.component_url("faults").expect("join");
        assert_eq!(got.path(), "/vehicle/v15/components/cvc00000/faults");
    }

    #[tokio::test]
    async fn entity_capabilities_rewrite_remote_alias_to_local_paths() {
        async fn handler() -> Json<EntityCapabilities> {
            Json(EntityCapabilities {
                id: "cvc00000".to_owned(),
                name: "CVC00000".to_owned(),
                translation_id: None,
                variant: None,
                configurations: None,
                bulk_data: None,
                data: Some(
                    "http://localhost:20002/vehicle/v15/components/cvc00000/data".to_owned(),
                ),
                data_lists: None,
                faults: Some(
                    "http://localhost:20002/vehicle/v15/components/cvc00000/faults".to_owned(),
                ),
                operations: Some(
                    "http://localhost:20002/vehicle/v15/components/cvc00000/operations".to_owned(),
                ),
                updates: None,
                modes: None,
                subareas: None,
                subcomponents: None,
                locks: Some(
                    "http://localhost:20002/vehicle/v15/components/cvc00000/locks".to_owned(),
                ),
                depends_on: None,
                hosts: None,
                is_located_on: None,
                scripts: None,
                logs: None,
            })
        }

        let app = Router::new().route("/vehicle/v15/components/cvc00000", get(handler));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve mock CDA");
        });

        let backend = CdaBackend::new_with_remote_component_and_path_prefix(
            ComponentId::new("cvc"),
            ComponentId::new("cvc00000"),
            Url::parse(&format!("http://{addr}/")).expect("parse mock CDA URL"),
            "vehicle/v15",
        )
        .expect("construct aliased backend");
        let capabilities = backend
            .entity_capabilities()
            .await
            .expect("entity capabilities");

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
        assert_eq!(
            capabilities.locks.as_deref(),
            Some("/sovd/v1/components/cvc/locks")
        );

        handle.abort();
    }

    #[tokio::test]
    async fn list_faults_authorizes_after_initial_401() {
        use std::sync::{
            Arc as StdArc,
            atomic::{AtomicU32, Ordering},
        };

        use axum::{
            extract::State,
            response::{IntoResponse, Response},
            routing::post,
        };
        use sovd_interfaces::spec::fault::{Fault, ListOfFaults};

        #[derive(Clone)]
        struct AuthRetryState {
            authorize_calls: StdArc<AtomicU32>,
            fault_calls: StdArc<AtomicU32>,
        }

        async fn authorize(State(state): State<AuthRetryState>) -> Json<serde_json::Value> {
            state.authorize_calls.fetch_add(1, Ordering::SeqCst);
            Json(serde_json::json!({
                "access_token": "phase5-token",
                "token_type": "Bearer",
                "expires_in": 2_000_000_000u64,
            }))
        }

        async fn faults(State(state): State<AuthRetryState>, headers: HeaderMap) -> Response {
            state.fault_calls.fetch_add(1, Ordering::SeqCst);
            let Some(value) = headers.get(AUTHORIZATION) else {
                return HttpStatusCode::UNAUTHORIZED.into_response();
            };
            if value != "Bearer phase5-token" {
                return HttpStatusCode::FORBIDDEN.into_response();
            }
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
                total: Some(1),
                next_page: None,
                schema: None,
                extras: None,
            })
            .into_response()
        }

        let state = AuthRetryState {
            authorize_calls: StdArc::new(AtomicU32::new(0)),
            fault_calls: StdArc::new(AtomicU32::new(0)),
        };
        let app = Router::new()
            .route("/vehicle/v15/authorize", post(authorize))
            .route("/vehicle/v15/components/cvc/faults", get(faults))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve mock CDA");
        });

        let backend = CdaBackend::new(
            ComponentId::new("cvc"),
            Url::parse(&format!("http://{addr}/")).expect("parse mock CDA URL"),
        )
        .expect("construct backend");
        let list = backend
            .list_faults(FaultFilter::all())
            .await
            .expect("list_faults");

        assert_eq!(list.items.len(), 1);
        assert_eq!(state.authorize_calls.load(Ordering::SeqCst), 1);
        assert_eq!(state.fault_calls.load(Ordering::SeqCst), 2);

        let cached = backend
            .list_faults(FaultFilter::all())
            .await
            .expect("list_faults with cached token");
        assert_eq!(cached.items.len(), 1);
        assert_eq!(
            state.authorize_calls.load(Ordering::SeqCst),
            1,
            "cached token should avoid a second authorize round-trip",
        );
        assert_eq!(state.fault_calls.load(Ordering::SeqCst), 3);

        handle.abort();
    }

    #[tokio::test]
    async fn list_faults_maps_upstream_404_to_empty_list() {
        use axum::routing::get;

        async fn faults_missing() -> HttpStatusCode {
            HttpStatusCode::NOT_FOUND
        }

        let app = Router::new().route("/vehicle/v15/components/cvc/faults", get(faults_missing));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve mock CDA");
        });

        let backend = CdaBackend::new(
            ComponentId::new("cvc"),
            Url::parse(&format!("http://{addr}/")).expect("parse mock CDA URL"),
        )
        .expect("construct backend");
        let list = backend
            .list_faults(FaultFilter::all())
            .await
            .expect("404 should normalize to an empty fault list");

        assert!(list.items.is_empty());
        assert_eq!(list.total, Some(0));
        assert_eq!(list.next_page, None);
        assert_eq!(list.schema, None);
        assert_eq!(list.extras, None);

        handle.abort();
    }

    #[tokio::test]
    async fn start_execution_authorizes_after_initial_401() {
        use std::sync::{
            Arc as StdArc,
            atomic::{AtomicU32, Ordering},
        };

        use axum::{
            extract::State,
            response::{IntoResponse, Response},
            routing::post,
        };

        #[derive(Clone)]
        struct StartExecutionState {
            authorize_calls: StdArc<AtomicU32>,
            execution_calls: StdArc<AtomicU32>,
        }

        async fn authorize(State(state): State<StartExecutionState>) -> Json<serde_json::Value> {
            state.authorize_calls.fetch_add(1, Ordering::SeqCst);
            Json(serde_json::json!({
                "access_token": "phase5-token",
                "token_type": "Bearer",
                "expires_in": 2_000_000_000u64,
            }))
        }

        async fn start_execution(
            State(state): State<StartExecutionState>,
            headers: HeaderMap,
        ) -> Response {
            state.execution_calls.fetch_add(1, Ordering::SeqCst);
            let Some(value) = headers.get(AUTHORIZATION) else {
                return HttpStatusCode::UNAUTHORIZED.into_response();
            };
            if value != "Bearer phase5-token" {
                return HttpStatusCode::FORBIDDEN.into_response();
            }
            Json(StartExecutionAsyncResponse {
                id: "exec-1".to_owned(),
                status: Some(sovd_interfaces::spec::operation::ExecutionStatus::Running),
            })
            .into_response()
        }

        let state = StartExecutionState {
            authorize_calls: StdArc::new(AtomicU32::new(0)),
            execution_calls: StdArc::new(AtomicU32::new(0)),
        };
        let app = Router::new()
            .route("/vehicle/v15/authorize", post(authorize))
            .route(
                "/vehicle/v15/components/rzc/operations/motor_self_test/executions",
                post(start_execution),
            )
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve mock CDA");
        });

        let backend = CdaBackend::new(
            ComponentId::new("rzc"),
            Url::parse(&format!("http://{addr}/")).expect("parse mock CDA URL"),
        )
        .expect("construct backend");
        let started = backend
            .start_execution(
                "motor_self_test",
                StartExecutionRequest {
                    timeout: Some(30),
                    parameters: Some(serde_json::json!({ "mode": "quick" })),
                    proximity_response: None,
                },
            )
            .await
            .expect("start_execution");

        assert_eq!(started.id, "exec-1");
        assert_eq!(state.authorize_calls.load(Ordering::SeqCst), 1);
        assert_eq!(state.execution_calls.load(Ordering::SeqCst), 2);

        handle.abort();
    }

    #[tokio::test]
    async fn sync_execution_response_is_bridged_into_async_poll_contract() {
        use std::sync::{
            Arc as StdArc,
            atomic::{AtomicU32, Ordering},
        };

        use axum::{
            extract::State,
            response::{IntoResponse, Response},
            routing::post,
        };

        #[derive(Clone)]
        struct SyncExecutionState {
            authorize_calls: StdArc<AtomicU32>,
            execution_calls: StdArc<AtomicU32>,
        }

        async fn authorize(State(state): State<SyncExecutionState>) -> Json<serde_json::Value> {
            state.authorize_calls.fetch_add(1, Ordering::SeqCst);
            Json(serde_json::json!({
                "access_token": "phase5-token",
                "token_type": "Bearer",
                "expires_in": 2_000_000_000u64,
            }))
        }

        async fn start_execution(
            State(state): State<SyncExecutionState>,
            headers: HeaderMap,
        ) -> Response {
            state.execution_calls.fetch_add(1, Ordering::SeqCst);
            let Some(value) = headers.get(AUTHORIZATION) else {
                return HttpStatusCode::UNAUTHORIZED.into_response();
            };
            if value != "Bearer phase5-token" {
                return HttpStatusCode::FORBIDDEN.into_response();
            }
            Json(serde_json::json!({
                "parameters": {
                    "result": "passed",
                },
                "errors": [],
            }))
            .into_response()
        }

        let state = SyncExecutionState {
            authorize_calls: StdArc::new(AtomicU32::new(0)),
            execution_calls: StdArc::new(AtomicU32::new(0)),
        };
        let app = Router::new()
            .route("/vehicle/v15/authorize", post(authorize))
            .route(
                "/vehicle/v15/components/rzc/operations/motor_self_test/executions",
                post(start_execution),
            )
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve mock CDA");
        });

        let backend = CdaBackend::new(
            ComponentId::new("rzc"),
            Url::parse(&format!("http://{addr}/")).expect("parse mock CDA URL"),
        )
        .expect("construct backend");
        let started = backend
            .start_execution(
                "motor_self_test",
                StartExecutionRequest {
                    timeout: Some(30),
                    parameters: Some(serde_json::json!({ "mode": "quick" })),
                    proximity_response: None,
                },
            )
            .await
            .expect("start_execution");
        let status = backend
            .execution_status("motor_self_test", &started.id)
            .await
            .expect("execution_status");

        assert_eq!(started.status, Some(ExecutionStatus::Running));
        assert_eq!(status.status, Some(ExecutionStatus::Completed));
        assert_eq!(
            status.parameters,
            Some(serde_json::json!({
                "result": "passed",
            }))
        );
        assert_eq!(state.authorize_calls.load(Ordering::SeqCst), 1);
        assert_eq!(state.execution_calls.load(Ordering::SeqCst), 2);

        handle.abort();
    }

    #[tokio::test]
    async fn server_error_execution_is_bridged_into_failed_async_poll_contract() {
        use std::sync::{
            Arc as StdArc,
            atomic::{AtomicU32, Ordering},
        };

        use axum::{
            extract::State,
            response::{IntoResponse, Response},
            routing::post,
        };

        #[derive(Clone)]
        struct FailedExecutionState {
            authorize_calls: StdArc<AtomicU32>,
            execution_calls: StdArc<AtomicU32>,
        }

        async fn authorize(State(state): State<FailedExecutionState>) -> Json<serde_json::Value> {
            state.authorize_calls.fetch_add(1, Ordering::SeqCst);
            Json(serde_json::json!({
                "access_token": "phase5-token",
                "token_type": "Bearer",
                "expires_in": 2_000_000_000u64,
            }))
        }

        async fn start_execution(
            State(state): State<FailedExecutionState>,
            headers: HeaderMap,
        ) -> Response {
            state.execution_calls.fetch_add(1, Ordering::SeqCst);
            let Some(value) = headers.get(AUTHORIZATION) else {
                return HttpStatusCode::UNAUTHORIZED.into_response();
            };
            if value != "Bearer phase5-token" {
                return HttpStatusCode::FORBIDDEN.into_response();
            }
            (
                HttpStatusCode::GATEWAY_TIMEOUT,
                Json(GenericError {
                    error_code: "vendor-specific".to_owned(),
                    vendor_code: Some("gateway-timeout".to_owned()),
                    message: "Ecu [3] offline".to_owned(),
                    translation_id: None,
                    parameters: None,
                }),
            )
                .into_response()
        }

        let state = FailedExecutionState {
            authorize_calls: StdArc::new(AtomicU32::new(0)),
            execution_calls: StdArc::new(AtomicU32::new(0)),
        };
        let app = Router::new()
            .route("/vehicle/v15/authorize", post(authorize))
            .route(
                "/vehicle/v15/components/rzc/operations/motor_self_test/executions",
                post(start_execution),
            )
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve mock CDA");
        });

        let backend = CdaBackend::new(
            ComponentId::new("rzc"),
            Url::parse(&format!("http://{addr}/")).expect("parse mock CDA URL"),
        )
        .expect("construct backend");
        let started = backend
            .start_execution(
                "motor_self_test",
                StartExecutionRequest {
                    timeout: Some(30),
                    parameters: Some(serde_json::json!({ "mode": "quick" })),
                    proximity_response: None,
                },
            )
            .await
            .expect("start_execution");
        let status = backend
            .execution_status("motor_self_test", &started.id)
            .await
            .expect("execution_status");

        assert_eq!(started.status, Some(ExecutionStatus::Running));
        assert_eq!(status.status, Some(ExecutionStatus::Failed));
        assert_eq!(state.authorize_calls.load(Ordering::SeqCst), 1);
        assert_eq!(state.execution_calls.load(Ordering::SeqCst), 2);
        let errors = status.error.expect("failed status should include errors");
        assert!(!errors.is_empty(), "failed status should keep a data error");
        assert_eq!(errors[0].path, "/");
        assert_eq!(
            errors[0]
                .error
                .as_ref()
                .and_then(|error| error.vendor_code.as_deref()),
            Some("gateway-timeout")
        );

        handle.abort();
    }

    // --- D3-red: retry with bounded backoff ----------------------------
    //
    // ADR-0018 rule 2 requires `CdaBackend` to retry transient downstream
    // failures (5xx, connection reset) up to 3 times with exponential
    // backoff and a 2 s total budget before surfacing an error. On
    // success within the budget the caller should see a normal
    // `ListOfFaults`; on budget exhaustion the caller should see
    // `SovdError::Degraded` — never a raw `SovdError::Transport`.
    //
    // We drive this via a hand-rolled axum mock that counts calls and
    // returns 503 N times before a 200 (or 503 forever).

    use std::sync::{
        Arc as StdArc,
        atomic::{AtomicU32, Ordering},
    };

    async fn spin_up_flaky_cda(
        fail_times: u32,
    ) -> (Url, StdArc<AtomicU32>, tokio::task::JoinHandle<()>) {
        use axum::{
            Json, Router,
            extract::State,
            http::StatusCode as AxumStatus,
            response::{IntoResponse, Response},
            routing::get,
        };
        use sovd_interfaces::spec::fault::{Fault, ListOfFaults};
        use tokio::net::TcpListener;

        #[derive(Clone)]
        struct SharedCounter {
            calls: StdArc<AtomicU32>,
            fail_until: u32,
        }

        async fn handler(State(counter): State<SharedCounter>) -> Response {
            let n = counter.calls.fetch_add(1, Ordering::SeqCst);
            if n < counter.fail_until {
                return AxumStatus::SERVICE_UNAVAILABLE.into_response();
            }
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
            .into_response()
        }

        let calls = StdArc::new(AtomicU32::new(0));
        let shared = SharedCounter {
            calls: StdArc::clone(&calls),
            fail_until: fail_times,
        };
        let app = Router::new()
            .route(
                "/vehicle/v15/components/{component_id}/faults",
                get(handler),
            )
            .with_state(shared);
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let url = Url::parse(&format!("http://{addr}/")).expect("parse");
        (url, calls, handle)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn retry_within_budget_succeeds_after_transient_503s() {
        // Two 503s then a 200. ADR-0018 says the retry loop must
        // absorb the first two failures and surface the eventual 200
        // as a normal ListOfFaults.
        let (base, calls, handle) = spin_up_flaky_cda(2).await;
        let backend = CdaBackend::new(ComponentId::new("cvc"), base).expect("construct backend");
        let list = backend
            .list_faults(FaultFilter::all())
            .await
            .expect("list_faults should succeed within retry budget");
        assert_eq!(list.items.len(), 1);
        assert!(
            calls.load(Ordering::SeqCst) >= 3,
            "expected at least 3 mock calls (2 fails + 1 success), saw {}",
            calls.load(Ordering::SeqCst)
        );
        handle.abort();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn retry_budget_exhausted_returns_degraded() {
        // 5 consecutive 503s. ADR-0018 caps the retry loop at 3
        // attempts (or 2 s budget) — on exhaustion we must see
        // SovdError::Degraded, NOT a raw Transport surfacing as a 5xx.
        let (base, calls, handle) = spin_up_flaky_cda(u32::MAX).await;
        let backend = CdaBackend::new(ComponentId::new("cvc"), base).expect("construct backend");
        let err = backend
            .list_faults(FaultFilter::all())
            .await
            .expect_err("list_faults should fail after retry budget");
        match err {
            SovdError::Degraded { ref reason } => {
                assert!(
                    reason.to_lowercase().contains("retry")
                        || reason.to_lowercase().contains("budget"),
                    "expected degraded reason to mention retry/budget, got {reason:?}"
                );
            }
            other => panic!("expected SovdError::Degraded, got {other:?}"),
        }
        let seen = calls.load(Ordering::SeqCst);
        assert!(
            (2..=5).contains(&seen),
            "expected 2..=5 retry attempts, saw {seen}"
        );
        handle.abort();
    }
}
