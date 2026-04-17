<!--
SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)
SPDX-License-Identifier: Apache-2.0
-->

# Phase 5 CDA MDDs

This folder holds generated Classic Diagnostic Adapter MDD clones for
the real Taktflow Phase 5 bench.

They are derived from the upstream `FLXC1000.mdd` template and patched
to expose distinct downstream CDA ids and DoIP logical addresses:

- `CVC00000.mdd` -> `0x0001`
- `FZC00000.mdd` -> `0x0002`
- `RZC00000.mdd` -> `0x0003`

OpenSOVD keeps the external ids `cvc`, `fzc`, and `rzc` via
`remote_component_id` in `deploy/pi/opensovd-pi-phase5-hybrid.toml`.

Regenerate the committed files with:

```bash
cargo run -p xtask -- phase5-cda-mdds
```
