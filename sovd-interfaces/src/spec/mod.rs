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

//! Spec-derived DTOs ported from the ASAM SOVD v1.1.0-rc1 `OpenAPI` template
//! (ISO 17978-3 ed.1).
//!
//! # Provenance
//!
//! Every type in this module is a Rust port of a named (or in a few cases
//! inline) schema from the ASAM SOVD v1.1.0-rc1 `OpenAPI` YAML tree, which we
//! ingest read-only at:
//!
//! `H:/eclipse-opensovd/external/asam-public/ISO_17978-3_openapi/openapi-specification-1.1.0-rc1/`
//!
//! See [`docs/openapi-audit-2026-04-14.md`](../../../docs/openapi-audit-2026-04-14.md)
//! for the full audit, schema list, and license analysis.
//!
//! # Clean-room discipline
//!
//! - Field names are facts about the wire protocol and are taken from the
//!   spec verbatim (`snake_case` as the spec uses).
//! - Doc comments **paraphrase** what each field is for; spec descriptions
//!   are not copied verbatim. Each type doc comment carries an upstream
//!   `Provenance:` reference of the form `<file>#<schema>` so a reader can
//!   cross-check against the YAML.
//! - The `OpenAPI` YAMLs themselves are **not** vendored into this repository
//!   and are not redistributed in any release artifact.
//!
//! # Spec license
//!
//! ASAM SOVD v1.1.0-rc1 © ASAM e.V. 2025
//! Licensing terms: <https://www.asam.net/license/>
//!
//! Any operational use of the spec is per ASAM license terms. Code in this
//! module is distributed under Apache-2.0 (see workspace `LICENSE`) — the
//! Rust types here are clean-room derivations of schema **shapes**, not
//! copies of the spec text.
//!
//! # `OpenAPI` generation
//!
//! Every type derives [`utoipa::ToSchema`]. Snapshot tests under
//! `tests/snapshots/` lock the generated JSON Schema output so drift against
//! the spec is visible in `git diff`.

pub mod component;
pub mod data;
pub mod error;
pub mod fault;
pub mod mode;
pub mod operation;
