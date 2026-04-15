# `sovd-server` Coverage — Phase 4 Line A D10

The Phase 4 Line A charter (see `docs/prompts/phase-4-line-a.md` D10)
requires at least 70% line coverage on `sovd-server` and
`sovd-gateway`. This file records the measured numbers at the end of
the Phase 4 Line A run, 2026-04-15, along with notes on how to
reproduce and which files are out of scope.

## How to measure

```bash
# install once
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked

# workspace-wide, human-readable summary
cargo llvm-cov --workspace --tests --summary-only

# just the two Phase 4 gate crates
cargo llvm-cov --workspace --tests -p sovd-server -p sovd-gateway --summary-only

# enforce the 70% gate (fails if below the threshold)
cargo llvm-cov --workspace --tests -p sovd-server -p sovd-gateway \
    --fail-under-lines 70 --summary-only
```

## Phase 4 Line A baseline

Measured on 2026-04-15 against commit tip of
`auto/line-a/phase-4-line-a-2026-04-15`:

| File                               | Lines covered |
|------------------------------------|---------------|
| `sovd-server/src/auth.rs`          | 98.78%        |
| `sovd-server/src/backends/cda.rs`  | 50.00% (*)    |
| `sovd-server/src/correlation.rs`   | 95.00%        |
| `sovd-server/src/in_memory.rs`     | 81.69%        |
| `sovd-server/src/lib.rs`           | 100.00%       |
| `sovd-server/src/openapi.rs`       | 100.00%       |
| `sovd-server/src/routes/components.rs` | 100.00%   |
| `sovd-server/src/routes/data.rs`   | 100.00%       |
| `sovd-server/src/routes/error.rs`  | 76.00%        |
| `sovd-server/src/routes/faults.rs` | 94.00%        |
| `sovd-server/src/routes/health.rs` | 91.67%        |
| `sovd-server/src/routes/mod.rs`    | 100.00%       |
| `sovd-server/src/routes/operations.rs` | 100.00%   |
| `sovd-gateway/src/lib.rs`          | 79.59%        |
| `sovd-gateway/src/remote.rs`       | 94.76%        |

**Aggregate sovd-server (including cda.rs)**: 84.6% line coverage.
**Aggregate sovd-gateway**: 85.1% line coverage.

**(*)** `sovd-server::backends::cda.rs` is the Phase 2 forwarder
surface whose full coverage depends on the `TAKTFLOW_BENCH=1`-gated
`phase2_cda_ecusim_smoke` / `phase4_sovd_gateway_cda_ecusim_bench`
tests. Developer workstations without the bench LAN skip those and
the file's measured coverage drops to the ~50% driven by its pure
unit tests. Under Phase 4 D10 we exclude it from the 70%
enforcement for developer runs, matching the pattern in
`xtask/coverage.toml`. CI that runs with `TAKTFLOW_BENCH=1` should
enforce 70% on the file as well.
