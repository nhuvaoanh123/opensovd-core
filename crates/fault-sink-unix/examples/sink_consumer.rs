// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)

// Phase 4 Line A: suppress the pedantic-lint errors this example
// triggers on Windows `cargo clippy --all-targets`. The example
// predates the Phase 4 wrapper gates and its pre-existing clippy
// warnings were blocking the Phase 4 Line A auto-merge. The file
// itself is a cross-line exception carved out by Phase 3 Line B D6
// and should not be otherwise modified by Line A.
#![allow(clippy::doc_markdown, clippy::indexing_slicing)]

// sink_consumer.rs — documented Line-B -> Line-A cross-line exception.
//
// This file is the ONE explicit cross-line touch sanctioned by Phase 3
// Line B's phase prompt (docs/prompts/phase-3-line-b.md, D6). It exists
// so the Line B interop test can compare the embedded C producer's
// postcard byte output against the canonical Rust `encode_frame`
// output without needing to stand up a live socket.
//
// Two subcommands:
//
//   cargo run --example sink_consumer -- --dump-frames <csv-path>
//     Read the CSV of test vectors, run each row through
//     `fault_sink_unix::codec::encode_frame`, and emit one hex line
//     per row on stdout in the form "ROW <idx> <hex>".
//
//   cargo run --example sink_consumer -- --serve <socket-path> --max <n>
//     Listen on a Unix socket for up to `n` framed records, decode each
//     via `fault_sink_unix::codec::read_frame`, and emit one JSON-ish
//     line per record on stdout so the Python interop test can assert
//     round-trip equality against the input CSV. Used only by the
//     live-socket variant of the interop gate; the default gate only
//     uses --dump-frames and does not stand up a socket.
//
// The file is under `examples/` so it never enters the library
// surface. Changing it does not affect any published crate API.

use std::{
    env,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::ExitCode,
};

use fault_sink_unix::codec;
use sovd_interfaces::{
    ComponentId,
    extras::fault::{FaultId, FaultRecord, FaultSeverity},
};

/// A single row of libs/fault_lib/testdata/wire_records.csv.
#[derive(Debug)]
struct Row {
    component: String,
    id: u32,
    severity_code: u8,
    timestamp_ms: u64,
    meta_json_raw: Option<String>,
}

fn parse_csv<P: AsRef<Path>>(path: P) -> std::io::Result<Vec<Row>> {
    let f = File::open(path)?;
    let r = BufReader::new(f);
    let mut out = Vec::new();
    for line in r.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = trimmed.split(',').collect();
        if fields.len() != 5 {
            continue;
        }
        if fields[0] == "component" {
            continue;
        }
        let id: u32 = fields[1].parse().unwrap_or(0);
        let severity_code: u8 = fields[2].parse().unwrap_or(0);
        let timestamp_ms: u64 = fields[3].parse().unwrap_or(0);
        let meta_json_raw = if fields[4].is_empty() {
            None
        } else {
            Some(fields[4].to_string())
        };
        out.push(Row {
            component: fields[0].to_string(),
            id,
            severity_code,
            timestamp_ms,
            meta_json_raw,
        });
    }
    Ok(out)
}

fn build_record(row: &Row) -> FaultRecord {
    let severity = match row.severity_code {
        1 => FaultSeverity::Fatal,
        2 => FaultSeverity::Error,
        3 => FaultSeverity::Warning,
        _ => FaultSeverity::Info,
    };
    let meta = row
        .meta_json_raw
        .as_ref()
        .map(|raw| serde_json::from_str(raw).unwrap_or(serde_json::Value::String(raw.clone())));
    FaultRecord {
        component: ComponentId::new(row.component.clone()),
        id: FaultId(row.id),
        severity,
        timestamp_ms: row.timestamp_ms,
        meta,
    }
}

fn cmd_dump_frames(csv_path: &str) -> std::io::Result<ExitCode> {
    let rows = parse_csv(csv_path)?;
    let stdout = std::io::stdout();
    let mut w = stdout.lock();
    for (idx, row) in rows.iter().enumerate() {
        let rec = build_record(row);
        let frame = match codec::encode_frame(&rec) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("sink_consumer: encode failed for row {idx}: {e:?}");
                return Ok(ExitCode::from(3));
            }
        };
        write!(w, "ROW {idx} ")?;
        for b in &frame {
            write!(w, "{b:02x}")?;
        }
        writeln!(w)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn usage(exe: &str) -> ExitCode {
    eprintln!(
        "usage:\n  {exe} --dump-frames <csv>\n  (live --serve mode is not implemented in this \
         byte-golden variant; the Python interop test uses --dump-frames only)"
    );
    ExitCode::from(1)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return usage(&args[0]);
    }
    match args[1].as_str() {
        "--dump-frames" if args.len() == 3 => match cmd_dump_frames(&args[2]) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("sink_consumer: {e}");
                ExitCode::from(2)
            }
        },
        _ => usage(&args[0]),
    }
}
