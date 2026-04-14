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

//! In-memory [`SovdServer`] implementation for the Phase 0 / Phase 1 MVP.
//!
//! This module exists so the rest of `opensovd-core` can exercise the full
//! typed SOVD request/response path end-to-end before the real DFM / MDD /
//! CDA backends land in Phase 3/4. It is deliberately boring: it holds a
//! fixed roster of demo components (`cvc`, `fzc`, `rzc` — the Taktflow
//! multi-board layout) in a `HashMap` behind a `RwLock`, and every trait
//! method reads canned data out of that map.
//!
//! # Role split vs. [`sovd_interfaces::traits::server::SovdServer`]
//!
//! The spec trait is per-component: one `SovdServer` serves one entity at a
//! time, and system-wide multiplexing is
//! [`SovdGateway`](sovd_interfaces::traits::gateway::SovdGateway)'s job.
//! `InMemoryServer` is a multi-component demo store, so it does **not**
//! implement `SovdServer` directly. Instead it hands out per-component
//! [`InMemoryComponentServer`] views via
//! [`InMemoryServer::component_server`]; those views implement the trait.
//!
//! The axum routes in [`crate::routes`] take an
//! `axum::extract::State<Arc<InMemoryServer>>`, read the `component-id`
//! from the path, and call `component_server(...)` to dispatch.
//!
//! All canned data lives in exactly one place —
//! [`InMemoryServer::new_with_demo_data`] — so individual route handlers
//! never embed literal fault codes or operation ids of their own.

use std::collections::HashMap;
use std::sync::Arc;

use sovd_interfaces::{
    ComponentId, SovdError,
    spec::{
        component::{DiscoveredEntities, EntityCapabilities, EntityReference},
        data::ReadValue,
        fault::{Fault, FaultDetails, FaultFilter, ListOfFaults},
        operation::{
            Capability, ExecutionStatus, ExecutionStatusResponse, OperationDescription,
            OperationsList, StartExecutionAsyncResponse, StartExecutionRequest,
        },
    },
    traits::server::SovdServer,
    types::error::Result,
};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Base URI used when building demo `href` fields. Kept relative so the
/// client can combine it with whatever host it reaches us on.
const BASE_URI: &str = "/sovd/v1";

/// One execution record held in memory.
#[derive(Debug, Clone)]
struct ExecutionRecord {
    /// Which operation this execution belongs to.
    operation_id: String,
    /// Current lifecycle status.
    status: ExecutionStatus,
    /// Parameters supplied at `POST` time, echoed back on `GET` for demo
    /// visibility.
    parameters: Option<serde_json::Value>,
}

/// In-memory state for one component.
#[derive(Debug, Clone)]
struct ComponentState {
    /// Entity capabilities served from `GET /components/{id}`.
    capabilities: EntityCapabilities,
    /// Current fault set served from `GET /components/{id}/faults`.
    faults: Vec<Fault>,
    /// Environment data per fault, indexed by fault code.
    fault_environments: HashMap<String, serde_json::Value>,
    /// Operation catalog served from `GET /components/{id}/operations`.
    operations: Vec<OperationDescription>,
    /// Active / historical executions keyed by execution id.
    executions: HashMap<String, ExecutionRecord>,
    /// Simple data store for `GET /components/{id}/data/{data-id}`.
    data_values: HashMap<String, serde_json::Value>,
}

impl ComponentState {
    fn entity_reference(&self) -> EntityReference {
        EntityReference {
            id: self.capabilities.id.clone(),
            name: self.capabilities.name.clone(),
            translation_id: self.capabilities.translation_id.clone(),
            href: format!("{BASE_URI}/components/{}", self.capabilities.id),
            tags: None,
        }
    }
}

/// Multi-component in-memory SOVD demo store.
///
/// Construct with [`InMemoryServer::new_with_demo_data`] to get the three
/// pre-populated Taktflow components (`cvc`, `fzc`, `rzc`). Obtain a
/// per-component trait view with [`InMemoryServer::component_server`].
#[derive(Debug, Clone)]
pub struct InMemoryServer {
    components: Arc<RwLock<HashMap<ComponentId, ComponentState>>>,
}

impl InMemoryServer {
    /// Build an empty server with no components. Mostly useful for tests
    /// that want to populate state by hand.
    #[must_use]
    pub fn new_empty() -> Self {
        Self {
            components: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Build an in-memory server pre-populated with three demo components
    /// matching the Taktflow layout (Central Vehicle Controller, Front Zone
    /// Controller, Rear Zone Controller).
    #[must_use]
    pub fn new_with_demo_data() -> Self {
        let mut components: HashMap<ComponentId, ComponentState> = HashMap::new();

        components.insert(
            ComponentId::new("cvc"),
            demo_component(
                "cvc",
                "Central Vehicle Controller",
                &[
                    demo_fault("P0A1F", "HV battery contactor welded", 2, "active"),
                    demo_fault("P0562", "System voltage low", 3, "pending"),
                ],
                &[
                    demo_op("motor_self_test", "Motor self test", true),
                    demo_op("hv_precharge", "HV precharge routine", true),
                    demo_op("read_vin", "Read VIN", false),
                ],
                &[
                    ("vin", serde_json::json!("WDD2031411F123456")),
                    (
                        "battery_voltage",
                        serde_json::json!({"value": 12.8f64, "unit": "V"}),
                    ),
                ],
            ),
        );

        components.insert(
            ComponentId::new("fzc"),
            demo_component(
                "fzc",
                "Front Zone Controller",
                &[demo_fault(
                    "U0100",
                    "Lost communication with ECU",
                    2,
                    "active",
                )],
                &[
                    demo_op("relay_self_test", "Relay self test", true),
                    demo_op("read_vin", "Read VIN", false),
                ],
                &[("vin", serde_json::json!("WDD2031411F123456"))],
            ),
        );

        components.insert(
            ComponentId::new("rzc"),
            demo_component(
                "rzc",
                "Rear Zone Controller",
                &[],
                &[demo_op("relay_self_test", "Relay self test", true)],
                &[],
            ),
        );

        Self {
            components: Arc::new(RwLock::new(components)),
        }
    }

    /// Return a per-component [`SovdServer`] view for `component`.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::NotFound`] if the component is not registered.
    pub async fn component_server(
        &self,
        component: &ComponentId,
    ) -> Result<InMemoryComponentServer> {
        let guard = self.components.read().await;
        if guard.contains_key(component) {
            Ok(InMemoryComponentServer {
                component: component.clone(),
                store: Arc::clone(&self.components),
            })
        } else {
            Err(SovdError::NotFound {
                entity: format!("component \"{component}\""),
            })
        }
    }

    /// `GET /sovd/v1/components` — list every registered entity.
    ///
    /// # Errors
    ///
    /// Never fails for the in-memory store (the `Result` is for trait
    /// parity with real backends that may fail).
    pub async fn list_entities(&self) -> Result<DiscoveredEntities> {
        let guard = self.components.read().await;
        let mut items: Vec<EntityReference> = guard
            .values()
            .map(ComponentState::entity_reference)
            .collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(DiscoveredEntities { items })
    }
}

impl Default for InMemoryServer {
    fn default() -> Self {
        Self::new_with_demo_data()
    }
}

/// Per-component view over the in-memory store. Implements
/// [`SovdServer`] for exactly one [`ComponentId`].
#[derive(Debug, Clone)]
pub struct InMemoryComponentServer {
    component: ComponentId,
    store: Arc<RwLock<HashMap<ComponentId, ComponentState>>>,
}

impl InMemoryComponentServer {
    /// Borrow the component id this view is bound to.
    #[must_use]
    pub fn component_id(&self) -> &ComponentId {
        &self.component
    }

    async fn with_state<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&ComponentState) -> Result<T>,
    {
        let guard = self.store.read().await;
        let state = guard
            .get(&self.component)
            .ok_or_else(|| SovdError::NotFound {
                entity: format!("component \"{}\"", self.component),
            })?;
        f(state)
    }

    async fn with_state_mut<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut ComponentState) -> Result<T>,
    {
        let mut guard = self.store.write().await;
        let state = guard
            .get_mut(&self.component)
            .ok_or_else(|| SovdError::NotFound {
                entity: format!("component \"{}\"", self.component),
            })?;
        f(state)
    }
}

impl SovdServer for InMemoryComponentServer {
    async fn list_faults(&self, filter: FaultFilter) -> Result<ListOfFaults> {
        self.with_state(|state| {
            let items = state
                .faults
                .iter()
                .filter(|fault| matches_filter(fault, &filter))
                .cloned()
                .collect();
            Ok(ListOfFaults {
                items,
                schema: None,
            })
        })
        .await
    }

    async fn get_fault(&self, code: &str) -> Result<FaultDetails> {
        self.with_state(|state| {
            let fault = state
                .faults
                .iter()
                .find(|f| f.code == code)
                .cloned()
                .ok_or_else(|| SovdError::NotFound {
                    entity: format!("fault \"{code}\""),
                })?;
            let environment_data = state.fault_environments.get(code).cloned();
            Ok(FaultDetails {
                item: fault,
                environment_data,
                errors: None,
                schema: None,
            })
        })
        .await
    }

    async fn clear_all_faults(&self) -> Result<()> {
        self.with_state_mut(|state| {
            state.faults.clear();
            state.fault_environments.clear();
            Ok(())
        })
        .await
    }

    async fn clear_fault(&self, code: &str) -> Result<()> {
        self.with_state_mut(|state| {
            let before = state.faults.len();
            state.faults.retain(|f| f.code != code);
            if state.faults.len() == before {
                return Err(SovdError::NotFound {
                    entity: format!("fault \"{code}\""),
                });
            }
            state.fault_environments.remove(code);
            Ok(())
        })
        .await
    }

    async fn start_execution(
        &self,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse> {
        let op_id = operation_id.to_owned();
        self.with_state_mut(|state| {
            if !state.operations.iter().any(|o| o.id == op_id) {
                return Err(SovdError::NotFound {
                    entity: format!("operation \"{op_id}\""),
                });
            }
            let exec_id = Uuid::new_v4().to_string();
            state.executions.insert(
                exec_id.clone(),
                ExecutionRecord {
                    operation_id: op_id.clone(),
                    status: ExecutionStatus::Running,
                    parameters: request.parameters,
                },
            );
            Ok(StartExecutionAsyncResponse {
                id: exec_id,
                status: Some(ExecutionStatus::Running),
            })
        })
        .await
    }

    async fn execution_status(
        &self,
        operation_id: &str,
        execution_id: &str,
    ) -> Result<ExecutionStatusResponse> {
        let op_id = operation_id.to_owned();
        let exec_id = execution_id.to_owned();
        self.with_state(|state| {
            let record = state
                .executions
                .get(&exec_id)
                .ok_or_else(|| SovdError::NotFound {
                    entity: format!("execution \"{exec_id}\""),
                })?;
            if record.operation_id != op_id {
                return Err(SovdError::NotFound {
                    entity: format!("execution \"{exec_id}\" of operation \"{op_id}\""),
                });
            }
            Ok(ExecutionStatusResponse {
                status: Some(record.status),
                capability: Capability::Execute,
                parameters: record.parameters.clone(),
                schema: None,
                error: None,
            })
        })
        .await
    }

    async fn read_data(&self, data_id: &str) -> Result<ReadValue> {
        let id = data_id.to_owned();
        self.with_state(|state| {
            let data = state
                .data_values
                .get(&id)
                .cloned()
                .ok_or_else(|| SovdError::NotFound {
                    entity: format!("data \"{id}\""),
                })?;
            Ok(ReadValue {
                id,
                data,
                errors: None,
                schema: None,
            })
        })
        .await
    }

    async fn entity_capabilities(&self) -> Result<EntityCapabilities> {
        self.with_state(|state| Ok(state.capabilities.clone()))
            .await
    }
}

/// List the operations available on one component (`GET .../operations`).
///
/// This is not on the per-component [`SovdServer`] trait — the spec's
/// "list operations" endpoint is covered by
/// [`SovdServer::entity_capabilities`] linking to the operations sub-
/// collection. We still expose it as an inherent method on the view so
/// the route handler has a typed entry point.
impl InMemoryComponentServer {
    /// Return the operation catalog for this component.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::NotFound`] if the component disappeared from
    /// the store between view creation and this call.
    pub async fn list_operations(&self) -> Result<OperationsList> {
        self.with_state(|state| {
            Ok(OperationsList {
                items: state.operations.clone(),
                schema: None,
            })
        })
        .await
    }
}

/// Best-effort `FaultFilter` evaluation for in-memory demo data.
///
/// Implements exactly what the spec requires: a fault matches if (a) its
/// `severity` is strictly below any configured threshold, (b) its `scope`
/// matches when a scope filter is set, and (c) at least one of the
/// status-key pairs matches when status filters are set.
fn matches_filter(fault: &Fault, filter: &FaultFilter) -> bool {
    if let Some(limit) = filter.severity {
        match fault.severity {
            Some(sev) if sev < limit => {}
            _ => return false,
        }
    }
    if let Some(scope) = &filter.scope {
        match &fault.scope {
            Some(fault_scope) if fault_scope == scope => {}
            _ => return false,
        }
    }
    if !filter.status_keys.is_empty() {
        let Some(serde_json::Value::Object(status)) = fault.status.as_ref() else {
            return false;
        };
        let any_match = filter.status_keys.iter().any(|(key, value)| {
            status
                .get(key)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|candidate| candidate == value)
        });
        if !any_match {
            return false;
        }
    }
    true
}

// ---- demo-data factories ----

fn demo_component(
    id: &str,
    name: &str,
    faults: &[Fault],
    operations: &[OperationDescription],
    data: &[(&str, serde_json::Value)],
) -> ComponentState {
    let capabilities = EntityCapabilities {
        id: id.to_owned(),
        name: name.to_owned(),
        translation_id: None,
        variant: None,
        configurations: None,
        bulk_data: None,
        data: Some(format!("{BASE_URI}/components/{id}/data")),
        data_lists: None,
        faults: Some(format!("{BASE_URI}/components/{id}/faults")),
        operations: Some(format!("{BASE_URI}/components/{id}/operations")),
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
    };

    let fault_environments = faults
        .iter()
        .map(|f| {
            (
                f.code.clone(),
                serde_json::json!({
                    "id": "env_data",
                    "data": {
                        "battery_voltage": 12.8f64,
                        "occurrence_counter": 1i32,
                    },
                }),
            )
        })
        .collect();

    let data_values = data
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect();

    ComponentState {
        capabilities,
        faults: faults.to_vec(),
        fault_environments,
        operations: operations.to_vec(),
        executions: HashMap::new(),
        data_values,
    }
}

fn demo_fault(code: &str, name: &str, severity: i32, aggregated_status: &str) -> Fault {
    Fault {
        code: code.to_owned(),
        scope: Some("Default".to_owned()),
        display_code: Some(code.to_owned()),
        fault_name: name.to_owned(),
        fault_translation_id: None,
        severity: Some(severity),
        status: Some(serde_json::json!({
            "aggregatedStatus": aggregated_status,
            "confirmedDTC": "1",
        })),
        symptom: None,
        symptom_translation_id: None,
        tags: None,
    }
}

fn demo_op(id: &str, name: &str, asynchronous: bool) -> OperationDescription {
    OperationDescription {
        id: id.to_owned(),
        name: Some(name.to_owned()),
        translation_id: None,
        proximity_proof_required: false,
        asynchronous_execution: asynchronous,
        tags: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn demo_server_lists_three_components() {
        let server = InMemoryServer::new_with_demo_data();
        let entities = server.list_entities().await.expect("list entities");
        let ids: Vec<String> = entities.items.iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["cvc", "fzc", "rzc"]);
    }

    #[tokio::test]
    async fn list_faults_returns_canned_faults() {
        let server = InMemoryServer::new_with_demo_data();
        let view = server
            .component_server(&ComponentId::new("cvc"))
            .await
            .expect("component view");
        let list = view
            .list_faults(FaultFilter::all())
            .await
            .expect("list faults");
        assert_eq!(list.items.len(), 2);
        assert!(list.items.iter().any(|f| f.code == "P0A1F"));
    }

    #[tokio::test]
    async fn get_fault_returns_environment_data() {
        let server = InMemoryServer::new_with_demo_data();
        let view = server
            .component_server(&ComponentId::new("cvc"))
            .await
            .expect("component view");
        let details = view.get_fault("P0A1F").await.expect("get fault");
        assert_eq!(details.item.code, "P0A1F");
        assert!(details.environment_data.is_some());
    }

    #[tokio::test]
    async fn clear_fault_removes_from_list() {
        let server = InMemoryServer::new_with_demo_data();
        let view = server
            .component_server(&ComponentId::new("cvc"))
            .await
            .expect("component view");
        view.clear_fault("P0A1F").await.expect("clear fault");
        let list = view
            .list_faults(FaultFilter::all())
            .await
            .expect("list faults");
        assert!(list.items.iter().all(|f| f.code != "P0A1F"));
    }

    #[tokio::test]
    async fn clear_all_faults_empties_list() {
        let server = InMemoryServer::new_with_demo_data();
        let view = server
            .component_server(&ComponentId::new("cvc"))
            .await
            .expect("component view");
        view.clear_all_faults().await.expect("clear all");
        let list = view
            .list_faults(FaultFilter::all())
            .await
            .expect("list faults");
        assert!(list.items.is_empty());
    }

    #[tokio::test]
    async fn start_execution_creates_tracked_execution() {
        let server = InMemoryServer::new_with_demo_data();
        let view = server
            .component_server(&ComponentId::new("cvc"))
            .await
            .expect("component view");
        let started = view
            .start_execution(
                "motor_self_test",
                StartExecutionRequest {
                    timeout: Some(30),
                    parameters: Some(serde_json::json!({"mode": "quick"})),
                    proximity_response: None,
                },
            )
            .await
            .expect("start execution");
        let status = view
            .execution_status("motor_self_test", &started.id)
            .await
            .expect("exec status");
        assert_eq!(status.status, Some(ExecutionStatus::Running));
    }

    #[tokio::test]
    async fn unknown_component_is_not_found() {
        let server = InMemoryServer::new_with_demo_data();
        let err = server
            .component_server(&ComponentId::new("nope"))
            .await
            .expect_err("should not find");
        assert!(matches!(err, SovdError::NotFound { .. }));
    }

    #[tokio::test]
    async fn entity_capabilities_round_trip() {
        let server = InMemoryServer::new_with_demo_data();
        let view = server
            .component_server(&ComponentId::new("cvc"))
            .await
            .expect("component view");
        let caps = view.entity_capabilities().await.expect("capabilities");
        assert_eq!(caps.id, "cvc");
        assert!(caps.faults.is_some());
    }

    #[tokio::test]
    async fn severity_filter_below_threshold() {
        let server = InMemoryServer::new_with_demo_data();
        let view = server
            .component_server(&ComponentId::new("cvc"))
            .await
            .expect("component view");
        let filter = FaultFilter {
            severity: Some(3),
            ..FaultFilter::all()
        };
        let list = view.list_faults(filter).await.expect("list");
        // P0A1F has severity 2 (< 3), P0562 has severity 3 (not < 3).
        assert!(list.items.iter().all(|f| f.severity.unwrap_or(0) < 3));
    }
}
