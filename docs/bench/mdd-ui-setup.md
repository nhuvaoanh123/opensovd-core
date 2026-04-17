# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)

# `mdd-ui` Setup

Phase 5 Line A D11 adds two bench-debugging affordances for live HIL work:

- `console-subscriber` as a `sovd-main` dev-dependency so `tokio-console`
  can be enabled in local debug instrumentation work without reaching
  outside the workspace dependency set.
- `mdd-ui` installed on the Windows dev host for fast inspection of MDD
  diagnostic databases from the terminal.

## `mdd-ui` on the Dev Host

Upstream repository:
- <https://github.com/alexmohr/mdd-ui>

Verified upstream install/build path from the repository README:

```bash
git clone https://github.com/alexmohr/mdd-ui.git
cd mdd-ui
cargo build --release
```

The resulting binary is:

```text
target/release/mdd-ui
```

For a host-local install into the user Cargo bin directory:

```bash
cargo install --git https://github.com/alexmohr/mdd-ui.git --locked
```

Verified on this Windows host on 2026-04-15:

- install command: `cargo install --git https://github.com/alexmohr/mdd-ui.git --locked`
- installed package: `mdd-ui v0.1.0`
- installed git revision: `7e181df9`
- verified command: `mdd-ui --help`

The installed binary path is:

```text
C:\Users\andao\.cargo\bin\mdd-ui.exe
```

Quick smoke checks:

```bash
mdd-ui --help
mdd-ui <PATH_TO_FILE.mdd>
mdd-ui diff <OLD_FILE.mdd> <NEW_FILE.mdd>
```

## `tokio-console` Readiness

D11 does not turn on `console-subscriber` by default in `sovd-main`; it
keeps the crate available as a dev-only dependency so debug-only console
instrumentation can be added without editing workspace dependencies again.

Typical local workflow:

```bash
cargo install tokio-console
RUST_LOG=info cargo run -p sovd-main
tokio-console
```

If live console instrumentation is added in a later slice, keep it behind
debug-only or explicitly opt-in code paths so release and Pi bench builds
do not pay an always-on overhead.
