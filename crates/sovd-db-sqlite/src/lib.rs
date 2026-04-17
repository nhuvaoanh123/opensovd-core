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

//! SQLite backend for the [`SovdDb`] trait.
//!
//! This is the default standalone persistence backend for the Taktflow
//! OpenSOVD stack per ADR-0003 and ADR-0016. It holds faults in a single
//! `faults` table, stored under WAL journaling, and answers the SOVD read
//! path by aggregating rows on-the-fly into [`spec::fault::Fault`] entries.
//!
//! The trait surface is in `sovd-interfaces` — this crate is a concrete
//! implementation only. See `crates/sovd-db-sqlite/migrations/` for the
//! schema.
//!
//! [`SovdDb`]: sovd_interfaces::traits::sovd_db::SovdDb
//! [`spec::fault::Fault`]: sovd_interfaces::spec::fault::Fault

use std::{collections::BTreeMap, path::Path, str::FromStr};

use async_trait::async_trait;
use sovd_interfaces::{
    SovdError,
    extras::fault::FaultRecord,
    spec::fault::{Fault, FaultDetails, FaultFilter, ListOfFaults},
    traits::sovd_db::{OperationCycleId, SovdDb},
    types::error::Result,
};
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions},
};
use tokio::sync::RwLock;

/// SQLite-backed [`SovdDb`] implementation.
///
/// Constructed via [`SqliteSovdDb::connect`] for a file database or
/// [`SqliteSovdDb::connect_in_memory`] for tests. Migrations are applied
/// automatically on startup.
#[derive(Debug, Clone)]
pub struct SqliteSovdDb {
    pool: SqlitePool,
    /// Currently active operation cycle, if any. Set by the DFM's cycle
    /// driver via [`Self::set_active_cycle`]; consulted by
    /// [`Self::ingest_fault`] so ingested events carry the right cycle
    /// tag without the trait signature having to change.
    active_cycle: std::sync::Arc<RwLock<Option<String>>>,
}

impl SqliteSovdDb {
    /// Open a SQLite file at `path`, create it if missing, enable WAL,
    /// and apply every embedded migration.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`] if the pool cannot be opened or
    /// migrations cannot be applied.
    pub async fn connect(path: &Path) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        Self::connect_with_options(opts).await
    }

    /// Open an in-memory SQLite pool. Intended for tests.
    ///
    /// # Errors
    ///
    /// Returns [`SovdError::Internal`] if the pool cannot be opened or
    /// migrations cannot be applied.
    pub async fn connect_in_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(|e| SovdError::Internal(format!("invalid sqlite uri: {e}")))?
            .journal_mode(SqliteJournalMode::Wal);
        // In-memory SQLite is per-connection, so we restrict the pool to
        // a single connection to avoid "database does not exist" races.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .map_err(|e| SovdError::Internal(format!("sqlite connect failed: {e}")))?;
        Self::with_pool(pool).await
    }

    async fn connect_with_options(opts: SqliteConnectOptions) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(opts)
            .await
            .map_err(|e| SovdError::Internal(format!("sqlite connect failed: {e}")))?;
        Self::with_pool(pool).await
    }

    async fn with_pool(pool: SqlitePool) -> Result<Self> {
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| SovdError::Internal(format!("sqlite migrate failed: {e}")))?;
        Ok(Self {
            pool,
            active_cycle: std::sync::Arc::new(RwLock::new(None)),
        })
    }

    /// Share the pool for advanced introspection (tests, admin CLIs).
    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Update the active operation cycle tag. The DFM's cycle driver
    /// calls this in response to
    /// [`OperationCycle::subscribe_events`](sovd_interfaces::traits::operation_cycle::OperationCycle::subscribe_events).
    pub async fn set_active_cycle(&self, cycle: Option<String>) {
        *self.active_cycle.write().await = cycle;
    }
}

fn code_from_fault_id(id: sovd_interfaces::extras::fault::FaultId) -> String {
    format!("{:06X}", id.0)
}

fn aggregate_rows_into_faults(rows: Vec<(String, i64, String, Option<String>)>) -> Vec<Fault> {
    // Aggregate by (component, code): one Fault per unique pair, status
    // carries last-seen metadata so GET /faults is idempotent under the
    // current event stream.
    let mut by_code: BTreeMap<(String, String), Fault> = BTreeMap::new();
    for (code, severity, component, meta_json) in rows {
        let entry = by_code
            .entry((component.clone(), code.clone()))
            .or_insert_with(|| Fault {
                code: code.clone(),
                scope: Some(component.clone()),
                display_code: Some(code.clone()),
                fault_name: code.clone(),
                fault_translation_id: None,
                severity: Some(i32::try_from(severity).unwrap_or(4)),
                status: Some(serde_json::json!({"aggregatedStatus": "active"})),
                symptom: None,
                symptom_translation_id: None,
                tags: None,
            });
        // Most-severe wins for the aggregated entry.
        if let Some(existing) = entry.severity {
            if i64::from(existing) > severity {
                entry.severity = Some(i32::try_from(severity).unwrap_or(existing));
            }
        }
        if let Some(raw) = meta_json.as_ref() {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw) {
                entry.status = Some(serde_json::json!({
                    "aggregatedStatus": "active",
                    "meta": parsed,
                }));
            }
        }
    }
    by_code.into_values().collect()
}

fn matches_filter(fault: &Fault, filter: &FaultFilter) -> bool {
    if let Some(threshold) = filter.severity {
        if let Some(sev) = fault.severity {
            if sev >= threshold {
                return false;
            }
        }
    }
    if let Some(scope) = filter.scope.as_ref() {
        if fault.scope.as_deref() != Some(scope.as_str()) {
            return false;
        }
    }
    if !filter.status_keys.is_empty() {
        let Some(status) = fault.status.as_ref().and_then(|s| s.as_object()) else {
            return false;
        };
        let any_match = filter
            .status_keys
            .iter()
            .any(|(k, v)| status.get(k).and_then(|val| val.as_str()) == Some(v.as_str()));
        if !any_match {
            return false;
        }
    }
    true
}

#[async_trait]
impl SovdDb for SqliteSovdDb {
    async fn ingest_fault(&self, record: FaultRecord) -> Result<()> {
        let cycle = self.active_cycle.read().await.clone();
        let code = code_from_fault_id(record.id);
        let severity = i64::from(record.severity.as_i32());
        let ts = i64::try_from(record.timestamp_ms).unwrap_or(i64::MAX);
        let meta_json = record
            .meta
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        sqlx::query(
            "INSERT INTO faults \
             (component, code, severity, timestamp_ms, meta_json, operation_cycle) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(record.component.as_str())
        .bind(&code)
        .bind(severity)
        .bind(ts)
        .bind(meta_json)
        .bind(cycle)
        .execute(&self.pool)
        .await
        .map_err(|e| SovdError::Internal(format!("sqlite insert failed: {e}")))?;
        Ok(())
    }

    async fn list_faults(&self, filter: FaultFilter) -> Result<ListOfFaults> {
        let rows = sqlx::query(
            "SELECT code, severity, component, meta_json FROM faults \
             ORDER BY row_id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SovdError::Internal(format!("sqlite select failed: {e}")))?;
        let mut raw: Vec<(String, i64, String, Option<String>)> = Vec::with_capacity(rows.len());
        for row in rows {
            raw.push((
                row.try_get::<String, _>("code")
                    .map_err(|e| SovdError::Internal(e.to_string()))?,
                row.try_get::<i64, _>("severity")
                    .map_err(|e| SovdError::Internal(e.to_string()))?,
                row.try_get::<String, _>("component")
                    .map_err(|e| SovdError::Internal(e.to_string()))?,
                row.try_get::<Option<String>, _>("meta_json")
                    .map_err(|e| SovdError::Internal(e.to_string()))?,
            ));
        }
        let all = aggregate_rows_into_faults(raw);
        let items = all
            .into_iter()
            .filter(|f| matches_filter(f, &filter))
            .collect();
        Ok(ListOfFaults {
            items,
            total: None,
            next_page: None,
            schema: None,
            extras: None,
        })
    }

    async fn get_fault(&self, code: &str) -> Result<FaultDetails> {
        let rows = sqlx::query(
            "SELECT code, severity, component, meta_json FROM faults \
             WHERE code = ?1 ORDER BY row_id ASC",
        )
        .bind(code)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SovdError::Internal(format!("sqlite select failed: {e}")))?;
        if rows.is_empty() {
            return Err(SovdError::NotFound {
                entity: format!("fault \"{code}\""),
            });
        }
        let mut raw: Vec<(String, i64, String, Option<String>)> = Vec::with_capacity(rows.len());
        for row in rows {
            raw.push((
                row.try_get::<String, _>("code")
                    .map_err(|e| SovdError::Internal(e.to_string()))?,
                row.try_get::<i64, _>("severity")
                    .map_err(|e| SovdError::Internal(e.to_string()))?,
                row.try_get::<String, _>("component")
                    .map_err(|e| SovdError::Internal(e.to_string()))?,
                row.try_get::<Option<String>, _>("meta_json")
                    .map_err(|e| SovdError::Internal(e.to_string()))?,
            ));
        }
        let aggregated = aggregate_rows_into_faults(raw);
        let item = aggregated
            .into_iter()
            .next()
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

    async fn clear_faults(&self, filter: FaultFilter) -> Result<()> {
        if filter == FaultFilter::all() {
            sqlx::query("DELETE FROM faults")
                .execute(&self.pool)
                .await
                .map_err(|e| SovdError::Internal(format!("sqlite delete failed: {e}")))?;
            return Ok(());
        }
        // For filtered clears we list-then-delete by code. Exact-match
        // SQL filtering for arbitrary status_keys maps poorly onto the
        // current schema; a targeted delete per aggregated code keeps the
        // trait honest without leaking SQL-level filter semantics.
        let list = self.list_faults(filter).await?;
        for fault in list.items {
            sqlx::query("DELETE FROM faults WHERE code = ?1")
                .bind(&fault.code)
                .execute(&self.pool)
                .await
                .map_err(|e| SovdError::Internal(format!("sqlite delete failed: {e}")))?;
        }
        Ok(())
    }

    async fn clear_fault_by_code(&self, code: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM faults WHERE code = ?1")
            .bind(code)
            .execute(&self.pool)
            .await
            .map_err(|e| SovdError::Internal(format!("sqlite delete failed: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(SovdError::NotFound {
                entity: format!("fault \"{code}\""),
            });
        }
        Ok(())
    }

    async fn snapshot_for_operation_cycle(&self, cycle_id: &OperationCycleId) -> Result<()> {
        sqlx::query("INSERT OR REPLACE INTO operation_cycles (cycle_id) VALUES (?1)")
            .bind(cycle_id)
            .execute(&self.pool)
            .await
            .map_err(|e| SovdError::Internal(format!("sqlite snapshot failed: {e}")))?;
        sqlx::query("UPDATE faults SET snapshotted = 1 WHERE operation_cycle = ?1")
            .bind(cycle_id)
            .execute(&self.pool)
            .await
            .map_err(|e| SovdError::Internal(format!("sqlite snapshot failed: {e}")))?;
        Ok(())
    }
}
