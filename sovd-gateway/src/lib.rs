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

//! SOVD gateway — federated host routing for Eclipse `OpenSOVD`.
//!
//! `sovd-gateway` sits **one layer above** [`sovd_interfaces::traits::backend::SovdBackend`]:
//! where `SovdBackend` serves a single SOVD host (one component per
//! backend instance), `GatewayBackend` routes whole SOVD requests
//! across a fleet of SOVD-speaking hosts, each of which may in turn
//! expose many components.
//!
//! # Host kinds
//!
//! The gateway ships two concrete host implementations:
//!
//! - [`LocalHost`] wraps one in-process [`SovdBackend`]. Used by
//!   `sovd-main` to glue a locally-owned DFM into the gateway so the
//!   same process can expose both federated and native components.
//! - [`RemoteHost`] wraps a `reqwest::Client` aimed at a remote SOVD
//!   server (another `sovd-main` instance, or a native ISO 17978
//!   compliant device). It forwards requests over HTTP/REST using the
//!   spec path table from ADR-0015.
//!
//! # Routing
//!
//! Each host advertises a routing table: a set of component ids it can
//! answer for. [`Gateway::route`] uses that map to pick a single host
//! for a component-scoped request. For fan-out endpoints
//! (`GET /components`) the gateway queries every host in parallel and
//! dedupes by component id.
//!
//! # Configuration
//!
//! Routing lives in `opensovd-gateway.toml`. The format is:
//!
//! ```toml
//! [[host]]
//! name = "dfm-local"
//! kind = "local"                    # local | remote
//! components = ["dfm"]              # ids this host serves
//!
//! [[host]]
//! name = "zone-a"
//! kind = "remote"
//! address = "http://127.0.0.1:9001" # required for kind = "remote"
//! components = ["cvc", "fzc"]
//! ```
//!
//! Loading from TOML is done by [`GatewayConfig::from_toml_str`]. The
//! binary that hosts the gateway (`sovd-main` in Phase 4) is
//! responsible for instantiating the concrete host objects from the
//! config and registering them via [`Gateway::register_host`].

#![allow(clippy::doc_markdown)]
// ADR-0018 D7: deny expect_used in production backend code.
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use async_trait::async_trait;
use serde::Deserialize;
use sovd_interfaces::{
    ComponentId, SovdError,
    spec::{
        component::{DiscoveredEntities, EntityCapabilities, EntityReference},
        fault::{FaultDetails, FaultFilter, ListOfFaults},
        operation::{
            ExecutionStatusResponse, OperationsList, StartExecutionAsyncResponse,
            StartExecutionRequest,
        },
    },
    traits::backend::SovdBackend,
    types::error::Result,
};

pub mod remote;

pub use remote::RemoteHost;

/// A named SOVD host participating in the federated gateway.
///
/// Gateways hold a list of `GatewayHost` trait objects; the trait's
/// `components()` accessor declares which component ids the host can
/// answer for, and the dispatch methods forward single-component
/// requests to the right backing implementation.
#[async_trait]
pub trait GatewayHost: Send + Sync {
    /// Stable identifier for this host. Used only for logging / admin
    /// endpoints — the gateway does not route by host name.
    fn name(&self) -> &str;

    /// The set of component ids this host answers for. The gateway
    /// uses this to build its routing table.
    fn components(&self) -> Vec<ComponentId>;

    /// Forward `list_faults` to the host for `component`.
    async fn list_faults(
        &self,
        component: &ComponentId,
        filter: FaultFilter,
    ) -> Result<ListOfFaults>;

    /// Forward `get_fault` to the host for `component`.
    async fn get_fault(&self, component: &ComponentId, code: &str) -> Result<FaultDetails>;

    /// Forward `clear_all_faults`.
    async fn clear_all_faults(&self, component: &ComponentId) -> Result<()>;

    /// Forward `clear_fault`.
    async fn clear_fault(&self, component: &ComponentId, code: &str) -> Result<()>;

    /// Forward `list_operations`.
    async fn list_operations(&self, component: &ComponentId) -> Result<OperationsList>;

    /// Forward `start_execution`.
    async fn start_execution(
        &self,
        component: &ComponentId,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse>;

    /// Forward `execution_status`.
    async fn execution_status(
        &self,
        component: &ComponentId,
        operation_id: &str,
        execution_id: &str,
    ) -> Result<ExecutionStatusResponse>;

    /// Forward `entity_capabilities`.
    async fn entity_capabilities(&self, component: &ComponentId) -> Result<EntityCapabilities>;
}

/// In-process host backed by a single [`SovdBackend`] per component.
///
/// Holds an owned map of component id → trait object. The constructor
/// validates that each backend's `component_id()` matches the key it
/// was registered under.
pub struct LocalHost {
    name: String,
    backends: HashMap<ComponentId, Arc<dyn SovdBackend>>,
    /// Sorted list of component ids this host serves. Kept alongside
    /// the `HashMap` so [`GatewayHost::components`] returns a
    /// deterministic order for downstream aggregation.
    component_order: Vec<ComponentId>,
}

impl std::fmt::Debug for LocalHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalHost")
            .field("name", &self.name)
            .field("components", &self.backends.keys().collect::<Vec<_>>())
            .field("component_order", &self.component_order)
            .finish()
    }
}

impl LocalHost {
    /// Build a local host from a named list of backends.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::InvalidRequest`] if two backends share a
    /// component id.
    pub fn new(name: impl Into<String>, backends: Vec<Arc<dyn SovdBackend>>) -> Result<Self> {
        let mut map: HashMap<ComponentId, Arc<dyn SovdBackend>> = HashMap::new();
        let mut order = Vec::new();
        for backend in backends {
            let cid = backend.component_id();
            if map.contains_key(&cid) {
                return Err(SovdError::InvalidRequest(format!(
                    "LocalHost: duplicate component \"{cid}\""
                )));
            }
            order.push(cid.clone());
            map.insert(cid, backend);
        }
        order.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        Ok(Self {
            name: name.into(),
            backends: map,
            component_order: order,
        })
    }

    fn backend(&self, component: &ComponentId) -> Result<&Arc<dyn SovdBackend>> {
        self.backends
            .get(component)
            .ok_or_else(|| SovdError::NotFound {
                entity: format!("component \"{component}\" in host \"{}\"", self.name),
            })
    }
}

#[async_trait]
impl GatewayHost for LocalHost {
    fn name(&self) -> &str {
        &self.name
    }

    fn components(&self) -> Vec<ComponentId> {
        self.component_order.clone()
    }

    async fn list_faults(
        &self,
        component: &ComponentId,
        filter: FaultFilter,
    ) -> Result<ListOfFaults> {
        self.backend(component)?.list_faults(filter).await
    }

    async fn get_fault(&self, component: &ComponentId, code: &str) -> Result<FaultDetails> {
        self.backend(component)?.get_fault(code).await
    }

    async fn clear_all_faults(&self, component: &ComponentId) -> Result<()> {
        self.backend(component)?.clear_all_faults().await
    }

    async fn clear_fault(&self, component: &ComponentId, code: &str) -> Result<()> {
        self.backend(component)?.clear_fault(code).await
    }

    async fn list_operations(&self, component: &ComponentId) -> Result<OperationsList> {
        self.backend(component)?.list_operations().await
    }

    async fn start_execution(
        &self,
        component: &ComponentId,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse> {
        self.backend(component)?
            .start_execution(operation_id, request)
            .await
    }

    async fn execution_status(
        &self,
        component: &ComponentId,
        operation_id: &str,
        execution_id: &str,
    ) -> Result<ExecutionStatusResponse> {
        self.backend(component)?
            .execution_status(operation_id, execution_id)
            .await
    }

    async fn entity_capabilities(&self, component: &ComponentId) -> Result<EntityCapabilities> {
        self.backend(component)?.entity_capabilities().await
    }
}

/// Federated SOVD gateway.
///
/// Holds a list of hosts and builds an index from component id →
/// host name at registration time. `route_host` looks up that index on
/// every request; fan-out methods iterate the whole host list and
/// aggregate results.
#[derive(Default)]
pub struct Gateway {
    hosts: Vec<Arc<dyn GatewayHost>>,
    /// component id → index into `hosts`. Invariant: every value is a
    /// valid index into `hosts`; enforced by [`Gateway::register_host`].
    route_table: HashMap<ComponentId, usize>,
}

impl std::fmt::Debug for Gateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gateway")
            .field(
                "hosts",
                &self
                    .hosts
                    .iter()
                    .map(|h| h.name().to_owned())
                    .collect::<Vec<_>>(),
            )
            .field("route_table", &self.route_table)
            .finish()
    }
}

impl Gateway {
    /// Build an empty gateway. Register hosts with
    /// [`Gateway::register_host`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a host. Later-registered hosts do not override earlier
    /// hosts on component-id conflicts — the first-registered host
    /// wins, and a conflict is reported as
    /// [`SovdError::InvalidRequest`] so the operator notices.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::InvalidRequest`] if any of `host`'s
    /// components are already served by a previously-registered host.
    pub fn register_host(&mut self, host: Arc<dyn GatewayHost>) -> Result<()> {
        let new_index = self.hosts.len();
        for component in host.components() {
            if let Some(&existing) = self.route_table.get(&component) {
                let existing_name = self
                    .hosts
                    .get(existing)
                    .map_or_else(|| "<unknown>".to_owned(), |h| h.name().to_owned());
                return Err(SovdError::InvalidRequest(format!(
                    "Gateway: component \"{component}\" already served by host \"{existing_name}\""
                )));
            }
            self.route_table.insert(component, new_index);
        }
        self.hosts.push(host);
        Ok(())
    }

    /// Borrow the list of registered hosts.
    #[must_use]
    pub fn hosts(&self) -> &[Arc<dyn GatewayHost>] {
        &self.hosts
    }

    /// Look up the host serving `component`.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::NotFound`] if no host serves `component`.
    pub fn route(&self, component: &ComponentId) -> Result<&Arc<dyn GatewayHost>> {
        self.route_table
            .get(component)
            .and_then(|&idx| self.hosts.get(idx))
            .ok_or_else(|| SovdError::NotFound {
                entity: format!("gateway route for \"{component}\""),
            })
    }

    /// Forward `list_faults` through the routed host.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::NotFound`] if no host serves `component`;
    /// propagates whatever the host returns otherwise.
    pub async fn list_faults(
        &self,
        component: &ComponentId,
        filter: FaultFilter,
    ) -> Result<ListOfFaults> {
        self.route(component)?.list_faults(component, filter).await
    }

    /// Forward `get_fault` through the routed host.
    ///
    /// # Errors
    ///
    /// See [`list_faults`](Self::list_faults).
    pub async fn get_fault(&self, component: &ComponentId, code: &str) -> Result<FaultDetails> {
        self.route(component)?.get_fault(component, code).await
    }

    /// Forward `start_execution` through the routed host.
    ///
    /// # Errors
    ///
    /// See [`list_faults`](Self::list_faults).
    pub async fn start_execution(
        &self,
        component: &ComponentId,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse> {
        self.route(component)?
            .start_execution(component, operation_id, request)
            .await
    }

    /// Aggregate `GET /components` across every host. Dedupes by
    /// component id — if two hosts both claim the same component (which
    /// `register_host` should have rejected), the first-registered
    /// host's entry is kept.
    ///
    /// ADR-0018 rule 5: one dead host out of N must NOT poison the
    /// whole response. Unreachable hosts' components are captured in
    /// the aggregated response's `extras.host_unreachable` list and
    /// the `extras.stale` flag is set, but the live hosts' entries
    /// still come through.
    ///
    /// # Errors
    ///
    /// Never fails at the aggregation layer; individual host errors
    /// are swallowed and logged via `tracing::warn`. If every host
    /// errors, returns [`SovdError::Transport`] with a summary string.
    pub async fn list_components(&self) -> Result<DiscoveredEntities> {
        let mut items: Vec<EntityReference> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut any_success = false;
        let mut unreachable: Vec<ComponentId> = Vec::new();
        for host in &self.hosts {
            let fan_out = list_components_for_host(host.as_ref()).await;
            if !fan_out.live.is_empty() {
                any_success = true;
            }
            for reference in fan_out.live {
                if seen.insert(reference.id.clone()) {
                    items.push(reference);
                }
            }
            for cid in fan_out.unreachable {
                if !unreachable.contains(&cid) {
                    unreachable.push(cid);
                }
            }
        }
        // Legacy behaviour: if we had zero successes AND zero
        // unreachables (nothing was registered) the gateway is
        // simply empty; return nominal empty list.
        let last_error: Option<String> = if unreachable.is_empty() || any_success {
            None
        } else {
            Some(format!(
                "all hosts unreachable ({} components)",
                unreachable.len()
            ))
        };
        if !any_success && self.hosts.is_empty() {
            // Empty gateway is a valid configuration — return an empty
            // list rather than faulting.
            return Ok(DiscoveredEntities {
                items,
                extras: None,
            });
        }
        if !any_success {
            return Err(SovdError::Transport(format!(
                "every host failed list_components; last error: {}",
                last_error.unwrap_or_else(|| "unknown".to_owned())
            )));
        }
        items.sort_by(|a, b| a.id.cmp(&b.id));
        let extras = if unreachable.is_empty() {
            None
        } else {
            unreachable.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            Some(sovd_interfaces::extras::response::ResponseExtras::host_unreachable(unreachable))
        };
        Ok(DiscoveredEntities { items, extras })
    }
}

/// Per-host list_components fan-out result. Carries both the live
/// [`EntityReference`] rows the host returned and the component ids
/// the host failed to answer for — ADR-0018 rule 5 lets the caller
/// distinguish "host silent" from "host said no to a specific
/// component". Empty `live` + non-empty `unreachable` means the
/// entire host was unreachable.
struct HostFanOut {
    live: Vec<EntityReference>,
    unreachable: Vec<ComponentId>,
}

async fn list_components_for_host(host: &dyn GatewayHost) -> HostFanOut {
    let mut refs = Vec::new();
    let mut unreachable = Vec::new();
    for component in host.components() {
        match host.entity_capabilities(&component).await {
            Ok(caps) => refs.push(EntityReference {
                id: caps.id.clone(),
                name: caps.name,
                translation_id: caps.translation_id,
                href: format!("/sovd/v1/components/{}", caps.id),
                tags: None,
            }),
            Err(e) => {
                tracing::warn!(
                    backend = "gateway",
                    operation = "list_components",
                    host = host.name(),
                    component_id = %component,
                    error_kind = "entity_capabilities_failed",
                    "entity_capabilities failed: {e}"
                );
                unreachable.push(component);
            }
        }
    }
    HostFanOut {
        live: refs,
        unreachable,
    }
}

// --- config ------------------------------------------------------------

/// Gateway configuration parsed from `opensovd-gateway.toml`.
///
/// See the crate docs for the TOML shape. Parsing is strict: unknown
/// keys and unknown host kinds are a hard error.
#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    /// Ordered list of hosts. Registration order determines
    /// first-wins conflict resolution in [`Gateway::register_host`].
    #[serde(default, rename = "host")]
    pub hosts: Vec<GatewayHostConfig>,
}

/// One host entry from the TOML config.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayHostConfig {
    /// Stable, human-friendly name. Shown in logs and admin endpoints.
    pub name: String,
    /// Which kind of host this is (`"local"` or `"remote"`).
    pub kind: HostKind,
    /// Base URL of the remote SOVD server. Required when
    /// `kind == Remote`, rejected when `kind == Local`.
    #[serde(default)]
    pub address: Option<String>,
    /// Components this host answers for.
    pub components: Vec<String>,
}

/// Host kind discriminator in the TOML config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    /// In-process host — backed by a local [`SovdBackend`].
    Local,
    /// Remote host — forwarded over HTTP via [`RemoteHost`].
    Remote,
}

impl GatewayConfig {
    /// Parse a [`GatewayConfig`] from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::InvalidRequest`] if the TOML is malformed,
    /// uses unknown keys, or has an internal inconsistency such as a
    /// remote host without an `address`.
    pub fn from_toml_str(s: &str) -> Result<Self> {
        let parsed: Self = toml::from_str(s)
            .map_err(|e| SovdError::InvalidRequest(format!("gateway TOML: {e}")))?;
        parsed.validate()?;
        Ok(parsed)
    }

    fn validate(&self) -> Result<()> {
        for host in &self.hosts {
            match host.kind {
                HostKind::Remote => {
                    if host.address.is_none() {
                        return Err(SovdError::InvalidRequest(format!(
                            "gateway host \"{}\": kind=\"remote\" requires address",
                            host.name
                        )));
                    }
                }
                HostKind::Local => {
                    if host.address.is_some() {
                        return Err(SovdError::InvalidRequest(format!(
                            "gateway host \"{}\": kind=\"local\" does not accept address",
                            host.name
                        )));
                    }
                }
            }
            if host.components.is_empty() {
                return Err(SovdError::InvalidRequest(format!(
                    "gateway host \"{}\": components list must not be empty",
                    host.name
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use sovd_interfaces::{
        traits::backend::{BackendKind, SovdBackend},
        types::error::Result as SovdResult,
    };

    use super::*;

    // --- test doubles ---------------------------------------------------

    struct MockBackend {
        id: ComponentId,
    }

    #[async_trait]
    impl SovdBackend for MockBackend {
        fn component_id(&self) -> ComponentId {
            self.id.clone()
        }
        fn kind(&self) -> BackendKind {
            BackendKind::NativeSovd
        }
        async fn list_faults(&self, _filter: FaultFilter) -> SovdResult<ListOfFaults> {
            Ok(ListOfFaults {
                items: Vec::new(),
                total: None,
                next_page: None,
                schema: None,
                extras: None,
            })
        }
        async fn clear_all_faults(&self) -> SovdResult<()> {
            Ok(())
        }
        async fn clear_fault(&self, _code: &str) -> SovdResult<()> {
            Ok(())
        }
        async fn start_execution(
            &self,
            _operation_id: &str,
            _request: StartExecutionRequest,
        ) -> SovdResult<StartExecutionAsyncResponse> {
            Ok(StartExecutionAsyncResponse {
                id: "exec-mock".into(),
                status: None,
            })
        }
        async fn entity_capabilities(&self) -> SovdResult<EntityCapabilities> {
            Ok(EntityCapabilities {
                id: self.id.as_str().to_owned(),
                name: format!("mock:{}", self.id),
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
    }

    fn mock_backend(id: &str) -> Arc<dyn SovdBackend> {
        Arc::new(MockBackend {
            id: ComponentId::new(id),
        })
    }

    // --- tests ----------------------------------------------------------

    #[tokio::test]
    async fn local_host_advertises_its_components() {
        let host =
            LocalHost::new("dfm-local", vec![mock_backend("dfm"), mock_backend("cvc")]).unwrap();
        let mut ids: Vec<String> = host
            .components()
            .iter()
            .map(|c| c.as_str().to_owned())
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["cvc", "dfm"]);
    }

    #[tokio::test]
    async fn local_host_rejects_duplicate_components() {
        let err =
            LocalHost::new("dup", vec![mock_backend("dfm"), mock_backend("dfm")]).unwrap_err();
        assert!(matches!(err, SovdError::InvalidRequest(_)), "{err:?}");
    }

    #[tokio::test]
    async fn gateway_routes_by_component_id() {
        let mut gateway = Gateway::new();
        gateway
            .register_host(Arc::new(
                LocalHost::new("h1", vec![mock_backend("a"), mock_backend("b")]).unwrap(),
            ))
            .unwrap();
        gateway
            .register_host(Arc::new(
                LocalHost::new("h2", vec![mock_backend("c")]).unwrap(),
            ))
            .unwrap();
        assert_eq!(gateway.route(&ComponentId::new("a")).unwrap().name(), "h1");
        assert_eq!(gateway.route(&ComponentId::new("c")).unwrap().name(), "h2");
        match gateway.route(&ComponentId::new("missing")) {
            Err(SovdError::NotFound { .. }) => {}
            Err(other) => panic!("expected NotFound, got Err({other:?})"),
            Ok(_) => panic!("expected NotFound, got Ok"),
        }
    }

    #[tokio::test]
    async fn gateway_rejects_component_conflict_between_hosts() {
        let mut gateway = Gateway::new();
        gateway
            .register_host(Arc::new(
                LocalHost::new("h1", vec![mock_backend("a")]).unwrap(),
            ))
            .unwrap();
        let err = gateway
            .register_host(Arc::new(
                LocalHost::new("h2", vec![mock_backend("a")]).unwrap(),
            ))
            .unwrap_err();
        assert!(
            matches!(err, SovdError::InvalidRequest(ref m) if m.contains("already served")),
            "{err:?}"
        );
    }

    // D5-red: ADR-0018 rule 5 mandates half-open handling in the
    // RemoteHost fan-out: one dead host out of N must NOT poison the
    // whole `GET /components` response. The live hosts' entries come
    // through, and the dead host's component ids are captured as
    // status: "host-unreachable" in the aggregated response extras.
    //
    // The test uses two in-process LocalHost fakes wired through the
    // GatewayHost trait. One fake panics on entity_capabilities —
    // simulating a remote host becoming unreachable — the other
    // answers normally. Gateway::list_components should see the
    // live one and mark the dead one's component in extras.host_unreachable.

    struct DeadHost {
        name: String,
        components: Vec<ComponentId>,
    }

    #[async_trait]
    impl GatewayHost for DeadHost {
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
        ) -> SovdResult<ListOfFaults> {
            Err(SovdError::HostUnreachable {
                component_id: component.clone(),
            })
        }
        async fn get_fault(
            &self,
            component: &ComponentId,
            _code: &str,
        ) -> SovdResult<FaultDetails> {
            Err(SovdError::HostUnreachable {
                component_id: component.clone(),
            })
        }
        async fn clear_all_faults(&self, component: &ComponentId) -> SovdResult<()> {
            Err(SovdError::HostUnreachable {
                component_id: component.clone(),
            })
        }
        async fn clear_fault(&self, component: &ComponentId, _code: &str) -> SovdResult<()> {
            Err(SovdError::HostUnreachable {
                component_id: component.clone(),
            })
        }
        async fn list_operations(&self, component: &ComponentId) -> SovdResult<OperationsList> {
            Err(SovdError::HostUnreachable {
                component_id: component.clone(),
            })
        }
        async fn start_execution(
            &self,
            component: &ComponentId,
            _operation_id: &str,
            _request: StartExecutionRequest,
        ) -> SovdResult<StartExecutionAsyncResponse> {
            Err(SovdError::HostUnreachable {
                component_id: component.clone(),
            })
        }
        async fn execution_status(
            &self,
            component: &ComponentId,
            _operation_id: &str,
            _execution_id: &str,
        ) -> SovdResult<ExecutionStatusResponse> {
            Err(SovdError::HostUnreachable {
                component_id: component.clone(),
            })
        }
        async fn entity_capabilities(
            &self,
            component: &ComponentId,
        ) -> SovdResult<EntityCapabilities> {
            Err(SovdError::HostUnreachable {
                component_id: component.clone(),
            })
        }
    }

    #[tokio::test]
    async fn list_components_survives_one_dead_host() {
        let mut gateway = Gateway::new();
        gateway
            .register_host(Arc::new(
                LocalHost::new("live", vec![mock_backend("cvc")]).unwrap(),
            ))
            .unwrap();
        gateway
            .register_host(Arc::new(DeadHost {
                name: "dead".into(),
                // Arbitrary IDs for the unreachable-host test; intentionally
                // not an active bench ECU (ADR-0023 keeps only cvc/sc/bcm).
                components: vec![ComponentId::new("ghost_a"), ComponentId::new("ghost_b")],
            }))
            .unwrap();
        let discovered = gateway
            .list_components()
            .await
            .expect("list_components must not fail on partial outage");
        let ids: Vec<String> = discovered.items.iter().map(|r| r.id.clone()).collect();
        assert_eq!(ids, vec!["cvc"], "live host's components must come through");
        let extras = discovered
            .extras
            .as_ref()
            .expect("extras must be set when a host is unreachable");
        let unreachable_raw = extras
            .host_unreachable
            .as_ref()
            .expect("host_unreachable list must be populated");
        let mut unreachable_sorted = unreachable_raw.clone();
        unreachable_sorted.sort();
        assert_eq!(
            unreachable_sorted,
            vec!["ghost_a".to_owned(), "ghost_b".to_owned()],
            "dead host's components must be reported in host_unreachable"
        );
        assert!(extras.stale, "stale flag must be set on partial outage");
    }

    #[tokio::test]
    async fn gateway_list_components_aggregates_and_dedupes() {
        let mut gateway = Gateway::new();
        gateway
            .register_host(Arc::new(
                LocalHost::new("h1", vec![mock_backend("b"), mock_backend("a")]).unwrap(),
            ))
            .unwrap();
        gateway
            .register_host(Arc::new(
                LocalHost::new("h2", vec![mock_backend("c")]).unwrap(),
            ))
            .unwrap();
        let discovered = gateway.list_components().await.unwrap();
        let ids: Vec<String> = discovered.items.iter().map(|r| r.id.clone()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn gateway_forwards_list_faults_to_routed_host() {
        let mut gateway = Gateway::new();
        gateway
            .register_host(Arc::new(
                LocalHost::new("h1", vec![mock_backend("dfm")]).unwrap(),
            ))
            .unwrap();
        let list = gateway
            .list_faults(&ComponentId::new("dfm"), FaultFilter::all())
            .await
            .unwrap();
        assert!(list.items.is_empty());
    }

    #[test]
    fn parse_config_happy_path() {
        let toml = r#"
            [[host]]
            name = "local-dfm"
            kind = "local"
            components = ["dfm"]

            [[host]]
            name = "remote-zone-a"
            kind = "remote"
            address = "http://127.0.0.1:9001"
            components = ["cvc", "fzc"]
        "#;
        let cfg = GatewayConfig::from_toml_str(toml).expect("parse");
        assert_eq!(cfg.hosts.len(), 2);
        assert_eq!(cfg.hosts.first().unwrap().kind, HostKind::Local);
        assert_eq!(cfg.hosts.get(1).unwrap().kind, HostKind::Remote);
    }

    #[test]
    fn parse_config_rejects_remote_without_address() {
        let toml = r#"
            [[host]]
            name = "bad-remote"
            kind = "remote"
            components = ["x"]
        "#;
        let err = GatewayConfig::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, SovdError::InvalidRequest(ref m) if m.contains("requires address")));
    }

    #[test]
    fn parse_config_rejects_local_with_address() {
        let toml = r#"
            [[host]]
            name = "bad-local"
            kind = "local"
            address = "http://x"
            components = ["x"]
        "#;
        let err = GatewayConfig::from_toml_str(toml).unwrap_err();
        assert!(
            matches!(err, SovdError::InvalidRequest(ref m) if m.contains("does not accept address"))
        );
    }

    #[test]
    fn parse_config_rejects_empty_components() {
        let toml = r#"
            [[host]]
            name = "bad"
            kind = "local"
            components = []
        "#;
        let err = GatewayConfig::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, SovdError::InvalidRequest(ref m) if m.contains("must not be empty")));
    }

    #[test]
    fn parse_config_rejects_unknown_keys() {
        let toml = r#"
            [[host]]
            name = "bad"
            kind = "local"
            components = ["x"]
            nonsense = 42
        "#;
        assert!(GatewayConfig::from_toml_str(toml).is_err());
    }
}
