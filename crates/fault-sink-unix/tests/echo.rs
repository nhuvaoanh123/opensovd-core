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

//! End-to-end transport test: client → server via the platform's native
//! local-IPC endpoint. Runs on both Linux (Unix socket) and Windows
//! (named pipe) using the same wire format.

// ADR-0018 D7: integration test file, not live backend code —
// allow expect() / unwrap() for test readability.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use fault_sink_unix::{UnixFaultSink, UnixFaultSource};
use sovd_interfaces::{
    ComponentId,
    extras::fault::{FaultId, FaultRecord, FaultSeverity},
    traits::fault_sink::FaultSink,
};

fn sample(id: u32) -> FaultRecord {
    FaultRecord {
        component: ComponentId::new("cvc"),
        id: FaultId(id),
        severity: FaultSeverity::Error,
        timestamp_ms: 42,
        meta: None,
    }
}

#[cfg(unix)]
fn endpoint_path() -> std::path::PathBuf {
    let dir = tempfile::tempdir().expect("tempdir");
    // The tempdir is leaked intentionally for the lifetime of the test.
    let path = dir.path().join("fault.sock");
    std::mem::forget(dir);
    path
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn echo_single_record() {
    let path = endpoint_path();
    let source = UnixFaultSource::bind(&path).expect("bind");
    let received: Arc<Mutex<Vec<FaultRecord>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);

    let server = tokio::spawn(async move {
        source
            .accept_and_drain(move |r| {
                received_clone.lock().expect("lock").push(r);
                Ok(())
            })
            .await
            .expect("drain");
    });

    // Give the listener a moment to be ready.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let sink = UnixFaultSink::connect(&path).await.expect("connect");
    sink.record_fault(sample(0x12).into())
        .await
        .expect("record");
    drop(sink);

    server.await.expect("join");
    let records = received.lock().expect("lock");
    assert_eq!(records.len(), 1);
    let first = records.first().expect("first record");
    assert_eq!(first.id, FaultId(0x12));
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn echo_single_record_named_pipe() {
    use std::ffi::OsString;

    // Use a pipe name unique to this test process.
    let pid = std::process::id();
    let pipe_name = OsString::from(format!(r"\\.\pipe\opensovd-test-fault-{pid}"));
    let source = UnixFaultSource::bind(&pipe_name).expect("bind");
    let received: Arc<Mutex<Vec<FaultRecord>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);

    let server = tokio::spawn(async move {
        source
            .accept_and_drain(move |r| {
                received_clone.lock().expect("lock").push(r);
                Ok(())
            })
            .await
            .expect("drain");
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let sink = UnixFaultSink::connect(&pipe_name).await.expect("connect");
    sink.record_fault(sample(0x34).into())
        .await
        .expect("record");
    drop(sink);

    server.await.expect("join");
    let records = received.lock().expect("lock");
    assert_eq!(records.len(), 1);
    let first = records.first().expect("first record");
    assert_eq!(first.id, FaultId(0x34));
}
