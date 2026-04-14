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

//! Spec-derived operation / routine types.
//!
//! Provenance:
//! - `operations/types.yaml#OperationDescription`
//! - `operations/types.yaml#ExecutionStatus`
//! - `operations/types.yaml#Capability`
//! - `operations/responses.yaml#Operations` (inline schema)
//! - `operations/responses.yaml#OperationDetails` (inline schema)
//! - `operations/responses.yaml#StartExecutionAsynchronous` (inline schema)
//! - `operations/responses.yaml#GetExecutionStatus` (inline schema)
//! - `operations/requests.yaml#StartExecution` (inline schema)
//!
//! In SOVD, what UDS / classic ECUs call a "routine" is called an
//! **operation**. An operation is started by `POST` to its `executions`
//! collection; a successful synchronous start returns `200`, and a
//! successful asynchronous start returns `202` with a `Location` header
//! pointing to the per-execution status resource.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::spec::error::{DataError, GenericError};

/// Description of one operation defined for an entity.
///
/// Provenance: `operations/types.yaml#OperationDescription`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct OperationDescription {
    /// Stable identifier for this operation.
    pub id: String,

    /// Human-readable name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Identifier for translating `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_id: Option<String>,

    /// True if the operation requires a fresh proof of physical proximity.
    pub proximity_proof_required: bool,

    /// True if the operation runs asynchronously (POST returns 202).
    pub asynchronous_execution: bool,

    /// Tags attached to this operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Lifecycle state of one operation execution.
///
/// Provenance: `operations/types.yaml#ExecutionStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionStatus {
    /// Execution is still running.
    Running,
    /// Execution finished successfully.
    Completed,
    /// Execution terminated in error.
    Failed,
}

/// Capability that may be applied to an operation execution.
///
/// Provenance: `operations/types.yaml#Capability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    /// Default — start executing the operation.
    Execute,
    /// Stop a running execution.
    Stop,
    /// Freeze a running execution.
    Freeze,
    /// Reset a running execution.
    Reset,
    /// Query status of an execution.
    Status,
}

/// Response body for `GET /{entity-collection}/{entity-id}/operations`.
///
/// Provenance: `operations/responses.yaml#Operations` (inline schema).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct OperationsList {
    /// All operations defined for the entity.
    pub items: Vec<OperationDescription>,

    /// Optional embedded JSON Schema describing the response shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

/// Response body for `GET .../operations/{operation-id}`.
///
/// Provenance: `operations/responses.yaml#OperationDetails` (inline schema).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct OperationDetails {
    /// Description of the operation.
    pub item: OperationDescription,

    /// Proximity challenge, present only if `item.proximity_proof_required`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proximity_challenge: Option<ProximityChallenge>,

    /// Modes the entity must be in for this operation to execute. Open
    /// `object` per spec — we carry it as `serde_json::Value`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<serde_json::Value>,

    /// Optional embedded JSON Schema describing the response shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

/// Request body for `POST .../operations/{operation-id}/executions`.
///
/// Provenance: `operations/requests.yaml#StartExecution` (inline schema).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct StartExecutionRequest {
    /// Server-side timeout in seconds before the execution is auto-stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i64>,

    /// Operation parameters. Open `AnyValue` per spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,

    /// Response to the proximity challenge, if one was issued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proximity_response: Option<String>,
}

/// Request body for `PUT .../operations/{operation-id}/executions/{execution-id}`.
///
/// Provenance: `operations/requests.yaml#OtherCapabilities` (inline schema).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ApplyCapabilityRequest {
    /// Capability to apply.
    pub capability: Capability,

    /// Server-side timeout in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i64>,

    /// Capability parameters. Open `AnyValue` per spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,

    /// Response to the proximity challenge, if one was issued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proximity_response: Option<String>,
}

/// 202 response body for an asynchronous execution start.
///
/// Provenance: `operations/responses.yaml#StartExecutionAsynchronous`
/// (inline schema). The `Location` HTTP header is carried separately by
/// the transport layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct StartExecutionAsyncResponse {
    /// Identifier for tracking the execution.
    pub id: String,

    /// Initial status, normally `Running`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ExecutionStatus>,
}

/// 200 response body for a synchronous execution start.
///
/// Provenance: `operations/responses.yaml#StartExecutionSynchronous`
/// (inline schema).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct StartExecutionSyncResponse {
    /// Operation response parameters. Open `object` per spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,

    /// Error, if the operation failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<GenericError>,
}

/// Response body for `GET .../operations/{op-id}/executions/{execution-id}`.
///
/// Provenance: `operations/responses.yaml#GetExecutionStatus` (inline schema).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ExecutionStatusResponse {
    /// Current status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ExecutionStatus>,

    /// Capability currently being applied.
    pub capability: Capability,

    /// Operation parameters. Open `AnyValue` per spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,

    /// Optional embedded JSON Schema describing the response shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,

    /// Errors raised during execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Vec<DataError>>,
}

/// Response body for `GET .../operations/{op-id}/executions`.
///
/// Provenance: `operations/responses.yaml#GetExecutions` (inline schema).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ExecutionsList {
    /// All currently existing execution identifiers.
    pub items: Vec<String>,
}

/// Proximity-challenge envelope, used by operations that require fresh
/// physical-proximity proof.
///
/// Provenance: `commons/types.yaml#ProximityChallenge`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProximityChallenge {
    /// Challenge string the SOVD client must solve.
    pub challenge: String,

    /// Absolute UTC timestamp at which the challenge expires.
    /// (RFC 3339 / ISO 8601 string.)
    pub valid_until: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_description_round_trip() {
        let op = OperationDescription {
            id: "SteeringAngleControl".into(),
            name: Some("Control the steering angle value".into()),
            translation_id: Some("tid1928653".into()),
            proximity_proof_required: false,
            asynchronous_execution: true,
            tags: None,
        };
        let json = serde_json::to_string(&op).expect("serialize");
        let back: OperationDescription = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(op, back);
    }

    #[test]
    fn execution_status_serialises_lowercase() {
        assert_eq!(
            serde_json::to_string(&ExecutionStatus::Running).expect("serialize"),
            "\"running\""
        );
        assert_eq!(
            serde_json::to_string(&ExecutionStatus::Completed).expect("serialize"),
            "\"completed\""
        );
        assert_eq!(
            serde_json::to_string(&ExecutionStatus::Failed).expect("serialize"),
            "\"failed\""
        );
    }

    #[test]
    fn capability_round_trip() {
        for cap in [
            Capability::Execute,
            Capability::Stop,
            Capability::Freeze,
            Capability::Reset,
            Capability::Status,
        ] {
            let json = serde_json::to_string(&cap).expect("serialize");
            let back: Capability = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(cap, back);
        }
    }

    #[test]
    fn start_execution_request_round_trip() {
        let req = StartExecutionRequest {
            timeout: Some(120),
            parameters: Some(serde_json::json!({
                "control-type": "absolute",
                "angle": 180i32,
            })),
            proximity_response: None,
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let back: StartExecutionRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(req, back);
    }

    #[test]
    fn start_execution_async_response_round_trip() {
        let r = StartExecutionAsyncResponse {
            id: "fd34f39d-06e7-494b-af2d-8928e1458fb0".into(),
            status: Some(ExecutionStatus::Running),
        };
        let json = serde_json::to_string(&r).expect("serialize");
        let back: StartExecutionAsyncResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, back);
    }

    #[test]
    fn execution_status_response_round_trip() {
        let r = ExecutionStatusResponse {
            status: Some(ExecutionStatus::Running),
            capability: Capability::Execute,
            parameters: Some(serde_json::json!({
                "control-type": "absolute",
                "angle": 120.3f64,
            })),
            schema: None,
            error: None,
        };
        let json = serde_json::to_string(&r).expect("serialize");
        let back: ExecutionStatusResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, back);
    }

    #[test]
    fn executions_list_round_trip() {
        let list = ExecutionsList {
            items: vec![
                "fd34f39d-06e7-494b-af2d-8928e1458fb0".into(),
                "00000000-0000-0000-0000-000000000001".into(),
            ],
        };
        let json = serde_json::to_string(&list).expect("serialize");
        let back: ExecutionsList = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(list, back);
    }

    #[test]
    fn operation_details_round_trip() {
        let d = OperationDetails {
            item: OperationDescription {
                id: "SteeringAngleControl".into(),
                name: Some("Control the steering angle value".into()),
                translation_id: None,
                proximity_proof_required: false,
                asynchronous_execution: true,
                tags: None,
            },
            proximity_challenge: None,
            modes: Some(serde_json::json!({"session": "EXTENDED"})),
            schema: None,
        };
        let json = serde_json::to_string(&d).expect("serialize");
        let back: OperationDetails = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(d, back);
    }
}
