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

#![allow(clippy::doc_markdown)]
// ADR-0018 D7: deny expect_used in production backend code.
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

//! Diagnostic Fault Manager (DFM) for the Eclipse `OpenSOVD` core stack.
//!
//! Phase 3 wires the DFM through the pluggable trait seams defined in
//! ADR-0016:
//!
//! - Persistence via [`SovdDb`] — default `sovd-db-sqlite`, optional
//!   `sovd-db-score` behind the `score` feature.
//! - Ingestion via [`FaultSink`] — default `fault-sink-unix`, optional
//!   `fault-sink-lola` behind the `score` feature.
//! - Lifecycle via [`OperationCycle`] — default `opcycle-taktflow`,
//!   optional `opcycle-score-lifecycle` behind the `score` feature.
//!
//! The DFM holds one boxed instance of each trait and implements both
//! [`FaultSink`] (ingestion side) and [`SovdBackend`] (read side). The
//! concrete backends are selected at runtime by `sovd-main` from the
//! `[backend]` TOML section.
//!
//! [`SovdDb`]: sovd_interfaces::traits::sovd_db::SovdDb
//! [`FaultSink`]: sovd_interfaces::traits::fault_sink::FaultSink
//! [`OperationCycle`]: sovd_interfaces::traits::operation_cycle::OperationCycle
//! [`SovdBackend`]: sovd_interfaces::traits::backend::SovdBackend

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use sovd_interfaces::{
    ComponentId, SovdError,
    spec::{
        component::EntityCapabilities,
        data::{Datas, ValueMetadata},
        fault::{FaultDetails, FaultFilter, ListOfFaults},
        operation::{
            Capability, ExecutionStatus, ExecutionStatusResponse, OperationDescription,
            OperationsList, StartExecutionAsyncResponse, StartExecutionRequest,
        },
    },
    traits::{
        backend::{BackendHealth, BackendKind, SovdBackend},
        fault_sink::{FaultRecordRef, FaultSink},
        operation_cycle::OperationCycle,
        sovd_db::SovdDb,
    },
    types::error::Result,
};
use tokio::sync::RwLock;
use uuid::Uuid;

pub mod config;

pub use config::{DfmBackendConfig, FaultSinkBackend, OperationCycleBackend, PersistenceBackend};

/// ADR-0018 rule 3: default budget for `try_lock_with_timeout`. The
/// operation under the guard must be short-running; the budget is
/// the outer wall-clock window we are willing to block before
/// falling back to a degraded response.
const LOCK_BUDGET: Duration = Duration::from_millis(50);

/// Record of one operation execution managed by the DFM.
#[derive(Debug, Clone)]
struct ExecutionRecord {
    operation_id: String,
    status: ExecutionStatus,
    parameters: Option<serde_json::Value>,
    /// Name of the operation cycle that was active when this execution
    /// was started. Used by the cycle-driven gating rule in
    /// [`Dfm::execution_status`] — a running execution whose cycle has
    /// since ended is reported as
    /// [`ExecutionStatus::Completed`].
    started_under_cycle: Option<String>,
}

/// Diagnostic Fault Manager runtime object.
///
/// Constructed by [`Dfm::builder`] with concrete backends. The DFM
/// owns the store (`SovdDb`), the lifecycle driver (`OperationCycle`),
/// an operation catalog, a data catalog, and a map of live executions.
/// It implements both [`FaultSink`] (ingestion side) and
/// [`SovdBackend`] (read side).
pub struct Dfm {
    component: ComponentId,
    db: Arc<dyn SovdDb>,
    cycles: Arc<dyn OperationCycle>,
    /// Operation catalog published via `list_operations`. Defaults to
    /// an empty catalog when the builder is not given one.
    operations: Vec<OperationDescription>,
    /// Data-metadata catalog published via `list_data`. Defaults to an
    /// empty catalog.
    data_catalog: Vec<ValueMetadata>,
    /// Active / historical executions keyed by execution id.
    ///
    /// RwLock (not parking_lot) so the DFM can serialise access from
    /// multiple axum task workers. `HashMap` is fine because the axum
    /// router only touches executions via
    /// [`SovdBackend::start_execution`] / [`SovdBackend::execution_status`].
    executions: Arc<RwLock<HashMap<String, ExecutionRecord>>>,
    /// ADR-0018 rule 4 last-known snapshot cache. Every successful
    /// `list_faults` from the underlying `SovdDb` captures its
    /// response here with a wall-clock timestamp. On a subsequent
    /// `SovdDb` error we serve the cached snapshot with a
    /// `stale: true` + `age_ms` marker rather than propagating the
    /// error. An empty cache on the first-ever error surfaces the
    /// original error, because there is nothing usable to fall
    /// back to.
    last_known_faults: Arc<RwLock<Option<LastKnownFaults>>>,
}

/// Last successful `SovdDb::list_faults` result, cached for the
/// stale-fallback path per ADR-0018 rule 4.
#[derive(Debug, Clone)]
struct LastKnownFaults {
    items: Vec<sovd_interfaces::spec::fault::Fault>,
    captured_at: Instant,
}

impl std::fmt::Debug for Dfm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dfm")
            .field("component", &self.component)
            .field("db", &"<dyn SovdDb>")
            .field("cycles", &"<dyn OperationCycle>")
            .field("operations", &self.operations.len())
            .field("data_catalog", &self.data_catalog.len())
            .field("executions", &"<RwLock<HashMap>>")
            .field("last_known_faults", &"<RwLock<Option<LastKnownFaults>>>")
            .finish()
    }
}

impl Dfm {
    /// Borrow the store trait object.
    #[must_use]
    pub fn db(&self) -> &Arc<dyn SovdDb> {
        &self.db
    }

    /// Borrow the cycle trait object.
    #[must_use]
    pub fn cycles(&self) -> &Arc<dyn OperationCycle> {
        &self.cycles
    }

    /// Borrow the published operation catalog.
    #[must_use]
    pub fn operation_catalog(&self) -> &[OperationDescription] {
        &self.operations
    }

    /// Borrow the published data-metadata catalog.
    #[must_use]
    pub fn data_catalog(&self) -> &[ValueMetadata] {
        &self.data_catalog
    }

    /// Builder entry point — use [`DfmBuilder::build`] to finish.
    #[must_use]
    pub fn builder(component: ComponentId) -> DfmBuilder {
        DfmBuilder {
            component,
            db: None,
            cycles: None,
            operations: Vec::new(),
            data_catalog: Vec::new(),
        }
    }
}

/// Builder for a [`Dfm`]. Keeps the wiring dependency-injected so
/// integration tests can plug in fakes without a TOML round-trip.
pub struct DfmBuilder {
    component: ComponentId,
    db: Option<Arc<dyn SovdDb>>,
    cycles: Option<Arc<dyn OperationCycle>>,
    operations: Vec<OperationDescription>,
    data_catalog: Vec<ValueMetadata>,
}

impl DfmBuilder {
    /// Attach a concrete `SovdDb`.
    #[must_use]
    pub fn with_db(mut self, db: Arc<dyn SovdDb>) -> Self {
        self.db = Some(db);
        self
    }

    /// Attach a concrete `OperationCycle`.
    #[must_use]
    pub fn with_cycles(mut self, cycles: Arc<dyn OperationCycle>) -> Self {
        self.cycles = Some(cycles);
        self
    }

    /// Seed the operation catalog published by [`Dfm::list_operations`].
    /// Without this, the DFM publishes an empty operations list and
    /// `start_execution` rejects every request with
    /// [`SovdError::NotFound`].
    #[must_use]
    pub fn with_operation_catalog(mut self, operations: Vec<OperationDescription>) -> Self {
        self.operations = operations;
        self
    }

    /// Seed the data-metadata catalog published by [`Dfm::list_data`].
    /// Without this, the DFM publishes an empty data catalog.
    #[must_use]
    pub fn with_data_catalog(mut self, data_catalog: Vec<ValueMetadata>) -> Self {
        self.data_catalog = data_catalog;
        self
    }

    /// Finish building.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::InvalidRequest`] if either backend is missing.
    pub fn build(self) -> Result<Dfm> {
        let db = self
            .db
            .ok_or_else(|| SovdError::InvalidRequest("DfmBuilder: missing SovdDb".into()))?;
        let cycles = self.cycles.ok_or_else(|| {
            SovdError::InvalidRequest("DfmBuilder: missing OperationCycle".into())
        })?;
        Ok(Dfm {
            component: self.component,
            db,
            cycles,
            operations: self.operations,
            data_catalog: self.data_catalog,
            executions: Arc::new(RwLock::new(HashMap::new())),
            last_known_faults: Arc::new(RwLock::new(None)),
        })
    }
}

#[async_trait]
impl FaultSink for Dfm {
    async fn record_fault<'buf>(&self, record: FaultRecordRef<'buf>) -> Result<()> {
        // Update the SQLite current-cycle tag via the cycle subscriber
        // (best-effort, no await on the watch — the DFM runtime is
        // responsible for running a background task that listens and
        // re-tags). Here we just forward the ingest.
        self.db.ingest_fault(record.into_owned()).await
    }
}

#[async_trait]
impl SovdBackend for Dfm {
    fn component_id(&self) -> ComponentId {
        self.component.clone()
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Dfm
    }

    async fn list_faults(&self, filter: FaultFilter) -> Result<ListOfFaults> {
        // ADR-0018 rule 4: on success, warm the last-known snapshot;
        // on SovdDb error, try to serve the cached snapshot with a
        // stale: true marker instead of propagating. Fall through to
        // the original error only if the cache is empty.
        match self.db.list_faults(filter).await {
            Ok(list) => {
                // ADR-0018 rule 3: bounded lock acquisition. If we
                // cannot grab the cache within LOCK_BUDGET we skip
                // cache update and return the fresh list anyway —
                // the next successful call will update the cache.
                if let Ok(mut cache) =
                    tokio::time::timeout(LOCK_BUDGET, self.last_known_faults.write()).await
                {
                    *cache = Some(LastKnownFaults {
                        items: list.items.clone(),
                        captured_at: Instant::now(),
                    });
                } else {
                    tracing::warn!(
                        backend = "dfm",
                        operation = "list_faults",
                        component_id = %self.component,
                        error_kind = "cache_lock_timeout",
                        budget_ms = u64::try_from(LOCK_BUDGET.as_millis()).unwrap_or(u64::MAX),
                        "Dfm: last_known_faults write lock contended; skipping cache update"
                    );
                }
                Ok(list)
            }
            Err(err) => {
                // Bounded read per ADR-0018 rule 3. Short-circuit to
                // the original error on contention rather than hang.
                let cache_guard = if let Ok(guard) =
                    tokio::time::timeout(LOCK_BUDGET, self.last_known_faults.read()).await
                {
                    Some(guard)
                } else {
                    tracing::warn!(
                        backend = "dfm",
                        operation = "list_faults",
                        component_id = %self.component,
                        error_kind = "cache_lock_timeout",
                        budget_ms = u64::try_from(LOCK_BUDGET.as_millis()).unwrap_or(u64::MAX),
                        "Dfm: last_known_faults read lock contended on fallback"
                    );
                    None
                };
                let Some(guard) = cache_guard.as_ref() else {
                    return Err(SovdError::Degraded {
                        reason: "dfm cache lock contention".into(),
                    });
                };
                let cache = guard.as_ref();
                if let Some(snapshot) = cache {
                    let age_ms = u64::try_from(snapshot.captured_at.elapsed().as_millis())
                        .unwrap_or(u64::MAX);
                    tracing::warn!(
                        backend = "dfm",
                        operation = "list_faults",
                        component_id = %self.component,
                        error_kind = "sovd_db_error_stale_fallback",
                        age_ms,
                        "Dfm: SovdDb error, serving last-known snapshot: {err}"
                    );
                    Ok(ListOfFaults {
                        items: snapshot.items.clone(),
                        total: None,
                        next_page: None,
                        schema: None,
                        extras: Some(
                            sovd_interfaces::extras::response::ResponseExtras::stale_cache(age_ms),
                        ),
                    })
                } else {
                    tracing::warn!(
                        backend = "dfm",
                        operation = "list_faults",
                        component_id = %self.component,
                        error_kind = "sovd_db_error_no_cache",
                        "Dfm: SovdDb error with empty cache, propagating: {err}"
                    );
                    Err(err)
                }
            }
        }
    }

    async fn get_fault(&self, code: &str) -> Result<FaultDetails> {
        self.db.get_fault(code).await
    }

    async fn clear_all_faults(&self) -> Result<()> {
        self.db.clear_faults(FaultFilter::all()).await
    }

    async fn clear_fault(&self, code: &str) -> Result<()> {
        self.db.clear_fault_by_code(code).await
    }

    async fn list_operations(&self) -> Result<OperationsList> {
        Ok(OperationsList {
            items: self.operations.clone(),
            schema: None,
        })
    }

    async fn start_execution(
        &self,
        operation_id: &str,
        request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse> {
        let op_id = operation_id.to_owned();
        let descriptor = self
            .operations
            .iter()
            .find(|o| o.id == op_id)
            .cloned()
            .ok_or_else(|| SovdError::NotFound {
                entity: format!("operation \"{op_id}\" on component \"{}\"", self.component),
            })?;

        let started_under_cycle = self.cycles.current_cycle().await.ok().and_then(|c| c.name);
        let exec_id = Uuid::new_v4().to_string();
        // Synchronous operations complete immediately — the DFM does not
        // spawn background work for them, so the first GET returns
        // Completed. Asynchronous operations stay in `Running` until a
        // follow-up mechanism marks them complete (Phase 5 worker pool).
        let initial_status = if descriptor.asynchronous_execution {
            ExecutionStatus::Running
        } else {
            ExecutionStatus::Completed
        };
        let record = ExecutionRecord {
            operation_id: op_id.clone(),
            status: initial_status,
            parameters: request.parameters,
            started_under_cycle,
        };
        self.executions
            .write()
            .await
            .insert(exec_id.clone(), record);
        Ok(StartExecutionAsyncResponse {
            id: exec_id,
            status: Some(initial_status),
        })
    }

    async fn execution_status(
        &self,
        operation_id: &str,
        execution_id: &str,
    ) -> Result<ExecutionStatusResponse> {
        let op_id = operation_id.to_owned();
        let exec_id = execution_id.to_owned();
        let current_cycle_name = self.cycles.current_cycle().await.ok().and_then(|c| c.name);
        let mut guard = self.executions.write().await;
        let record = guard.get_mut(&exec_id).ok_or_else(|| SovdError::NotFound {
            entity: format!("execution \"{exec_id}\""),
        })?;
        if record.operation_id != op_id {
            return Err(SovdError::NotFound {
                entity: format!("execution \"{exec_id}\" of operation \"{op_id}\""),
            });
        }
        // Cycle-gated completion: if the operation was started under a
        // cycle that has since ended, we report it as Completed. This is
        // the ADR-0012 rule exposed through the backend so the
        // OperationCycle state machine really does drive execution
        // semantics (the prompt's "OperationCycle-driven" requirement).
        if record.status == ExecutionStatus::Running
            && record.started_under_cycle.is_some()
            && record.started_under_cycle != current_cycle_name
        {
            record.status = ExecutionStatus::Completed;
        }
        Ok(ExecutionStatusResponse {
            status: Some(record.status),
            capability: Capability::Execute,
            parameters: record.parameters.clone(),
            schema: None,
            error: None,
        })
    }

    async fn list_data(&self) -> Result<Datas> {
        Ok(Datas {
            items: self.data_catalog.clone(),
            schema: None,
        })
    }

    async fn entity_capabilities(&self) -> Result<EntityCapabilities> {
        let id = self.component.as_str().to_owned();
        Ok(EntityCapabilities {
            id: id.clone(),
            name: format!("dfm:{id}"),
            translation_id: None,
            variant: None,
            configurations: None,
            bulk_data: None,
            data: if self.data_catalog.is_empty() {
                None
            } else {
                Some(format!("/sovd/v1/components/{id}/data"))
            },
            data_lists: None,
            faults: Some(format!("/sovd/v1/components/{id}/faults")),
            operations: if self.operations.is_empty() {
                None
            } else {
                Some(format!("/sovd/v1/components/{id}/operations"))
            },
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
        // Phase 4 probe strategy: list_faults with an empty filter is
        // the cheapest end-to-end SovdDb round-trip. If it fails the
        // store is degraded regardless of the specific error kind.
        match self.db.list_faults(FaultFilter::all()).await {
            Ok(_) => BackendHealth::Ok,
            Err(e) => BackendHealth::Degraded {
                reason: format!("sovd_db probe failed: {e}"),
            },
        }
    }

    async fn current_operation_cycle(&self) -> Option<String> {
        self.cycles.current_cycle().await.ok().and_then(|c| c.name)
    }
}

#[cfg(test)]
mod tests {
    use opcycle_taktflow::TaktflowOperationCycle;
    use sovd_db_sqlite::SqliteSovdDb;
    use sovd_interfaces::extras::fault::{FaultId, FaultRecord, FaultSeverity};

    use super::*;

    async fn build_dfm() -> Dfm {
        let db: Arc<dyn SovdDb> = Arc::new(
            SqliteSovdDb::connect_in_memory()
                .await
                .expect("sqlite connect"),
        );
        let cycles: Arc<dyn OperationCycle> = Arc::new(TaktflowOperationCycle::new());
        Dfm::builder(ComponentId::new("cvc"))
            .with_db(db)
            .with_cycles(cycles)
            .build()
            .expect("build")
    }

    fn sample(id: u32) -> FaultRecord {
        FaultRecord {
            component: ComponentId::new("cvc"),
            id: FaultId(id),
            severity: FaultSeverity::Error,
            timestamp_ms: 7,
            meta: None,
        }
    }

    #[tokio::test]
    async fn ingest_then_list_via_backend() {
        let dfm = build_dfm().await;
        dfm.record_fault(sample(0x11).into()).await.expect("ingest");
        let list = dfm.list_faults(FaultFilter::all()).await.expect("list");
        assert_eq!(list.items.len(), 1);
        let first = list.items.first().expect("first item");
        assert_eq!(first.code, "000011");
    }

    #[tokio::test]
    async fn clear_all_via_backend() {
        let dfm = build_dfm().await;
        dfm.record_fault(sample(0x22).into()).await.expect("ingest");
        dfm.clear_all_faults().await.expect("clear");
        let list = dfm.list_faults(FaultFilter::all()).await.expect("list");
        assert!(list.items.is_empty());
    }

    // D4-red: ADR-0018 rule 4 mandates that on a SovdDb error Dfm
    // falls back to a last-known snapshot with a stale: true flag,
    // NOT a propagated error. We simulate the failure path with a
    // fake SovdDb that succeeds once, caches the snapshot through
    // Dfm::list_faults, then fails, and expect the Dfm layer to
    // serve the cache rather than bail.
    //
    // The fake needs interior mutability so the test can flip the
    // fail flag between calls.
    #[tokio::test]
    async fn list_faults_falls_back_to_last_known_snapshot_on_db_error() {
        use std::sync::{
            Arc as StdArc,
            atomic::{AtomicBool, Ordering},
        };

        use sovd_interfaces::{spec::fault::Fault, traits::sovd_db::SovdDb};

        struct FlakySovdDb {
            fail: StdArc<AtomicBool>,
        }

        #[async_trait]
        impl SovdDb for FlakySovdDb {
            async fn ingest_fault(
                &self,
                _record: sovd_interfaces::extras::fault::FaultRecord,
            ) -> Result<()> {
                Ok(())
            }
            async fn list_faults(&self, _filter: FaultFilter) -> Result<ListOfFaults> {
                if self.fail.load(Ordering::SeqCst) {
                    return Err(SovdError::Internal("simulated sqlite busy".into()));
                }
                Ok(ListOfFaults {
                    items: vec![Fault {
                        code: "CACHED_001".into(),
                        scope: None,
                        display_code: None,
                        fault_name: "cached".into(),
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
            async fn get_fault(&self, code: &str) -> Result<FaultDetails> {
                Err(SovdError::NotFound {
                    entity: code.into(),
                })
            }
            async fn clear_faults(&self, _filter: FaultFilter) -> Result<()> {
                Ok(())
            }
            async fn clear_fault_by_code(&self, _code: &str) -> Result<()> {
                Ok(())
            }
            async fn snapshot_for_operation_cycle(
                &self,
                _cycle_id: &sovd_interfaces::traits::sovd_db::OperationCycleId,
            ) -> Result<()> {
                Ok(())
            }
        }

        let fail = StdArc::new(AtomicBool::new(false));
        let db: Arc<dyn SovdDb> = Arc::new(FlakySovdDb {
            fail: StdArc::clone(&fail),
        });
        let cycles: Arc<dyn OperationCycle> =
            Arc::new(opcycle_taktflow::TaktflowOperationCycle::new());
        let dfm = Dfm::builder(ComponentId::new("cvc"))
            .with_db(db)
            .with_cycles(cycles)
            .build()
            .expect("build");

        // First call: warms the cache from a successful DB read.
        let nominal = dfm
            .list_faults(FaultFilter::all())
            .await
            .expect("warm cache");
        assert_eq!(nominal.items.len(), 1);
        assert!(
            nominal.extras.is_none(),
            "nominal response must not carry stale marker"
        );

        // Flip the DB to error mode and try again — ADR-0018 rule 4
        // requires the cached snapshot to be served with stale: true.
        fail.store(true, Ordering::SeqCst);
        let degraded = dfm
            .list_faults(FaultFilter::all())
            .await
            .expect("should return cached snapshot, not error");
        assert_eq!(degraded.items.len(), 1);
        assert_eq!(
            degraded.items.first().map(|f| f.code.as_str()),
            Some("CACHED_001")
        );
        let extras = degraded.extras.expect("stale extras must be set");
        assert!(extras.stale, "stale flag must be set on degraded response");
        assert!(
            extras.age_ms.is_some(),
            "age_ms must be populated on stale cache response"
        );
    }

    // D6: lock contention stress. ADR-0018 rule 3 mandates that
    // backend locks use a bounded acquisition budget (default 50 ms)
    // and fall back to Degraded on timeout rather than hanging. We
    // exercise the path by holding the last_known_faults write lock
    // externally for longer than LOCK_BUDGET, then firing a
    // list_faults call — under nominal behaviour the DFM should
    // either skip its cache update silently (on the Ok() branch)
    // or surface a Degraded on the fallback read branch.
    #[tokio::test]
    async fn list_faults_write_cache_timeout_does_not_hang() {
        use sovd_interfaces::{spec::fault::Fault, traits::sovd_db::SovdDb};

        struct AlwaysOkDb;

        #[async_trait]
        impl SovdDb for AlwaysOkDb {
            async fn ingest_fault(
                &self,
                _record: sovd_interfaces::extras::fault::FaultRecord,
            ) -> Result<()> {
                Ok(())
            }
            async fn list_faults(&self, _filter: FaultFilter) -> Result<ListOfFaults> {
                Ok(ListOfFaults {
                    items: vec![Fault {
                        code: "LIVE".into(),
                        scope: None,
                        display_code: None,
                        fault_name: "live".into(),
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
            async fn get_fault(&self, code: &str) -> Result<FaultDetails> {
                Err(SovdError::NotFound {
                    entity: code.into(),
                })
            }
            async fn clear_faults(&self, _filter: FaultFilter) -> Result<()> {
                Ok(())
            }
            async fn clear_fault_by_code(&self, _code: &str) -> Result<()> {
                Ok(())
            }
            async fn snapshot_for_operation_cycle(
                &self,
                _cycle_id: &sovd_interfaces::traits::sovd_db::OperationCycleId,
            ) -> Result<()> {
                Ok(())
            }
        }

        let db: Arc<dyn SovdDb> = Arc::new(AlwaysOkDb);
        let cycles: Arc<dyn OperationCycle> =
            Arc::new(opcycle_taktflow::TaktflowOperationCycle::new());
        let dfm = Arc::new(
            Dfm::builder(ComponentId::new("cvc"))
                .with_db(db)
                .with_cycles(cycles)
                .build()
                .expect("build"),
        );

        // Take the write lock for > LOCK_BUDGET (50 ms) so the
        // in-flight list_faults caches contend on the write path.
        let blocker_dfm = Arc::clone(&dfm);
        let blocker = tokio::spawn(async move {
            let _guard = blocker_dfm.last_known_faults.write().await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        // Let the blocker acquire the lock first.
        tokio::time::sleep(Duration::from_millis(10)).await;

        // This call must return (not hang) within the test timeout,
        // and must return the live list — the cache update is
        // allowed to no-op on lock timeout per ADR-0018 rule 3.
        let list = tokio::time::timeout(
            Duration::from_millis(500),
            dfm.list_faults(FaultFilter::all()),
        )
        .await
        .expect("list_faults must not hang under lock contention")
        .expect("live list_faults should still succeed");
        assert_eq!(list.items.len(), 1);
        blocker.await.expect("blocker task");
    }

    #[tokio::test]
    async fn cycles_round_trip_through_dfm_cycles_accessor() {
        let dfm = build_dfm().await;
        dfm.cycles()
            .start_cycle("tester.dfm".into())
            .await
            .expect("start");
        let current = dfm.cycles().current_cycle().await.expect("current");
        assert_eq!(current.name.as_deref(), Some("tester.dfm"));
    }
}
