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

//! Spec-derived error envelopes.
//!
//! Provenance:
//! - `commons/errors.yaml#GenericError`
//! - `commons/errors.yaml#DataError`
//!
//! These are the wire-format error bodies returned by every SOVD endpoint
//! on a non-2xx response. The Rust [`crate::types::error::SovdError`] enum
//! is the **internal** error type used inside trait method signatures; it
//! is mapped onto these wire types by `sovd-server` at the HTTP layer.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// SOVD generic error envelope.
///
/// Provenance: `commons/errors.yaml#GenericError`.
///
/// Returned in the body of any non-2xx SOVD response. The `error_code` is
/// a SOVD-standardised code; `vendor_code` is filled when the error is
/// vendor-specific. `parameters` carries open key/value detail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GenericError {
    /// SOVD standardised error code.
    pub error_code: String,

    /// Vendor-specific error code, set when `error_code` is vendor-specific.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor_code: Option<String>,

    /// Human-readable problem description.
    pub message: String,

    /// Identifier for translating `message`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_id: Option<String>,

    /// Additional key/value detail about the error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

/// SOVD partial-error envelope, used when only one element of a structured
/// response failed.
///
/// Provenance: `commons/errors.yaml#DataError`.
///
/// `path` is a JSON Pointer pointing at the offending field of the response
/// body. `error` describes the failure in more detail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DataError {
    /// JSON Pointer to the offending element of the response.
    pub path: String,

    /// Detailed error describing why `path` failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<GenericError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_error_round_trip() {
        let err = GenericError {
            error_code: "vehicle.not_responding".into(),
            vendor_code: Some("oem-0042".into()),
            message: "ECU did not respond within timeout".into(),
            translation_id: Some("tid-vehicle.not_responding".into()),
            parameters: Some(serde_json::json!({"timeout_ms": 1000})),
        };
        let json = serde_json::to_string(&err).expect("serialize");
        let back: GenericError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(err, back);
    }

    #[test]
    fn generic_error_minimum_round_trip() {
        // Only the two required fields per the spec.
        let json = r#"{"error_code":"unknown","message":"boom"}"#;
        let parsed: GenericError = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.error_code, "unknown");
        assert_eq!(parsed.message, "boom");
        assert!(parsed.vendor_code.is_none());
        assert!(parsed.parameters.is_none());
    }

    #[test]
    fn data_error_round_trip() {
        let err = DataError {
            path: "/items/0/code".into(),
            error: Some(GenericError {
                error_code: "format.invalid".into(),
                vendor_code: None,
                message: "code is not a hex string".into(),
                translation_id: None,
                parameters: None,
            }),
        };
        let json = serde_json::to_string(&err).expect("serialize");
        let back: DataError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(err, back);
    }
}
