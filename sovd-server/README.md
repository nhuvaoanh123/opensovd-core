<!--
SPDX-License-Identifier: Apache-2.0
SPDX-FileCopyrightText: 2026 The Contributors to Eclipse OpenSOVD (see CONTRIBUTORS)
-->

# `sovd-server`

HTTP/REST SOVD server for the Eclipse OpenSOVD core stack. Hosts the axum
route handlers that turn wire-format SOVD requests into calls against a
[`SovdServer`](../sovd-interfaces/src/traits/server.rs) trait impl and
serialise typed responses back out.

## What this crate does

- Owns the axum route handlers for every MVP SOVD endpoint: faults,
  operations, components (entity discovery), and the preserved
  `GET /sovd/v1/health` liveness probe.
- Provides `InMemoryServer` and its per-component trait view
  `InMemoryComponentServer`, which implement the full MVP surface
  against in-memory canned data. This is what the default `sovd-main`
  binary boots and is the vehicle through which Phase 0/1/2 validates
  the typed request/response path end-to-end.
- Maps internal [`SovdError`](../sovd-interfaces/src/types/error.rs) values
  onto spec-defined [`GenericError`](../sovd-interfaces/src/spec/error.rs)
  wire envelopes in `routes::error`, so route handlers can propagate
  errors with `?`.
- Assembles a `utoipa`-generated OpenAPI document from the handler
  annotations plus the spec DTOs' `ToSchema` impls, and exposes it on a
  debug-only `GET /sovd/v1/openapi.json` endpoint for dev tooling.

## Architecture

`InMemoryServer` is a **multi-component store** (holds state for every
demo component). The per-component trait
[`SovdServer`](../sovd-interfaces/src/traits/server.rs) is implemented by
the `InMemoryComponentServer` view obtained from
`InMemoryServer::component_server(&ComponentId)`. Routes hold
`axum::extract::State<Arc<InMemoryServer>>`, read the `{component-id}`
from the URL, and dispatch to the per-component view.

```
 HTTP request
     |
     v
  axum Router              (routes/mod.rs — app_with_server)
     |
     v
  route handler            (routes/{faults,components,operations}.rs)
     |                      # deserialises spec request body
     v
  Arc<InMemoryServer>      (state)
     |
     v
  component_server(id)     (returns InMemoryComponentServer)
     |
     v
  SovdServer trait call    (list_faults, start_execution, ...)
     |                      # returns sovd_interfaces::spec::* DTOs
     v
  Json<spec::*>            (serialised response)
```

This is different from the spec-level `Arc<dyn SovdServer>` pattern the
Phase-0 scaffold originally contemplated: the `SovdServer` trait uses
native `async fn in trait` and is not dyn-safe for `Send` futures, so
the state is the concrete multi-component store rather than a trait
object. Per-component multiplexing still happens at the route-handler
boundary, which matches what `SovdGateway` does at the system level.

## How to run

Use the default config (`server.mode = "in_memory"`):

```bash
cargo run -p sovd-main
```

This boots `InMemoryServer::new_with_demo_data()` on `0.0.0.0:20002`
with three pre-populated Taktflow components: `cvc`, `fzc`, `rzc`.

Smoke-test the endpoints with `curl`:

```bash
# health
curl http://127.0.0.1:20002/sovd/v1/health

# list all components
curl http://127.0.0.1:20002/sovd/v1/components

# entity capabilities for cvc
curl http://127.0.0.1:20002/sovd/v1/components/cvc

# list cvc faults
curl http://127.0.0.1:20002/sovd/v1/components/cvc/faults

# fault details for one code
curl http://127.0.0.1:20002/sovd/v1/components/cvc/faults/P0A1F

# list cvc operations
curl http://127.0.0.1:20002/sovd/v1/components/cvc/operations

# start the motor self test
curl -X POST \
  -H 'content-type: application/json' \
  -d '{"timeout":30,"parameters":{"mode":"quick"}}' \
  http://127.0.0.1:20002/sovd/v1/components/cvc/operations/motor_self_test/executions

# poll execution status (replace {id} with the id returned above)
curl http://127.0.0.1:20002/sovd/v1/components/cvc/operations/motor_self_test/executions/{id}

# clear every fault on cvc
curl -X DELETE http://127.0.0.1:20002/sovd/v1/components/cvc/faults

# clear one fault on cvc
curl -X DELETE http://127.0.0.1:20002/sovd/v1/components/cvc/faults/P0A1F
```

## Hello-world mode (bare health endpoint)

For smoke tests that don't want the full route surface:

```toml
# opensovd.toml
[server]
mode = "hello_world"
```

or

```bash
OPENSOVD_SERVER__MODE=hello_world cargo run -p sovd-main
```

In hello-world mode only `GET /sovd/v1/health` is mounted.

## Regenerating OpenAPI from code

Debug builds expose a dev-only endpoint that returns the current
generated OpenAPI document:

```bash
cargo run -p sovd-main &
curl http://127.0.0.1:20002/sovd/v1/openapi.json > openapi.generated.json
```

The document is assembled at compile time from the `#[utoipa::path]`
attributes on every route handler plus the `ToSchema` derives on the
types in `sovd_interfaces::spec`. The endpoint is gated behind
`cfg(debug_assertions)` so release binaries never expose it. Regeneration
is exercised by `integration-tests/tests/openapi_roundtrip.rs`, which
asserts that every type we register actually appears in the output.

## Phase 3/4 handoff

The `Arc<InMemoryServer>` boundary is the seam where Phase 3/4 swaps in
real backends:

1. **Phase 3 — DFM bridge.** Replace `InMemoryServer` with a struct that
   holds a `DfmClient` handle and implements the same per-component view
   surface. `routes/mod.rs` needs no changes beyond a new state type;
   handler bodies only call `SovdServer` trait methods.
2. **Phase 4 — CDA adapter.** Register a second backend kind (CDA over
   DoIP) behind the same per-component view, either as a `BackendKind`
   variant inside the DFM server or as a sibling that implements
   `SovdBackend`. At that point the multi-component dispatch logic in
   `routes::app_with_server` lifts into `sovd-gateway` and the route
   state becomes `Arc<dyn SovdGateway>`.

The demo data in `InMemoryServer::new_with_demo_data()` stays as fixture
material for integration tests even after the real DFM lands.
