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

#![allow(clippy::doc_markdown)]

//! `cargo xtask` — workspace developer task runner.
//!
//! Currently only implements `openapi-dump`, which regenerates
//! `sovd-server/openapi.yaml` from the live `utoipa::openapi::OpenApi`
//! document in `sovd-server::openapi::ApiDoc`. The staleness gate in
//! `integration-tests/tests/phase4_openapi_staleness.rs` fails the
//! test suite whenever the committed yaml drifts from the live output,
//! so this command is the only blessed way to refresh it.

use std::{
    fs,
    path::{Path, PathBuf},
};

use cda_database::{
    datatypes::{
        self, CompuCategory, DataType, DiagCodedTypeVariant, DiagnosticDatabase, MinMaxLengthType,
        ResponseType, Termination,
        database_builder::{
            Addressing, DOP, DiagClassType, DiagCommParams, DiagLayerParams, DiagServiceParams,
            EcuDataBuilder, EcuDataParams, Param, SimpleComParamEntry, TransmissionMode, WIPOffset,
        },
    },
    load_ecudata,
};
use cda_interfaces::datatypes::{ComParamValue, FlatbBufConfig};
use clap::{Parser, Subcommand};
use prost::Message;
use utoipa::OpenApi;

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "OpenSOVD workspace task runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Regenerate `sovd-server/openapi.yaml` from the live ApiDoc.
    OpenapiDump {
        /// Print a diff to stderr and exit non-zero instead of
        /// overwriting the file. Used in CI staleness gates.
        #[arg(long)]
        check: bool,
    },
    /// Generate the Phase 5 CDA MDD clones used for the real Taktflow bench.
    Phase5CdaMdds {
        /// Check that the committed generated files are up to date instead
        /// of rewriting them.
        #[arg(long)]
        check: bool,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::OpenapiDump { check } => match openapi_dump(check) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask openapi-dump failed: {e}");
                std::process::ExitCode::FAILURE
            }
        },
        Command::Phase5CdaMdds { check } => match phase5_cda_mdds(check) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask phase5-cda-mdds failed: {e}");
                std::process::ExitCode::FAILURE
            }
        },
    }
}

fn openapi_dump(check: bool) -> Result<(), Box<dyn std::error::Error>> {
    let live = sovd_server::openapi::ApiDoc::openapi().to_yaml()?;

    let path = openapi_yaml_path();
    if check {
        let committed = fs::read_to_string(&path)?;
        let live_normalised = live.replace("\r\n", "\n");
        let committed_normalised = committed.replace("\r\n", "\n");
        if live_normalised.trim() != committed_normalised.trim() {
            eprintln!("openapi.yaml is stale at {}", path.display());
            return Err("openapi.yaml staleness gate failed".into());
        }
        eprintln!("openapi.yaml is in sync");
        return Ok(());
    }

    // Ensure a trailing newline on write so git's diff view looks
    // tidy even on editors that do not auto-append one.
    let mut buf = live;
    if !buf.ends_with('\n') {
        buf.push('\n');
    }
    fs::write(&path, buf)?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

fn openapi_yaml_path() -> PathBuf {
    workspace_root().join("sovd-server").join("openapi.yaml")
}

#[derive(Clone, Copy, Debug)]
struct Phase5MddSpec {
    remote_component_id_upper: &'static str,
    logical_address_decimal: &'static str,
    include_motor_self_test: bool,
}

const PHASE5_FUNCTIONAL_ADDRESS: &str = "65535";
const PHASE5_MDD_MAGIC: [u8; 20] = [
    0x4d, 0x44, 0x44, 0x20, 0x76, 0x65, 0x72, 0x73, 0x69, 0x6f, 0x6e, 0x20, 0x30, 0x20, 0x20, 0x20,
    0x20, 0x20, 0x20, 0x00,
];
const PHASE5_MDD_SPECS: &[Phase5MddSpec] = &[
    Phase5MddSpec {
        remote_component_id_upper: "CVC00000",
        logical_address_decimal: "0001",
        include_motor_self_test: false,
    },
    Phase5MddSpec {
        remote_component_id_upper: "FZC00000",
        logical_address_decimal: "0002",
        include_motor_self_test: false,
    },
    Phase5MddSpec {
        remote_component_id_upper: "RZC00000",
        logical_address_decimal: "0003",
        include_motor_self_test: true,
    },
];

const PHASE5_DTC_RECORDS: &[(u32, &str, &str, u32)] = &[
    (0xC00100, "C00100", "Pedal Sensor Plausibility Failure", 2),
    (0xC00200, "C00200", "Pedal Sensor 1 Communication Loss", 2),
    (0xC00300, "C00300", "Pedal Sensor 2 Communication Loss", 2),
    (0xC00400, "C00400", "Both Pedal Sensors Failed", 2),
    (0xC10100, "C10100", "CAN Rx Timeout (FZC Lost CVC)", 2),
    (0xC10200, "C10200", "CAN Rx Timeout (RZC Lost CVC)", 2),
    (0xC10300, "C10300", "Can Bus Off", 2),
    (0xC10400, "C10400", "E2E CRC Error On Safety Message", 2),
    (0xC10500, "C10500", "E2E Alive Counter Error", 2),
    (0xC20100, "C20100", "Motor Overcurrent", 2),
    (0xC20200, "C20200", "Motor Overtemperature", 2),
    (0xC20300, "C20300", "Motor Overtemperature Warning", 1),
    (0xC20001, "C20001", "Motor Overcurrent", 2),
    (0xC20002, "C20002", "Motor Overtemperature", 2),
    (0xC20004, "C20004", "Battery Low", 1),
    (0xC30001, "C30001", "Lidar Blocked", 1),
    (0xC30002, "C30002", "Steering Lost", 2),
    (0xC30100, "C30100", "Steering Angle Sensor Failure", 2),
    (0xC30200, "C30200", "Steering Servo Jammed", 2),
    (0xC30300, "C30300", "Lidar Sensor Failure", 1),
    (0xC40100, "C40100", "E-Stop Activated", 2),
    (0xC50100, "C50100", "WdgM Supervision Expired", 2),
    (0xC50200, "C50200", "BswM Mode Transition Failure", 2),
];

#[derive(Clone, PartialEq, prost::Message)]
struct GeneratedEncryption {
    #[prost(string, tag = "1")]
    encryption_algorithm: String,
    #[prost(bytes = "vec", optional, tag = "2")]
    key_identifier: Option<Vec<u8>>,
    #[prost(bytes = "vec", repeated, tag = "3")]
    certificates: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct GeneratedSignature {
    #[prost(string, tag = "1")]
    algorithm: String,
    #[prost(bytes = "vec", optional, tag = "2")]
    key_identifier: Option<Vec<u8>>,
    #[prost(map = "string, string", tag = "3")]
    metadata: std::collections::HashMap<String, String>,
    #[prost(bytes = "vec", tag = "4")]
    signature: Vec<u8>,
    #[prost(bytes = "vec", repeated, tag = "5")]
    certificates: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct GeneratedChunk {
    #[prost(enumeration = "generated_chunk::DataType", tag = "1")]
    r#type: i32,
    #[prost(string, optional, tag = "2")]
    name: Option<String>,
    #[prost(map = "string, string", tag = "3")]
    metadata: std::collections::HashMap<String, String>,
    #[prost(message, repeated, tag = "4")]
    signatures: Vec<GeneratedSignature>,
    #[prost(string, optional, tag = "5")]
    compression_algorithm: Option<String>,
    #[prost(uint64, optional, tag = "6")]
    uncompressed_size: Option<u64>,
    #[prost(message, optional, tag = "7")]
    encryption: Option<GeneratedEncryption>,
    #[prost(bytes = "vec", optional, tag = "8")]
    data: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    mime_type: Option<String>,
}

mod generated_chunk {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
    #[repr(i32)]
    pub enum DataType {
        DiagnosticDescription = 0,
        CodeFile = 1,
        CodeFilePartial = 2,
        EmbeddedFile = 3,
        VendorSpecific = 1024,
    }
}

#[derive(Clone, PartialEq, prost::Message)]
struct GeneratedMddFile {
    #[prost(string, tag = "1")]
    version: String,
    #[prost(enumeration = "generated_mdd_file::FeatureFlag", repeated, tag = "2")]
    feature_flags: Vec<i32>,
    #[prost(string, tag = "3")]
    ecu_name: String,
    #[prost(string, optional, tag = "4")]
    revision: Option<String>,
    #[prost(map = "string, string", tag = "5")]
    metadata: std::collections::HashMap<String, String>,
    #[prost(message, repeated, tag = "6")]
    chunks: Vec<GeneratedChunk>,
    #[prost(message, optional, tag = "7")]
    chunks_signature: Option<GeneratedSignature>,
}

mod generated_mdd_file {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
    #[repr(i32)]
    pub enum FeatureFlag {
        Reserved = 0,
    }
}

fn phase5_cda_mdds(check: bool) -> Result<(), Box<dyn std::error::Error>> {
    let source_license = phase5_template_mdd_license_path();
    let output_dir = phase5_output_dir();
    let license_body = fs::read_to_string(&source_license)?;

    if !check {
        fs::create_dir_all(&output_dir)?;
    }

    for spec in PHASE5_MDD_SPECS {
        let output = output_dir.join(format!("{}.mdd", spec.remote_component_id_upper));
        let output_license =
            output_dir.join(format!("{}.mdd.license", spec.remote_component_id_upper));

        if check {
            validate_phase5_mdd(&output, spec)?;
            let committed_license = fs::read_to_string(&output_license)?;
            if committed_license != license_body {
                return Err(format!(
                    "{} is stale; rerun `cargo run -p xtask -- phase5-cda-mdds`",
                    output_license.display()
                )
                .into());
            }
        } else {
            let generated = generate_phase5_mdd(spec)?;
            fs::write(&output, generated)?;
            fs::write(&output_license, &license_body)?;
            validate_phase5_mdd(&output, spec)?;
            eprintln!("wrote {}", output.display());
            eprintln!("wrote {}", output_license.display());
        }
    }

    Ok(())
}

fn generate_phase5_mdd(spec: &Phase5MddSpec) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let diag_blob = build_phase5_diag_blob(spec)?;
    let mdd = GeneratedMddFile {
        version: "0".to_owned(),
        feature_flags: Vec::new(),
        ecu_name: spec.remote_component_id_upper.to_owned(),
        revision: Some("1".to_owned()),
        metadata: std::collections::HashMap::new(),
        chunks: vec![GeneratedChunk {
            r#type: generated_chunk::DataType::DiagnosticDescription as i32,
            name: Some("diagnostic-description".to_owned()),
            metadata: std::collections::HashMap::new(),
            signatures: Vec::new(),
            compression_algorithm: None,
            uncompressed_size: None,
            encryption: None,
            data: Some(diag_blob),
            mime_type: None,
        }],
        chunks_signature: None,
    };
    let mut bytes = Vec::with_capacity(PHASE5_MDD_MAGIC.len().saturating_add(mdd.encoded_len()));
    bytes.extend_from_slice(&PHASE5_MDD_MAGIC);
    mdd.encode(&mut bytes)?;
    Ok(bytes)
}

fn build_status_parameters<'a>(
    builder: &mut EcuDataBuilder<'a>,
    bool_dop: WIPOffset<DOP<'a>>,
    byte_position: u32,
) -> Vec<WIPOffset<Param<'a>>> {
    vec![
        builder.create_value_param("testFailed", bool_dop, byte_position, 0),
        builder.create_value_param("testFailedThisOperationCycle", bool_dop, byte_position, 1),
        builder.create_value_param("pendingDTC", bool_dop, byte_position, 2),
        builder.create_value_param("confirmedDTC", bool_dop, byte_position, 3),
        builder.create_value_param("testNotCompletedSinceLastClear", bool_dop, byte_position, 4),
        builder.create_value_param("testFailedSinceLastClear", bool_dop, byte_position, 5),
        builder.create_value_param(
            "testNotCompletedThisOperationCycle",
            bool_dop,
            byte_position,
            6,
        ),
        builder.create_value_param("warningIndicatorRequested", bool_dop, byte_position, 7),
    ]
}

fn build_phase5_diag_blob(spec: &Phase5MddSpec) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut builder = EcuDataBuilder::new();

    let protocol = builder.create_protocol("UDS_Ethernet_DoIP", None, None, None);
    let protocol_dobt = builder.create_protocol("UDS_Ethernet_DoIP_DOBT", None, None, None);
    let identical_compu = builder.create_compu_method(CompuCategory::Identical, None, None);
    let u1_diag_type = builder.create_diag_coded_type_standard_length(1, DataType::UInt32);
    let u8_diag_type = builder.create_diag_coded_type_standard_length(8, DataType::UInt32);
    let u16_diag_type = builder.create_diag_coded_type_standard_length(16, DataType::UInt32);
    let u24_diag_type = builder.create_diag_coded_type_standard_length(24, DataType::UInt32);
    let ascii_diag_type = builder.create_diag_coded_type(
        None,
        DataType::AsciiString,
        true,
        DiagCodedTypeVariant::MinMaxLength(MinMaxLengthType::new(
            1,
            Some(128),
            Termination::EndOfPdu,
        )?),
    );

    let bool_dop =
        builder.create_regular_normal_dop("true_false_dop", u1_diag_type, identical_compu);
    let u8_dop = builder.create_regular_normal_dop("u8_dop", u8_diag_type, identical_compu);
    let u16_dop = builder.create_regular_normal_dop("u16_dop", u16_diag_type, identical_compu);
    let _ascii_dop =
        builder.create_regular_normal_dop("ascii_dop", ascii_diag_type, identical_compu);

    let cp_refs = vec![
        builder.create_simple_com_param_ref(
            "CP_DoIPLogicalGatewayAddress",
            "COM",
            Some("1"),
            u16_dop,
            protocol,
            spec.logical_address_decimal,
        ),
        builder.create_simple_com_param_ref(
            "CP_DoIPLogicalFunctionalAddress",
            "COM",
            Some(PHASE5_FUNCTIONAL_ADDRESS),
            u16_dop,
            protocol,
            PHASE5_FUNCTIONAL_ADDRESS,
        ),
        builder.create_complex_com_param_ref_from_simple_entries(
            "CP_UniqueRespIdTable",
            "UNIQUE_ID",
            protocol,
            vec![
                SimpleComParamEntry {
                    short_name: "CP_DoIPLogicalEcuAddress",
                    param_class: "UNIQUE_ID",
                    physical_default_value: Some("1"),
                    dop: u16_dop,
                    value: spec.logical_address_decimal,
                },
                SimpleComParamEntry {
                    short_name: "CP_DoIPSecondaryLogicalECUResponseAddress",
                    param_class: "UNIQUE_ID",
                    physical_default_value: Some("0"),
                    dop: u16_dop,
                    value: "0",
                },
                SimpleComParamEntry {
                    short_name: "CP_ECULayerShortName",
                    param_class: "UNIQUE_ID",
                    physical_default_value: Some("None"),
                    dop: u16_dop,
                    value: spec.remote_component_id_upper,
                },
            ],
            true,
        ),
        builder.create_simple_com_param_ref(
            "CP_DoIPLogicalGatewayAddress",
            "COM",
            Some("1"),
            u16_dop,
            protocol_dobt,
            spec.logical_address_decimal,
        ),
        builder.create_simple_com_param_ref(
            "CP_DoIPLogicalFunctionalAddress",
            "COM",
            Some(PHASE5_FUNCTIONAL_ADDRESS),
            u16_dop,
            protocol_dobt,
            PHASE5_FUNCTIONAL_ADDRESS,
        ),
        builder.create_complex_com_param_ref_from_simple_entries(
            "CP_UniqueRespIdTable",
            "UNIQUE_ID",
            protocol_dobt,
            vec![
                SimpleComParamEntry {
                    short_name: "CP_DoIPLogicalEcuAddress",
                    param_class: "UNIQUE_ID",
                    physical_default_value: Some("1"),
                    dop: u16_dop,
                    value: spec.logical_address_decimal,
                },
                SimpleComParamEntry {
                    short_name: "CP_DoIPSecondaryLogicalECUResponseAddress",
                    param_class: "UNIQUE_ID",
                    physical_default_value: Some("0"),
                    dop: u16_dop,
                    value: "0",
                },
                SimpleComParamEntry {
                    short_name: "CP_ECULayerShortName",
                    param_class: "UNIQUE_ID",
                    physical_default_value: Some("None"),
                    dop: u16_dop,
                    value: spec.remote_component_id_upper,
                },
            ],
            true,
        ),
    ];

    let fault_mem_class = builder.create_funct_class("FaultMem");

    let dtcs = PHASE5_DTC_RECORDS
        .iter()
        .map(|(code, display_code, fault_name, severity)| {
            builder.create_dtc(*code, Some(display_code), Some(fault_name), *severity)
        })
        .collect::<Vec<_>>();
    let dtc_dop = builder.create_dtc_dop(u24_diag_type, Some(dtcs), Some(identical_compu));
    let mut dtc_record_params = vec![builder.create_value_param("DtcRecord", dtc_dop, 0, 0)];
    dtc_record_params.extend(build_status_parameters(&mut builder, bool_dop, 3));
    let dtc_record_structure = builder.create_structure(Some(dtc_record_params), None, true);
    let dtc_end_of_pdu = builder.create_end_of_pdu_field_dop(0, None, Some(dtc_record_structure));

    let mut diag_services = Vec::new();

    let fault_mem_report_by_status_mask = {
        let diag_comm = builder.create_diag_comm(DiagCommParams {
            short_name: "FaultMem_ReportDTCByStatusMask",
            long_name: Some("Report DTC By Status Mask"),
            funct_class: Some(vec![fault_mem_class]),
            diag_class_type: DiagClassType::START_COMM,
            protocols: Some(vec![protocol, protocol_dobt]),
            ..Default::default()
        });
        let mut request_params = vec![
            builder.create_coded_const_param("SID_RQ", "25", 0, 0, 8, DataType::UInt32),
            builder.create_coded_const_param("SubFunction", "2", 1, 0, 8, DataType::UInt32),
        ];
        request_params.extend(build_status_parameters(&mut builder, bool_dop, 2));
        let request = builder.create_request(Some(request_params), None);
        let mut response_params = vec![
            builder.create_coded_const_param("SID_PR", "89", 0, 0, 8, DataType::UInt32),
            builder.create_coded_const_param("SubFunction_PR", "2", 1, 0, 8, DataType::UInt32),
        ];
        response_params.extend(build_status_parameters(&mut builder, bool_dop, 2));
        response_params.push(builder.create_value_param(
            "DTCAndStatusRecord",
            dtc_end_of_pdu,
            3,
            0,
        ));
        let response = builder.create_response(ResponseType::Positive, Some(response_params), None);
        builder.create_diag_service(DiagServiceParams {
            diag_comm: Some(diag_comm),
            request: Some(request),
            pos_responses: vec![response],
            neg_responses: Vec::new(),
            addressing: *Addressing::FUNCTIONAL_OR_PHYSICAL,
            transmission_mode: *TransmissionMode::SEND_AND_RECEIVE,
            ..Default::default()
        })
    };
    diag_services.push(fault_mem_report_by_status_mask);

    for (name, subfunction, description) in [
        (
            "FaultMem_ReportDTCSnapshotRecordByDtcNumber",
            4u32,
            "Report DTC Snapshot Record By Dtc Number",
        ),
        (
            "FaultMem_ReportDTCExtDataRecordByDtcNumber",
            6u32,
            "Report DTC Extended Data Record By Dtc Number",
        ),
    ] {
        let diag_comm = builder.create_diag_comm(DiagCommParams {
            short_name: name,
            long_name: Some(description),
            funct_class: Some(vec![fault_mem_class]),
            diag_class_type: DiagClassType::START_COMM,
            protocols: Some(vec![protocol, protocol_dobt]),
            ..Default::default()
        });
        let subfunction_string = subfunction.to_string();
        let request_params = vec![
            builder.create_coded_const_param("SID_RQ", "25", 0, 0, 8, DataType::UInt32),
            builder.create_coded_const_param(
                "SubFunction",
                &subfunction_string,
                1,
                0,
                8,
                DataType::UInt32,
            ),
            builder.create_value_param("DtcCode", dtc_dop, 2, 0),
            builder.create_value_param("DTCSnapshotRecordNr", u8_dop, 5, 0),
        ];
        let request = builder.create_request(Some(request_params), None);
        let response_params = vec![
            builder.create_coded_const_param("SID_PR", "89", 0, 0, 8, DataType::UInt32),
            builder.create_coded_const_param(
                "SubFunction_PR",
                &subfunction_string,
                1,
                0,
                8,
                DataType::UInt32,
            ),
            builder.create_value_param("DTCAndStatusRecord", dtc_end_of_pdu, 2, 0),
        ];
        let response = builder.create_response(ResponseType::Positive, Some(response_params), None);
        diag_services.push(builder.create_diag_service(DiagServiceParams {
            diag_comm: Some(diag_comm),
            request: Some(request),
            pos_responses: vec![response],
            neg_responses: Vec::new(),
            addressing: *Addressing::FUNCTIONAL_OR_PHYSICAL,
            transmission_mode: *TransmissionMode::SEND_AND_RECEIVE,
            ..Default::default()
        }));
    }

    let fault_mem_clear_dtcs = {
        let diag_comm = builder.create_diag_comm(DiagCommParams {
            short_name: "FaultMem_ClearDTCs",
            long_name: Some("Clear DTCs"),
            funct_class: Some(vec![fault_mem_class]),
            diag_class_type: DiagClassType::START_COMM,
            protocols: Some(vec![protocol, protocol_dobt]),
            ..Default::default()
        });
        let request_params = vec![
            builder.create_coded_const_param("SID_RQ", "20", 0, 0, 8, DataType::UInt32),
            builder.create_value_param("Dtc", dtc_dop, 1, 0),
        ];
        let request = builder.create_request(Some(request_params), None);
        let response_params =
            vec![builder.create_coded_const_param("SID_PR", "84", 0, 0, 8, DataType::UInt32)];
        let response = builder.create_response(ResponseType::Positive, Some(response_params), None);
        builder.create_diag_service(DiagServiceParams {
            diag_comm: Some(diag_comm),
            request: Some(request),
            pos_responses: vec![response],
            neg_responses: Vec::new(),
            addressing: *Addressing::FUNCTIONAL_OR_PHYSICAL,
            transmission_mode: *TransmissionMode::SEND_AND_RECEIVE,
            ..Default::default()
        })
    };
    diag_services.push(fault_mem_clear_dtcs);

    let clear_user_memory = {
        let diag_comm = builder.create_diag_comm(DiagCommParams {
            short_name: "Clear_Diagnostic_User_Memory",
            long_name: Some("Clear User-Defined DTC Memory"),
            funct_class: Some(vec![fault_mem_class]),
            diag_class_type: DiagClassType::START_COMM,
            protocols: Some(vec![protocol, protocol_dobt]),
            ..Default::default()
        });
        let request_params = vec![
            builder.create_coded_const_param("SID_RQ", "49", 0, 0, 8, DataType::UInt32),
            builder.create_coded_const_param("RoutineControlType", "1", 1, 0, 8, DataType::UInt32),
            builder.create_coded_const_param("RoutineId", "16896", 2, 0, 16, DataType::UInt32),
        ];
        let request = builder.create_request(Some(request_params), None);
        let response_params = vec![
            builder.create_coded_const_param("SID_PR", "113", 0, 0, 8, DataType::UInt32),
            builder.create_coded_const_param(
                "RoutineControlType_PR",
                "1",
                1,
                0,
                8,
                DataType::UInt32,
            ),
            builder.create_coded_const_param("RoutineId_PR", "16896", 2, 0, 16, DataType::UInt32),
        ];
        let response = builder.create_response(ResponseType::Positive, Some(response_params), None);
        builder.create_diag_service(DiagServiceParams {
            diag_comm: Some(diag_comm),
            request: Some(request),
            pos_responses: vec![response],
            neg_responses: Vec::new(),
            addressing: *Addressing::FUNCTIONAL_OR_PHYSICAL,
            transmission_mode: *TransmissionMode::SEND_AND_RECEIVE,
            ..Default::default()
        })
    };
    diag_services.push(clear_user_memory);

    if spec.include_motor_self_test {
        let motor_self_test = {
            let diag_comm = builder.create_diag_comm(DiagCommParams {
                short_name: "motor_self_test_start",
                long_name: Some("Motor Self Test"),
                diag_class_type: DiagClassType::START_COMM,
                protocols: Some(vec![protocol, protocol_dobt]),
                ..Default::default()
            });
            let request_params = vec![
                builder.create_coded_const_param("SID_RQ", "49", 0, 0, 8, DataType::UInt32),
                builder.create_coded_const_param(
                    "RoutineControlType",
                    "1",
                    1,
                    0,
                    8,
                    DataType::UInt32,
                ),
                builder.create_coded_const_param("RoutineId", "21008", 2, 0, 16, DataType::UInt32),
                builder.create_coded_const_param("mode", "1", 4, 0, 8, DataType::UInt32),
            ];
            let request = builder.create_request(Some(request_params), None);
            let response_params = vec![
                builder.create_coded_const_param("SID_PR", "113", 0, 0, 8, DataType::UInt32),
                builder.create_coded_const_param(
                    "RoutineControlType_PR",
                    "1",
                    1,
                    0,
                    8,
                    DataType::UInt32,
                ),
                builder.create_coded_const_param(
                    "RoutineId_PR",
                    "21008",
                    2,
                    0,
                    16,
                    DataType::UInt32,
                ),
                builder.create_value_param("result", u8_dop, 4, 0),
            ];
            let response =
                builder.create_response(ResponseType::Positive, Some(response_params), None);
            builder.create_diag_service(DiagServiceParams {
                diag_comm: Some(diag_comm),
                request: Some(request),
                pos_responses: vec![response],
                neg_responses: Vec::new(),
                addressing: *Addressing::FUNCTIONAL_OR_PHYSICAL,
                transmission_mode: *TransmissionMode::SEND_AND_RECEIVE,
                ..Default::default()
            })
        };
        diag_services.push(motor_self_test);
    }

    let diag_layer = builder.create_diag_layer(DiagLayerParams {
        short_name: spec.remote_component_id_upper,
        com_param_refs: Some(cp_refs),
        diag_services: Some(diag_services),
        ..Default::default()
    });
    let variant = builder.create_variant(diag_layer, true, None, None);
    Ok(builder.finish_to_vec(EcuDataParams {
        ecu_name: spec.remote_component_id_upper,
        revision: "1",
        version: "1.0.0",
        variants: Some(vec![variant]),
        ..Default::default()
    }))
}

fn validate_phase5_mdd(
    output: &Path,
    spec: &Phase5MddSpec,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = output
        .to_str()
        .ok_or_else(|| format!("non-UTF8 output path: {}", output.display()))?;
    let (proto_name, blob) =
        load_ecudata(path).map_err(|e| format!("load_ecudata {}: {e}", output.display()))?;
    if proto_name != spec.remote_component_id_upper {
        return Err(format!(
            "{} proto ecu_name mismatch: expected {}, got {}",
            output.display(),
            spec.remote_component_id_upper,
            proto_name
        )
        .into());
    }

    let db = DiagnosticDatabase::new_from_bytes(path.to_owned(), blob, FlatbBufConfig::default())
        .map_err(|e| format!("decode {}: {e}", output.display()))?;
    let db_name = db
        .ecu_name()
        .map_err(|e| format!("ecu_name {}: {e}", output.display()))?;
    if db_name != spec.remote_component_id_upper {
        return Err(format!(
            "{} database ecu_name mismatch: expected {}, got {}",
            output.display(),
            spec.remote_component_id_upper,
            db_name
        )
        .into());
    }

    let base = db
        .base_variant()
        .map_err(|e| format!("base_variant {}: {e}", output.display()))?;
    let Some(diag_layer) = base.diag_layer() else {
        return Err(format!("{} base variant has no diag_layer", output.display()).into());
    };
    let Some(cp_refs) = diag_layer.com_param_refs() else {
        return Err(format!("{} diag_layer has no com_param_refs", output.display()).into());
    };

    let mut saw_gateway_address = false;
    let mut saw_unique_resp_table = false;
    for cp_ref in cp_refs.iter() {
        let (name, value) =
            datatypes::resolve_comparam(&cp_ref).map_err(|e| format!("resolve_comparam: {e}"))?;
        if name == "CP_DoIPLogicalGatewayAddress" {
            let rendered = format_com_param(&value);
            if rendered == spec.logical_address_decimal {
                saw_gateway_address = true;
            }
        }
        if name == "CP_UniqueRespIdTable" {
            let rendered = format_com_param(&value);
            if rendered.contains(&format!(
                "CP_DoIPLogicalEcuAddress={}",
                spec.logical_address_decimal
            )) && rendered.contains(&format!(
                "CP_ECULayerShortName={}",
                spec.remote_component_id_upper
            )) {
                saw_unique_resp_table = true;
            }
        }
    }

    if !saw_gateway_address {
        return Err(format!(
            "{} missing CP_DoIPLogicalGatewayAddress={}",
            output.display(),
            spec.logical_address_decimal
        )
        .into());
    }
    if !saw_unique_resp_table {
        return Err(format!(
            "{} missing CP_UniqueRespIdTable alias for {} / {}",
            output.display(),
            spec.remote_component_id_upper,
            spec.logical_address_decimal
        )
        .into());
    }
    let Some(diag_services) = diag_layer.diag_services() else {
        return Err(format!("{} diag_layer has no diag_services", output.display()).into());
    };
    let mut has_fault_read_by_status = false;
    let mut has_fault_snapshot = false;
    let mut has_fault_ext_data = false;
    let mut service_signatures = Vec::new();
    for service in diag_services.iter() {
        let service = datatypes::DiagService(service);
        let short_name = service
            .diag_comm()
            .and_then(|diag_comm| diag_comm.short_name().map(ToOwned::to_owned))
            .unwrap_or_else(|| "<unnamed>".to_owned());
        let request_id = service.request_id();
        let subfunction = service.request_sub_function_id().map(|(value, _)| value);
        service_signatures.push(format!(
            "{short_name}:sid={request_id:?}:subfunction={subfunction:?}"
        ));
        if request_id == Some(0x19) && subfunction == Some(0x02) {
            has_fault_read_by_status = true;
        }
        if request_id == Some(0x19) && subfunction == Some(0x04) {
            has_fault_snapshot = true;
        }
        if request_id == Some(0x19) && subfunction == Some(0x06) {
            has_fault_ext_data = true;
        }
    }
    if !has_fault_read_by_status {
        return Err(format!(
            "{} missing SID 0x19 subfunction 0x02 fault service; saw {}",
            output.display(),
            service_signatures.join(", ")
        )
        .into());
    }
    if !has_fault_snapshot {
        return Err(format!(
            "{} missing SID 0x19 subfunction 0x04 fault service; saw {}",
            output.display(),
            service_signatures.join(", ")
        )
        .into());
    }
    if !has_fault_ext_data {
        return Err(format!(
            "{} missing SID 0x19 subfunction 0x06 fault service; saw {}",
            output.display(),
            service_signatures.join(", ")
        )
        .into());
    }
    if spec.include_motor_self_test {
        let saw_motor_self_test = diag_services.iter().any(|service| {
            let service = datatypes::DiagService(service);
            service
                .diag_comm()
                .and_then(|diag_comm| diag_comm.short_name())
                .is_some_and(|name| name == "motor_self_test_start")
                && service.request_id() == Some(0x31)
        });
        if !saw_motor_self_test {
            return Err(format!(
                "{} missing motor_self_test_start diagnostic operation",
                output.display()
            )
            .into());
        }
    }

    Ok(())
}

fn format_com_param(value: &ComParamValue) -> String {
    match value {
        ComParamValue::Simple(simple) => simple.value.clone(),
        ComParamValue::Complex(entries) => {
            let mut parts: Vec<String> = entries
                .iter()
                .map(|(key, value)| format!("{key}={}", format_com_param(value)))
                .collect();
            parts.sort();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn workspace_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent().expect("workspace root").to_path_buf()
}

fn phase5_template_mdd_license_path() -> PathBuf {
    workspace_root()
        .parent()
        .expect("outer workspace root")
        .join("classic-diagnostic-adapter")
        .join("testcontainer")
        .join("odx")
        .join("FLXC1000.mdd.license")
}

fn phase5_output_dir() -> PathBuf {
    workspace_root().join("deploy").join("pi").join("cda-mdd")
}
