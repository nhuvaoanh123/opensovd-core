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

use std::sync::Arc;

use async_trait::async_trait;
use sovd_interfaces::{
    ComponentId, SovdError,
    spec::{
        component::EntityCapabilities,
        fault::{FaultFilter, ListOfFaults},
        operation::{StartExecutionAsyncResponse, StartExecutionRequest},
    },
    traits::{
        backend::{BackendKind, SovdBackend},
        fault_sink::{FaultRecordRef, FaultSink},
        operation_cycle::OperationCycle,
        sovd_db::SovdDb,
    },
    types::error::Result,
};

pub mod config;

pub use config::{DfmBackendConfig, FaultSinkBackend, OperationCycleBackend, PersistenceBackend};

/// Diagnostic Fault Manager runtime object.
///
/// Constructed by [`Dfm::builder`] with concrete backends. The DFM
/// owns the store (`SovdDb`), the lifecycle driver (`OperationCycle`),
/// and a component id it serves as a `SovdBackend`.
pub struct Dfm {
    component: ComponentId,
    db: Arc<dyn SovdDb>,
    cycles: Arc<dyn OperationCycle>,
}

impl std::fmt::Debug for Dfm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dfm")
            .field("component", &self.component)
            .field("db", &"<dyn SovdDb>")
            .field("cycles", &"<dyn OperationCycle>")
            .finish()
    }
}

impl Dfm {
    /// Build a DFM that owns the given concrete backends and serves the
    /// given component id.
    #[must_use]
    pub fn new(
        component: ComponentId,
        db: Arc<dyn SovdDb>,
        cycles: Arc<dyn OperationCycle>,
    ) -> Self {
        Self {
            component,
            db,
            cycles,
        }
    }

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

    /// Builder entry point — use [`DfmBuilder::with_defaults`] for the
    /// standalone wiring.
    #[must_use]
    pub fn builder(component: ComponentId) -> DfmBuilder {
        DfmBuilder {
            component,
            db: None,
            cycles: None,
        }
    }
}

/// Builder for a [`Dfm`]. Keeps the wiring dependency-injected so
/// integration tests can plug in fakes without a TOML round-trip.
pub struct DfmBuilder {
    component: ComponentId,
    db: Option<Arc<dyn SovdDb>>,
    cycles: Option<Arc<dyn OperationCycle>>,
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
        self.db.list_faults(filter).await
    }

    async fn clear_all_faults(&self) -> Result<()> {
        self.db.clear_faults(FaultFilter::all()).await
    }

    async fn clear_fault(&self, code: &str) -> Result<()> {
        self.db.clear_fault_by_code(code).await
    }

    async fn start_execution(
        &self,
        _operation_id: &str,
        _request: StartExecutionRequest,
    ) -> Result<StartExecutionAsyncResponse> {
        Err(SovdError::InvalidRequest(
            "DFM does not support operation execution in Phase 3".into(),
        ))
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
            data: None,
            data_lists: None,
            faults: Some(format!("/sovd/v1/components/{id}/faults")),
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

#[cfg(test)]
mod tests {
    use super::*;
    use opcycle_taktflow::TaktflowOperationCycle;
    use sovd_db_sqlite::SqliteSovdDb;
    use sovd_interfaces::{
        extras::fault::{FaultId, FaultRecord, FaultSeverity},
    };

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
        dfm.record_fault(sample(0x11).into())
            .await
            .expect("ingest");
        let list = dfm
            .list_faults(FaultFilter::all())
            .await
            .expect("list");
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].code, "000011");
    }

    #[tokio::test]
    async fn clear_all_via_backend() {
        let dfm = build_dfm().await;
        dfm.record_fault(sample(0x22).into()).await.expect("ingest");
        dfm.clear_all_faults().await.expect("clear");
        let list = dfm
            .list_faults(FaultFilter::all())
            .await
            .expect("list");
        assert!(list.items.is_empty());
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
