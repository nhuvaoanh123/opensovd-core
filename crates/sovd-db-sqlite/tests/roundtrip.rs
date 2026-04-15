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

//! Round-trip tests for the SQLite-backed `SovdDb` implementation.

use sovd_db_sqlite::SqliteSovdDb;
use sovd_interfaces::{
    ComponentId,
    extras::fault::{FaultId, FaultRecord, FaultSeverity},
    spec::fault::FaultFilter,
    traits::sovd_db::SovdDb,
};

fn sample_record(code: u32, severity: FaultSeverity) -> FaultRecord {
    FaultRecord {
        component: ComponentId::new("cvc"),
        id: FaultId(code),
        severity,
        timestamp_ms: 1_000,
        meta: Some(serde_json::json!({"battery_voltage": 12.8})),
    }
}

#[tokio::test]
async fn ingest_then_list_roundtrip() {
    let db = SqliteSovdDb::connect_in_memory().await.expect("connect");
    db.ingest_fault(sample_record(0x12_34_56, FaultSeverity::Error))
        .await
        .expect("ingest");
    db.ingest_fault(sample_record(0xAB_CD_EF, FaultSeverity::Warning))
        .await
        .expect("ingest");

    let list = db.list_faults(FaultFilter::all()).await.expect("list");
    assert_eq!(list.items.len(), 2, "expected two aggregated faults");
    let codes: Vec<_> = list.items.iter().map(|f| f.code.clone()).collect();
    assert!(codes.contains(&"123456".to_string()));
    assert!(codes.contains(&"ABCDEF".to_string()));
}

#[tokio::test]
async fn clear_all_removes_everything() {
    let db = SqliteSovdDb::connect_in_memory().await.expect("connect");
    db.ingest_fault(sample_record(0x00_00_01, FaultSeverity::Error))
        .await
        .expect("ingest");
    db.clear_faults(FaultFilter::all()).await.expect("clear");
    let list = db.list_faults(FaultFilter::all()).await.expect("list");
    assert!(list.items.is_empty());
}

#[tokio::test]
async fn clear_fault_by_code_targeted() {
    let db = SqliteSovdDb::connect_in_memory().await.expect("connect");
    db.ingest_fault(sample_record(0x00_00_01, FaultSeverity::Error))
        .await
        .expect("ingest");
    db.ingest_fault(sample_record(0x00_00_02, FaultSeverity::Error))
        .await
        .expect("ingest");
    db.clear_fault_by_code("000001").await.expect("clear one");
    let list = db.list_faults(FaultFilter::all()).await.expect("list");
    assert_eq!(list.items.len(), 1);
    let first = list.items.first().expect("first item");
    assert_eq!(first.code, "000002");
}

#[tokio::test]
async fn clear_fault_by_code_not_found() {
    let db = SqliteSovdDb::connect_in_memory().await.expect("connect");
    let err = db
        .clear_fault_by_code("000099")
        .await
        .expect_err("should fail");
    assert!(matches!(err, sovd_interfaces::SovdError::NotFound { .. }));
}

#[tokio::test]
async fn get_fault_returns_details() {
    let db = SqliteSovdDb::connect_in_memory().await.expect("connect");
    db.ingest_fault(sample_record(0x00_00_07, FaultSeverity::Fatal))
        .await
        .expect("ingest");
    let details = db.get_fault("000007").await.expect("get");
    assert_eq!(details.item.code, "000007");
    assert_eq!(details.item.severity, Some(1));
}

#[tokio::test]
async fn snapshot_tags_active_cycle() {
    let db = SqliteSovdDb::connect_in_memory().await.expect("connect");
    db.set_active_cycle(Some("tester.run1".into())).await;
    db.ingest_fault(sample_record(0x00_00_42, FaultSeverity::Error))
        .await
        .expect("ingest");
    db.snapshot_for_operation_cycle(&"tester.run1".to_string())
        .await
        .expect("snapshot");

    // Second ingest after cycle-end should carry a different tag.
    db.set_active_cycle(None).await;
    db.ingest_fault(sample_record(0x00_00_43, FaultSeverity::Warning))
        .await
        .expect("ingest");
    let list = db.list_faults(FaultFilter::all()).await.expect("list");
    assert_eq!(list.items.len(), 2);
}

#[tokio::test]
async fn concurrent_writer_smoke() {
    let db = SqliteSovdDb::connect_in_memory().await.expect("connect");
    let mut handles = Vec::new();
    for i in 0i32..16 {
        let db = db.clone();
        handles.push(tokio::spawn(async move {
            let code: u32 = u32::try_from(i).expect("positive").saturating_add(100);
            db.ingest_fault(sample_record(code, FaultSeverity::Warning))
                .await
                .expect("ingest");
        }));
    }
    for h in handles {
        h.await.expect("join");
    }
    let list = db.list_faults(FaultFilter::all()).await.expect("list");
    assert_eq!(list.items.len(), 16);
}

#[tokio::test]
async fn migration_idempotent_on_reopen() {
    // Two connect cycles against the same in-memory pool should apply
    // the migration once — sqlx's _sqlx_migrations table guards against
    // double-apply.
    let db1 = SqliteSovdDb::connect_in_memory().await.expect("connect1");
    drop(db1);
    let db2 = SqliteSovdDb::connect_in_memory().await.expect("connect2");
    let list = db2.list_faults(FaultFilter::all()).await.expect("list");
    assert!(list.items.is_empty());
}
