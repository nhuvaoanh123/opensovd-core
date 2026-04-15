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

//! `[backend]` TOML section shapes for sovd-main.
//!
//! Per ADR-0016, `sovd-main` picks the concrete backend impls at
//! runtime from a TOML config. The `score` Cargo feature at compile
//! time just controls whether the S-CORE crates are linked in — the
//! selection of "use SQLite" vs "use S-CORE persistency" is still a
//! TOML-level decision.

use serde::{Deserialize, Serialize};

/// Which persistence backend to select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceBackend {
    /// Default standalone backend: SQLite via sqlx + WAL (ADR-0003).
    #[default]
    Sqlite,
    /// Optional S-CORE backend: score-persistency KVS. Only available
    /// when the `score` Cargo feature is enabled.
    Score,
}

/// Which FaultSink backend to select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FaultSinkBackend {
    /// Default standalone backend: Unix socket (POSIX) or
    /// named pipe (Windows), both carrying the same postcard wire.
    #[default]
    Unix,
    /// Optional S-CORE backend: LoLa zero-copy shared-memory.
    Lola,
}

/// Which OperationCycle backend to select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OperationCycleBackend {
    /// Default standalone backend: in-process tokio::sync::watch state
    /// machine (ADR-0012).
    #[default]
    Taktflow,
    /// Optional S-CORE backend: score-lifecycle event subscriber.
    ScoreLifecycle,
}

/// `[backend]` TOML section.
///
/// Example:
///
/// ```toml
/// [backend]
/// persistence = "sqlite"
/// fault_sink = "unix"
/// operation_cycle = "taktflow"
/// sqlite_path = "./dfm.db"
/// fault_sink_endpoint = "/tmp/opensovd-fault.sock"
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DfmBackendConfig {
    /// Which [`SovdDb`](crate::SovdDb) impl to use.
    pub persistence: PersistenceBackend,
    /// Which [`FaultSink`](sovd_interfaces::traits::fault_sink::FaultSink)
    /// impl to use for the Fault Library ingest path.
    pub fault_sink: FaultSinkBackend,
    /// Which [`OperationCycle`](sovd_interfaces::traits::operation_cycle::OperationCycle)
    /// impl to use for lifecycle events.
    pub operation_cycle: OperationCycleBackend,
    /// Path to the SQLite database file when `persistence = "sqlite"`.
    /// Defaults to `./dfm.db`.
    pub sqlite_path: String,
    /// Unix socket path (POSIX) or named-pipe name (Windows) for the
    /// Fault Library IPC endpoint when `fault_sink = "unix"`.
    pub fault_sink_endpoint: String,
}

impl Default for DfmBackendConfig {
    fn default() -> Self {
        Self {
            persistence: PersistenceBackend::default(),
            fault_sink: FaultSinkBackend::default(),
            operation_cycle: OperationCycleBackend::default(),
            sqlite_path: "./dfm.db".to_owned(),
            #[cfg(unix)]
            fault_sink_endpoint: "/tmp/opensovd-fault.sock".to_owned(),
            #[cfg(windows)]
            fault_sink_endpoint: r"\\.\pipe\opensovd-fault".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_sqlite_unix_taktflow() {
        let c = DfmBackendConfig::default();
        assert_eq!(c.persistence, PersistenceBackend::Sqlite);
        assert_eq!(c.fault_sink, FaultSinkBackend::Unix);
        assert_eq!(c.operation_cycle, OperationCycleBackend::Taktflow);
    }

    #[test]
    fn toml_round_trip() {
        let c = DfmBackendConfig::default();
        let text = toml::to_string(&c).expect("serialize");
        let back: DfmBackendConfig = toml::from_str(&text).expect("deserialize");
        assert_eq!(back.persistence, c.persistence);
        assert_eq!(back.fault_sink, c.fault_sink);
        assert_eq!(back.operation_cycle, c.operation_cycle);
    }

    #[test]
    fn toml_parses_score_variant() {
        let text = r#"
persistence = "score"
fault_sink = "lola"
operation_cycle = "score-lifecycle"
sqlite_path = "./dfm.db"
fault_sink_endpoint = "unused"
"#;
        let c: DfmBackendConfig = toml::from_str(text).expect("deserialize");
        assert_eq!(c.persistence, PersistenceBackend::Score);
        assert_eq!(c.fault_sink, FaultSinkBackend::Lola);
        assert_eq!(c.operation_cycle, OperationCycleBackend::ScoreLifecycle);
    }
}
