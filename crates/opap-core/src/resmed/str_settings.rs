// Copyright (C) 2011-2018 Mark Watkins
// Copyright (C) 2019-2025 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Ported and modified from OSCAR:
// https://gitlab.com/CrimsonNape/OSCAR-code
// Upstream commit: 64c5e90a26f91fb15868bcfcccde0c1e1522ac86
// Relevant upstream file:
// oscar/SleepLib/loader_plugins/resmed_loader.cpp
// Modified: 2026-07-23

//! Bounded decoding and normalization of ResMed STR therapy settings.
//!
//! The ordered tables retain OSCAR's localized labels for auditability, but
//! `opap-edf` currently enforces EDF's ASCII header boundary. A source that
//! actually encodes a non-ASCII label is therefore rejected during parsing
//! before these aliases can match; this is an explicit remaining fidelity gap.

// This module is intentionally prepared before its session-index integration.
// Keep the allowance local; the integration commit can remove it once
// `resmed.rs` consumes the decoder.
#![allow(dead_code)]

use crate::domain::{Setting, SettingValue, ValueOrigin};
use opap_channels::{ChannelKind, by_stable_key};
use opap_edf::{EdfFile, Limits, ParseError, Parser};
use serde::{Deserialize, Serialize};
use std::{error, fmt};

const SECONDS_PER_DAY: f64 = 86_400.0;
const MAX_SIGNALS: usize = 256;
const MAX_RECORDS: usize = 20_000;
const MAX_SIGNAL_RECORDS: usize = MAX_SIGNALS * MAX_RECORDS;
const MAX_TOTAL_SAMPLES: usize = 16_000_000;
const MAX_WARNINGS: usize = 1_024;

const MODE_ALGORITHM: &str = "resmed-str-mode-normalization-v1";
const AIR11_ENUM_ALGORITHM: &str = "resmed-str-air11-enum-normalization-v1";
const PRESSURE_ALGORITHM: &str = "resmed-str-pressure-derivation-v1";
const FIXED_BOUNDS_ALGORITHM: &str = "resmed-str-fixed-pressure-bounds-v1";
const EPR_ALGORITHM: &str = "resmed-str-epr-reconciliation-v1";

const STR_SETTINGS_LIMITS: Limits = Limits {
    max_signals: MAX_SIGNALS,
    max_records: MAX_RECORDS,
    max_signal_records: MAX_SIGNAL_RECORDS,
    max_total_samples: MAX_TOTAL_SAMPLES,
    max_annotation_bytes: 0,
    max_annotation_records: 0,
    max_annotations: 0,
    max_annotation_text_bytes: 0,
};

/// Largest uncompressed STR source accepted by this decoder.
pub(super) const RESMED_STR_SETTINGS_MAX_FILE_BYTES: usize = 32 * 1024 * 1024;

/// ResMed setting encoding selected from trusted machine identification.
///
/// OSCAR chooses the Air11 branch when the numeric product code is at least
/// 39000. This pure decoder receives the already-resolved generation so it
/// never guesses from an EDF label or source path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StrSettingsGeneration {
    /// Series 9 and Air10 encoding.
    PreAir11,
    /// AirSense 11 and AirCurve 11 encoding.
    Air11,
}

/// Caller-supplied policy for one STR settings decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StrSettingsDecodeOptions {
    /// Trusted machine generation used for manufacturer enum normalization.
    pub generation: StrSettingsGeneration,
}

/// Settings decoded from one STR therapy-day record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct StrDaySettings {
    /// Zero-based EDF data-record index, suitable for joining to STR
    /// boundaries and daily summaries without assigning a session.
    pub record_index: usize,
    /// Source and normalized manufacturer mode codes.
    ///
    /// Air11 uses a different code space. The raw code is retained here rather
    /// than being mislabeled with the legacy `RMS9_Mode` setting semantics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_evidence: Option<StrModeEvidence>,
    /// Canonical settings in stable-key order with no duplicate keys.
    pub settings: Vec<Setting>,
}

/// Manufacturer mode evidence retained alongside normalized settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StrModeEvidence {
    /// Affine-calibrated integral code reported in the STR mode signal.
    pub reported_code: i64,
    /// Legacy ResMed code after the pinned Air11-to-pre-Air11 mapping.
    pub normalized_resmed_code: i64,
}

/// Aggregate counters for intentionally omitted or repaired source values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StrSettingsDiagnostics {
    /// Calibrated negative values treated as OSCAR-compatible missing values.
    pub negative_values_omitted: u32,
    /// Exact known signals omitted because they did not have one sample per
    /// therapy-day record or were not digital.
    pub invalid_signal_shapes: u32,
    /// Exact known signals omitted because affine calibration was invalid.
    pub invalid_calibrations: u32,
    /// Manufacturer enum values omitted because they were not finite integral
    /// values in the supported `i64` range.
    pub invalid_categorical_values: u32,
    /// Records whose reported therapy mode has no supported generic mapping.
    pub unsupported_modes: u32,
    /// Warnings omitted after the fixed global warning budget was exhausted.
    pub warnings_dropped: u32,
}

/// Privacy-safe semantic role attached to a settings warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StrSettingRole {
    Mode,
    SetPressure,
    MinimumPressure,
    MaximumPressure,
    Epap,
    MinimumEpap,
    MaximumEpap,
    Ipap,
    MinimumIpap,
    MaximumIpap,
    PressureSupport,
    MinimumPressureSupport,
    MaximumPressureSupport,
    EpapAuto,
    RampPressure,
    RampTime,
    RampEnabled,
    Epr,
    EprLevel,
    EprType,
    EprEnabled,
    EprClinicalEnabled,
    SmartStart,
    SmartStop,
    AntibacterialFilter,
    ClimateControl,
    MaskType,
    PatientAccess,
    HumidifierEnabled,
    HumidityLevel,
    TemperatureEnabled,
    Temperature,
    ComfortResponse,
    RiseEnabled,
    RiseTime,
    Cycle,
    Trigger,
    TiMax,
    TiMin,
}

/// A bounded warning category; no source paths, serials, or arbitrary EDF
/// strings are retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StrSettingsWarningKind {
    InvalidSignalShape,
    InvalidCalibration,
    InvalidCategoricalValue,
    UnsupportedTherapyMode,
}

/// One privacy-safe warning associated with a therapy-day record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StrSettingsWarning {
    pub record_index: usize,
    pub setting: StrSettingRole,
    pub kind: StrSettingsWarningKind,
}

/// Complete pure result for one uncompressed STR source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct StrSettingsIndex {
    /// Every EDF therapy-day record, including records with no usable settings.
    pub days: Vec<StrDaySettings>,
    pub diagnostics: StrSettingsDiagnostics,
    /// Globally capped, privacy-safe warnings in record/source evaluation
    /// order.
    pub warnings: Vec<StrSettingsWarning>,
}

/// Failure to decode a structurally trustworthy STR settings source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StrSettingsDecodeError {
    FileTooLarge {
        limit: usize,
        actual: usize,
    },
    Parse(ParseError),
    UnsupportedEdfPlus,
    TrailingData {
        bytes: usize,
    },
    InvalidRecordDuration,
    InvalidRecordStart,
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl fmt::Display for StrSettingsDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileTooLarge { limit, actual } => {
                write!(
                    formatter,
                    "STR EDF exceeds the {limit}-byte settings input limit ({actual} bytes)"
                )
            }
            Self::Parse(source) => {
                write!(
                    formatter,
                    "could not parse bounded STR settings EDF: {source}"
                )
            }
            Self::UnsupportedEdfPlus => {
                formatter.write_str("STR settings decoding accepts plain EDF only")
            }
            Self::TrailingData { bytes } => {
                write!(
                    formatter,
                    "STR settings EDF has {bytes} trailing bytes after its declared records"
                )
            }
            Self::InvalidRecordDuration => {
                formatter.write_str("STR settings EDF records must each span exactly one day")
            }
            Self::InvalidRecordStart => {
                formatter.write_str("STR settings EDF must start at device-local noon")
            }
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(
                formatter,
                "could not reserve capacity for {requested} {resource}"
            ),
        }
    }
}

impl error::Error for StrSettingsDecodeError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Parse(source) => Some(source),
            _ => None,
        }
    }
}

impl From<ParseError> for StrSettingsDecodeError {
    fn from(source: ParseError) -> Self {
        Self::Parse(source)
    }
}

/// Decode and normalize settings for each therapy-day record in a complete,
/// uncompressed STR source.
///
/// Lookup is exact, case-sensitive, label-ordered, and deterministic. It never
/// uses the permissive ResMed prefix matcher. Missing signals are normal;
/// malformed known signals are omitted with bounded diagnostics.
///
/// # Errors
///
/// Returns [`StrSettingsDecodeError`] for over-limit, malformed, EDF+,
/// non-daily, non-noon, or trailing-data input.
pub(super) fn decode_str_settings(
    bytes: &[u8],
    options: StrSettingsDecodeOptions,
) -> Result<StrSettingsIndex, StrSettingsDecodeError> {
    if bytes.len() > RESMED_STR_SETTINGS_MAX_FILE_BYTES {
        return Err(StrSettingsDecodeError::FileTooLarge {
            limit: RESMED_STR_SETTINGS_MAX_FILE_BYTES,
            actual: bytes.len(),
        });
    }

    let parser = Parser::new(STR_SETTINGS_LIMITS);
    let header = parser.parse_header(bytes)?;
    if header.is_continuous() || header.is_discontinuous() {
        return Err(StrSettingsDecodeError::UnsupportedEdfPlus);
    }
    if header.record_duration_seconds.to_bits() != SECONDS_PER_DAY.to_bits() {
        return Err(StrSettingsDecodeError::InvalidRecordDuration);
    }
    if header.start.hour != 12 || header.start.minute != 0 || header.start.second != 0 {
        return Err(StrSettingsDecodeError::InvalidRecordStart);
    }

    let parsed = parser.parse(bytes)?;
    if parsed.trailing_data_bytes() != 0 {
        return Err(StrSettingsDecodeError::TrailingData {
            bytes: parsed.trailing_data_bytes(),
        });
    }
    normalize_records(&parsed, options)
}

fn normalize_records(
    parsed: &EdfFile,
    options: StrSettingsDecodeOptions,
) -> Result<StrSettingsIndex, StrSettingsDecodeError> {
    let signal_lookup = SignalLookup::new(parsed)?;
    let mut output = StrSettingsIndex {
        days: Vec::new(),
        diagnostics: StrSettingsDiagnostics::default(),
        warnings: Vec::new(),
    };
    output
        .days
        .try_reserve_exact(parsed.record_count())
        .map_err(|_| StrSettingsDecodeError::AllocationFailed {
            resource: "STR settings days",
            requested: parsed.record_count(),
        })?;

    for record_index in 0..parsed.record_count() {
        let mut decoder = RecordDecoder {
            parsed,
            signal_lookup: &signal_lookup,
            record_index,
            generation: options.generation,
            output: &mut output,
        };
        let (mode_evidence, settings) = decoder.normalize();
        output.days.push(StrDaySettings {
            record_index,
            mode_evidence,
            settings,
        });
    }
    Ok(output)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GenericMode {
    Unknown,
    Cpap,
    Apap,
    BilevelFixed,
    BilevelAutoFixedPressureSupport,
    Asv,
    AsvVariableEpap,
    Avaps,
}

impl GenericMode {
    const fn text(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Cpap => "CPAP",
            Self::Apap => "APAP",
            Self::BilevelFixed => "Bilevel fixed",
            Self::BilevelAutoFixedPressureSupport => "Bilevel auto fixed PS",
            Self::Asv => "ASV",
            Self::AsvVariableEpap => "ASV variable EPAP",
            Self::Avaps => "AVAPS",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ModeInfo {
    reported_code: i64,
    normalized_resmed_code: i64,
    generic: GenericMode,
}

#[derive(Debug, Clone, Copy)]
enum SettingOrigin {
    DeviceReported,
    Derived(&'static str),
}

impl SettingOrigin {
    fn domain(self) -> ValueOrigin {
        match self {
            Self::DeviceReported => ValueOrigin::DeviceReported,
            Self::Derived(algorithm) => ValueOrigin::Derived {
                algorithm: algorithm.to_owned(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Number {
    value: f64,
    origin: SettingOrigin,
}

impl Number {
    const fn reported(value: f64) -> Self {
        Self {
            value,
            origin: SettingOrigin::DeviceReported,
        }
    }

    const fn derived(value: f64, algorithm: &'static str) -> Self {
        Self {
            value,
            origin: SettingOrigin::Derived(algorithm),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Code {
    value: i64,
    origin: SettingOrigin,
}

impl Code {
    const fn reported(value: i64) -> Self {
        Self {
            value,
            origin: SettingOrigin::DeviceReported,
        }
    }

    const fn derived(value: i64, algorithm: &'static str) -> Self {
        Self {
            value,
            origin: SettingOrigin::Derived(algorithm),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Reading<T> {
    present: bool,
    value: Option<T>,
}

impl<T> Default for Reading<T> {
    fn default() -> Self {
        Self {
            present: false,
            value: None,
        }
    }
}

#[derive(Debug, Default)]
struct WorkingSettings {
    mode: Option<ModeInfo>,
    set_pressure: Option<Number>,
    minimum_pressure: Option<Number>,
    maximum_pressure: Option<Number>,
    epap: Option<Number>,
    minimum_epap: Option<Number>,
    maximum_epap: Option<Number>,
    ipap: Option<Number>,
    minimum_ipap: Option<Number>,
    maximum_ipap: Option<Number>,
    pressure_support: Option<Number>,
    minimum_pressure_support: Option<Number>,
    maximum_pressure_support: Option<Number>,
    ramp_pressure: Option<Number>,
    ramp_time: Option<Number>,
    ramp_enabled: Option<Code>,
    epr: Option<Code>,
    epr_level: Option<Number>,
    smart_start: Option<Code>,
    smart_stop: Option<Code>,
    antibacterial_filter: Option<Code>,
    climate_control: Option<Code>,
    mask_type: Option<Code>,
    patient_access: Option<Code>,
    patient_view: Option<Code>,
    humidifier_enabled: Option<Code>,
    humidity_level: Option<Code>,
    temperature_enabled: Option<Code>,
    temperature: Option<Number>,
    comfort_response: Option<Code>,
    rise_enabled: Option<Code>,
    rise_time: Option<Number>,
    cycle: Option<Code>,
    trigger: Option<Code>,
    ti_max: Option<Number>,
    ti_min: Option<Number>,
}

struct RecordDecoder<'a, 'b, 'c> {
    parsed: &'a EdfFile,
    signal_lookup: &'b SignalLookup,
    record_index: usize,
    generation: StrSettingsGeneration,
    output: &'c mut StrSettingsIndex,
}

impl RecordDecoder<'_, '_, '_> {
    fn normalize(&mut self) -> (Option<StrModeEvidence>, Vec<Setting>) {
        let mut working = WorkingSettings {
            mode: self.decode_mode(),
            ..WorkingSettings::default()
        };
        self.decode_pressure_settings(&mut working);
        self.decode_epr(&mut working);
        self.decode_ramp(&mut working);
        self.decode_environment_and_access(&mut working);
        self.decode_bilevel_controls(&mut working);
        let mode_evidence = working.mode.map(|mode| StrModeEvidence {
            reported_code: mode.reported_code,
            normalized_resmed_code: mode.normalized_resmed_code,
        });
        (mode_evidence, self.store_settings(working))
    }

    fn decode_mode(&mut self) -> Option<ModeInfo> {
        let reported = self.read_code(StrSettingRole::Mode, MODE);
        let code = reported.value?.value;
        let normalized_resmed_code = match self.generation {
            StrSettingsGeneration::PreAir11 => code,
            StrSettingsGeneration::Air11 => match code {
                1 => 1,
                2 => 11,
                3 => 0,
                4 => 3,
                6 => 7,
                7 => 8,
                8 => 6,
                0 | 5 => 16,
                _ => 16,
            },
        };
        let generic = match normalized_resmed_code {
            0 => GenericMode::Cpap,
            1 | 11 => GenericMode::Apap,
            2..=5 => GenericMode::BilevelFixed,
            6 => GenericMode::BilevelAutoFixedPressureSupport,
            7 => GenericMode::Asv,
            8 => GenericMode::AsvVariableEpap,
            9 => GenericMode::Avaps,
            _ => GenericMode::Unknown,
        };
        if generic == GenericMode::Unknown {
            self.output.diagnostics.unsupported_modes =
                self.output.diagnostics.unsupported_modes.saturating_add(1);
            self.warn(
                StrSettingRole::Mode,
                StrSettingsWarningKind::UnsupportedTherapyMode,
            );
        }
        Some(ModeInfo {
            reported_code: code,
            normalized_resmed_code,
            generic,
        })
    }

    fn decode_pressure_settings(&mut self, working: &mut WorkingSettings) {
        let Some(mode) = working.mode else {
            return;
        };

        // OSCAR evaluates mode-specific ramp candidates before the pressure
        // branches below. Later, more-specific labels deliberately overwrite
        // these values when present.
        match mode.normalized_resmed_code {
            0 => Self::overwrite_number(
                &mut working.ramp_pressure,
                self.read_number(StrSettingRole::RampPressure, CPAP_START_PRESSURE),
            ),
            1 => Self::overwrite_number(
                &mut working.ramp_pressure,
                self.read_number(StrSettingRole::RampPressure, APAP_START_PRESSURE),
            ),
            11 => Self::overwrite_number(
                &mut working.ramp_pressure,
                self.read_number(StrSettingRole::RampPressure, APAP_FOR_HER_START_PRESSURE),
            ),
            _ => {}
        }
        if mode.generic == GenericMode::BilevelFixed {
            Self::overwrite_number(
                &mut working.ramp_pressure,
                self.read_number(StrSettingRole::RampPressure, BILEVEL_START_PRESSURE),
            );
        }
        if matches!(
            mode.generic,
            GenericMode::Asv
                | GenericMode::AsvVariableEpap
                | GenericMode::BilevelAutoFixedPressureSupport
        ) {
            Self::overwrite_number(
                &mut working.ramp_pressure,
                self.read_number(StrSettingRole::RampPressure, VAUTO_START_PRESSURE),
            );
        }

        Self::overwrite_number(
            &mut working.ipap,
            self.read_number(StrSettingRole::Ipap, IPAP),
        );
        Self::overwrite_number(
            &mut working.epap,
            self.read_number(StrSettingRole::Epap, EPAP),
        );

        match mode.generic {
            GenericMode::Avaps => self.decode_avaps_pressures(working),
            GenericMode::Asv => self.decode_asv_pressures(working),
            GenericMode::AsvVariableEpap => self.decode_asv_auto_pressures(working),
            _ => {}
        }

        // Auto-for-Her uses its dedicated labels if the label exists at all.
        // A present but invalid/negative source intentionally prevents fallback
        // to a different mode's generic pressure label.
        if mode.normalized_resmed_code == 11 {
            let maximum =
                self.read_number(StrSettingRole::MaximumPressure, APAP_FOR_HER_MAX_PRESSURE);
            if maximum.present {
                Self::overwrite_number(&mut working.maximum_pressure, maximum);
            } else {
                Self::overwrite_number(
                    &mut working.maximum_pressure,
                    self.read_number(StrSettingRole::MaximumPressure, MAX_PRESSURE),
                );
            }
            let minimum =
                self.read_number(StrSettingRole::MinimumPressure, APAP_FOR_HER_MIN_PRESSURE);
            if minimum.present {
                Self::overwrite_number(&mut working.minimum_pressure, minimum);
            } else {
                Self::overwrite_number(
                    &mut working.minimum_pressure,
                    self.read_number(StrSettingRole::MinimumPressure, MIN_PRESSURE),
                );
            }
        } else {
            Self::overwrite_number(
                &mut working.maximum_pressure,
                self.read_number(StrSettingRole::MaximumPressure, MAX_PRESSURE),
            );
            Self::overwrite_number(
                &mut working.minimum_pressure,
                self.read_number(StrSettingRole::MinimumPressure, MIN_PRESSURE),
            );
        }

        Self::overwrite_number(
            &mut working.set_pressure,
            self.read_number(StrSettingRole::SetPressure, SET_PRESSURE),
        );
        Self::overwrite_number(
            &mut working.maximum_epap,
            self.read_number(StrSettingRole::MaximumEpap, MAX_EPAP),
        );
        Self::overwrite_number(
            &mut working.minimum_epap,
            self.read_number(StrSettingRole::MinimumEpap, MIN_EPAP),
        );
        Self::overwrite_number(
            &mut working.maximum_ipap,
            self.read_number(StrSettingRole::MaximumIpap, MAX_IPAP),
        );
        Self::overwrite_number(
            &mut working.minimum_ipap,
            self.read_number(StrSettingRole::MinimumIpap, MIN_IPAP),
        );
        Self::overwrite_number(
            &mut working.pressure_support,
            self.read_number(StrSettingRole::PressureSupport, PRESSURE_SUPPORT),
        );

        // One known ResMed STR layout contains two signals with each exact
        // label. OSCAR selects occurrence 1 only for variable-EPAP ASV and
        // occurrence 0 for every other mode.
        let duplicate_occurrence = usize::from(mode.generic == GenericMode::AsvVariableEpap);
        Self::overwrite_number(
            &mut working.maximum_pressure_support,
            self.read_number(
                StrSettingRole::MaximumPressureSupport,
                Selector::new(&["Max PS"], duplicate_occurrence),
            ),
        );
        Self::overwrite_number(
            &mut working.minimum_pressure_support,
            self.read_number(
                StrSettingRole::MinimumPressureSupport,
                Selector::new(&["Min PS"], duplicate_occurrence),
            ),
        );

        match mode.generic {
            GenericMode::AsvVariableEpap => {
                working.minimum_ipap =
                    self.derive_sum(working.minimum_epap, working.minimum_pressure_support);
                working.maximum_ipap =
                    self.derive_sum(working.maximum_epap, working.maximum_pressure_support);
            }
            GenericMode::Asv => {
                working.minimum_ipap =
                    self.derive_sum(working.epap, working.minimum_pressure_support);
                working.maximum_ipap =
                    self.derive_sum(working.epap, working.maximum_pressure_support);
            }
            _ => {}
        }
    }

    fn decode_avaps_pressures(&mut self, working: &mut WorkingSettings) {
        Self::overwrite_number(
            &mut working.ramp_pressure,
            self.read_number(StrSettingRole::RampPressure, IVAPS_START_PRESSURE),
        );
        let fixed_epap = self.read_number(StrSettingRole::Epap, IVAPS_EPAP);
        if fixed_epap.present {
            working.epap = fixed_epap.value;
            working.minimum_epap = fixed_epap.value;
            working.maximum_epap = fixed_epap.value;
        }
        let epap_auto = self.read_number(StrSettingRole::EpapAuto, IVAPS_EPAP_AUTO);
        Self::overwrite_number(
            &mut working.minimum_pressure_support,
            self.read_number(StrSettingRole::MinimumPressureSupport, IVAPS_MIN_PS),
        );
        Self::overwrite_number(
            &mut working.minimum_epap,
            self.read_number(StrSettingRole::MinimumEpap, IVAPS_MIN_EPAP),
        );
        Self::overwrite_number(
            &mut working.maximum_epap,
            self.read_number(StrSettingRole::MaximumEpap, IVAPS_MAX_EPAP),
        );
        Self::overwrite_number(
            &mut working.maximum_pressure_support,
            self.read_number(StrSettingRole::MaximumPressureSupport, IVAPS_MAX_PS),
        );

        let fixed = epap_auto_is_fixed(epap_auto.value, working.epap);
        let minimum_base = if fixed {
            working.epap
        } else {
            working.minimum_epap
        };
        let maximum_base = if fixed {
            working.epap
        } else {
            working.maximum_epap
        };
        working.minimum_ipap = self.derive_sum(minimum_base, working.minimum_pressure_support);
        working.maximum_ipap = self.derive_sum(maximum_base, working.maximum_pressure_support);
    }

    fn decode_asv_pressures(&mut self, working: &mut WorkingSettings) {
        Self::overwrite_number(
            &mut working.ramp_pressure,
            self.read_number(StrSettingRole::RampPressure, ASV_START_PRESSURE),
        );
        let epap = self.read_number(StrSettingRole::Epap, ASV_EPAP);
        if epap.present {
            working.epap = epap.value;
            working.minimum_epap = epap.value;
            working.maximum_epap = epap.value;
        }
        Self::overwrite_number(
            &mut working.minimum_pressure_support,
            self.read_number(StrSettingRole::MinimumPressureSupport, ASV_MIN_PS),
        );
        Self::overwrite_number(
            &mut working.maximum_pressure_support,
            self.read_number(StrSettingRole::MaximumPressureSupport, ASV_MAX_PS),
        );
        working.minimum_ipap = self.derive_sum(working.epap, working.minimum_pressure_support);
        working.maximum_ipap = self.derive_sum(working.epap, working.maximum_pressure_support);
    }

    fn decode_asv_auto_pressures(&mut self, working: &mut WorkingSettings) {
        Self::overwrite_number(
            &mut working.ramp_pressure,
            self.read_number(StrSettingRole::RampPressure, ASV_AUTO_START_PRESSURE),
        );
        Self::overwrite_number(
            &mut working.minimum_epap,
            self.read_number(StrSettingRole::MinimumEpap, ASV_AUTO_MIN_EPAP),
        );
        Self::overwrite_number(
            &mut working.maximum_epap,
            self.read_number(StrSettingRole::MaximumEpap, ASV_AUTO_MAX_EPAP),
        );
        Self::overwrite_number(
            &mut working.minimum_pressure_support,
            self.read_number(StrSettingRole::MinimumPressureSupport, ASV_AUTO_MIN_PS),
        );
        Self::overwrite_number(
            &mut working.maximum_pressure_support,
            self.read_number(StrSettingRole::MaximumPressureSupport, ASV_AUTO_MAX_PS),
        );
        working.minimum_ipap =
            self.derive_sum(working.minimum_epap, working.minimum_pressure_support);
        working.maximum_ipap =
            self.derive_sum(working.maximum_epap, working.maximum_pressure_support);
    }

    fn decode_epr(&mut self, working: &mut WorkingSettings) {
        let Some(mode) = working.mode else {
            return;
        };
        if !matches!(mode.generic, GenericMode::Cpap | GenericMode::Apap) {
            return;
        }

        let mut epr = self.read_air11_code(StrSettingRole::Epr, EPR).value;
        let mut epr_level = self.read_number(StrSettingRole::EprLevel, EPR_LEVEL).value;

        let epr_type = self.read_code(StrSettingRole::EprType, EPR_TYPE);
        if let Some(value) = epr_type.value {
            let adjustment = match self.generation {
                StrSettingsGeneration::PreAir11 => 1,
                StrSettingsGeneration::Air11 => 0,
            };
            epr = value
                .value
                .checked_add(adjustment)
                .map(|code| Code::derived(code, EPR_ALGORITHM));
        }

        let epr_enabled = self.read_air11_code(StrSettingRole::EprEnabled, EPR_ENABLED);
        let clinical_enabled = if epr_enabled.value.is_some_and(|value| value.value != 0) {
            self.read_air11_code(StrSettingRole::EprClinicalEnabled, EPR_CLINICAL_ENABLED)
        } else {
            Reading::default()
        };
        let has_air1x_controls = epr_type.present || epr_enabled.present;
        let controls_enable_epr = epr_enabled.value.is_some_and(|value| value.value != 0)
            && clinical_enabled.value.is_some_and(|value| value.value != 0);
        if has_air1x_controls && !controls_enable_epr {
            epr = Some(Code::derived(0, EPR_ALGORITHM));
            epr_level = Some(Number::derived(0.0, EPR_ALGORITHM));
        }

        match (epr, epr_level) {
            (Some(mode), Some(level)) => {
                working.epr = Some(mode);
                working.epr_level = Some(level);
            }
            (Some(mode), None) => {
                working.epr = Some(Code::derived(i64::from(mode.value > 0), EPR_ALGORITHM));
                working.epr_level = Some(Number::derived(mode.value as f64, EPR_ALGORITHM));
            }
            (None, Some(level)) => {
                working.epr = Some(Code::derived(i64::from(level.value > 0.0), EPR_ALGORITHM));
                working.epr_level = Some(level);
            }
            (None, None) => {}
        }
    }

    fn decode_ramp(&mut self, working: &mut WorkingSettings) {
        Self::overwrite_number(
            &mut working.ramp_time,
            self.read_number(StrSettingRole::RampTime, RAMP_TIME),
        );
        let enabled = self.read_air11_code(StrSettingRole::RampEnabled, RAMP_ENABLED);
        if enabled.present {
            working.ramp_enabled = enabled.value;
            if enabled.value.is_some_and(|value| value.value == 2) {
                // ResMed code 2 is Auto ramp. OSCAR intentionally suppresses
                // the numeric fixed ramp time in this state.
                working.ramp_time = None;
            }
        }
    }

    fn decode_environment_and_access(&mut self, working: &mut WorkingSettings) {
        working.comfort_response = self
            .read_air11_code(StrSettingRole::ComfortResponse, COMFORT_RESPONSE)
            .value;
        working.antibacterial_filter = self
            .read_air11_code(StrSettingRole::AntibacterialFilter, ANTIBACTERIAL_FILTER)
            .value;
        working.climate_control = self
            .read_air11_code(StrSettingRole::ClimateControl, CLIMATE_CONTROL)
            .value;

        let mask = self.read_code(StrSettingRole::MaskType, MASK_TYPE);
        working.mask_type = match (self.generation, mask.value) {
            (StrSettingsGeneration::Air11, Some(value)) => {
                let normalized = if (2..=4).contains(&value.value) {
                    value.value - 2
                } else {
                    4
                };
                Some(Code {
                    value: normalized,
                    origin: SettingOrigin::Derived(AIR11_ENUM_ALGORITHM),
                })
            }
            (_, value) => value,
        };

        let access = self.read_code(StrSettingRole::PatientAccess, PATIENT_ACCESS);
        match self.generation {
            StrSettingsGeneration::PreAir11 => working.patient_access = access.value,
            StrSettingsGeneration::Air11 => {
                working.patient_view = self.adjust_air11(access).value;
            }
        }

        working.smart_start = self
            .read_air11_code(StrSettingRole::SmartStart, SMART_START)
            .value;
        working.smart_stop = self
            .read_air11_code(StrSettingRole::SmartStop, SMART_STOP)
            .value;
        working.humidifier_enabled = self
            .read_air11_code(StrSettingRole::HumidifierEnabled, HUMIDIFIER_ENABLED)
            .value;
        working.humidity_level = self
            .read_code(StrSettingRole::HumidityLevel, HUMIDITY_LEVEL)
            .value;
        working.temperature_enabled = self
            .read_air11_code(StrSettingRole::TemperatureEnabled, TEMPERATURE_ENABLED)
            .value;
        working.temperature = self
            .read_number(StrSettingRole::Temperature, TEMPERATURE)
            .value;

        // `S.Tube` is deliberately not read or persisted: OPAP has no
        // evidence-backed stable channel for it.
    }

    fn decode_bilevel_controls(&mut self, working: &mut WorkingSettings) {
        let Some(mode) = working.mode else {
            return;
        };
        if (2..=5).contains(&mode.normalized_resmed_code) {
            let (rise_enabled, rise_time, cycle, trigger, ti_max, ti_min) = match self.generation {
                StrSettingsGeneration::PreAir11 => {
                    (RISE_ENABLED, RISE_TIME, CYCLE, TRIGGER, TI_MAX, TI_MIN)
                }
                StrSettingsGeneration::Air11 => (
                    AIR11_S_RISE_ENABLED,
                    AIR11_S_RISE_TIME,
                    AIR11_S_CYCLE,
                    AIR11_S_TRIGGER,
                    AIR11_S_TI_MAX,
                    AIR11_S_TI_MIN,
                ),
            };
            working.rise_enabled = self
                .read_air11_code(StrSettingRole::RiseEnabled, rise_enabled)
                .value;
            working.rise_time = self.read_number(StrSettingRole::RiseTime, rise_time).value;
            if matches!(mode.normalized_resmed_code, 3 | 4) {
                working.cycle = self.read_air11_code(StrSettingRole::Cycle, cycle).value;
                working.trigger = self.read_air11_code(StrSettingRole::Trigger, trigger).value;
                working.ti_max = self.read_number(StrSettingRole::TiMax, ti_max).value;
                working.ti_min = self.read_number(StrSettingRole::TiMin, ti_min).value;
            }
        } else if mode.normalized_resmed_code == 6 {
            let (cycle, trigger, ti_max, ti_min) = match self.generation {
                StrSettingsGeneration::PreAir11 => (CYCLE, TRIGGER, TI_MAX, TI_MIN),
                StrSettingsGeneration::Air11 => (
                    AIR11_VA_CYCLE,
                    AIR11_VA_TRIGGER,
                    AIR11_VA_TI_MAX,
                    AIR11_VA_TI_MIN,
                ),
            };
            working.cycle = self.read_air11_code(StrSettingRole::Cycle, cycle).value;
            working.trigger = self.read_air11_code(StrSettingRole::Trigger, trigger).value;
            working.ti_max = self.read_number(StrSettingRole::TiMax, ti_max).value;
            working.ti_min = self.read_number(StrSettingRole::TiMin, ti_min).value;
        }

        // OSCAR decodes EasyBreathe for mode 3. OPAP intentionally does not
        // invent a stable channel ID, so that source is not persisted.
    }

    fn store_settings(&mut self, working: WorkingSettings) -> Vec<Setting> {
        let mut settings = Vec::new();
        if let Some(mode) = working.mode {
            let manufacturer_mode_origin = match self.generation {
                StrSettingsGeneration::PreAir11 => SettingOrigin::DeviceReported,
                StrSettingsGeneration::Air11 => SettingOrigin::Derived(MODE_ALGORITHM),
            };
            self.push_integer(
                &mut settings,
                "pap.setting.resmed.therapy_mode",
                Code {
                    value: mode.normalized_resmed_code,
                    origin: manufacturer_mode_origin,
                },
            );
            self.push_text(
                &mut settings,
                "pap.setting.pap_mode",
                mode.generic.text(),
                SettingOrigin::Derived(MODE_ALGORITHM),
            );

            match mode.generic {
                GenericMode::Cpap => {
                    self.push_number(
                        &mut settings,
                        "pap.setting.resmed.set_pressure",
                        working.set_pressure,
                    );
                }
                GenericMode::Apap => {
                    self.push_number(
                        &mut settings,
                        "pap.setting.minimum_pressure",
                        working.minimum_pressure,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.maximum_pressure",
                        working.maximum_pressure,
                    );
                }
                GenericMode::BilevelFixed => {
                    self.push_fixed_bounds(
                        &mut settings,
                        "pap.setting.epap_minimum",
                        "pap.setting.epap_maximum",
                        working.epap,
                    );
                    self.push_fixed_bounds(
                        &mut settings,
                        "pap.setting.ipap_minimum",
                        "pap.setting.ipap_maximum",
                        working.ipap,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.pressure_support",
                        working.pressure_support,
                    );
                    self.push_bilevel_controls(&mut settings, &working, true);
                }
                GenericMode::BilevelAutoFixedPressureSupport => {
                    self.push_number(
                        &mut settings,
                        "pap.setting.epap_minimum",
                        working.minimum_epap,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.ipap_maximum",
                        working.maximum_ipap,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.pressure_support",
                        working.pressure_support,
                    );
                    self.push_bilevel_controls(&mut settings, &working, false);
                }
                GenericMode::Asv => {
                    self.push_fixed_bounds(
                        &mut settings,
                        "pap.setting.epap_minimum",
                        "pap.setting.epap_maximum",
                        working.epap,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.pressure_support_minimum",
                        working.minimum_pressure_support,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.pressure_support_maximum",
                        working.maximum_pressure_support,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.ipap_maximum",
                        working.maximum_ipap,
                    );
                }
                GenericMode::AsvVariableEpap => {
                    self.push_number(
                        &mut settings,
                        "pap.setting.epap_maximum",
                        working.maximum_epap,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.epap_minimum",
                        working.minimum_epap,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.ipap_maximum",
                        working.maximum_ipap,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.ipap_minimum",
                        working.minimum_ipap,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.pressure_support_minimum",
                        working.minimum_pressure_support,
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.pressure_support_maximum",
                        working.maximum_pressure_support,
                    );
                }
                GenericMode::Unknown | GenericMode::Avaps => {
                    // This is OSCAR's generic positive-only fallback. It
                    // preserves useful settings for iVAPS and unknown future
                    // modes without presenting a zero sentinel as a pressure.
                    self.push_fixed_bounds(
                        &mut settings,
                        "pap.setting.epap_minimum",
                        "pap.setting.epap_maximum",
                        positive(working.epap),
                    );
                    self.push_fixed_bounds(
                        &mut settings,
                        "pap.setting.ipap_minimum",
                        "pap.setting.ipap_maximum",
                        positive(working.ipap),
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.resmed.set_pressure",
                        positive(working.set_pressure),
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.minimum_pressure",
                        positive(working.minimum_pressure),
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.maximum_pressure",
                        positive(working.maximum_pressure),
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.epap_maximum",
                        positive(working.maximum_epap),
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.epap_minimum",
                        positive(working.minimum_epap),
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.ipap_maximum",
                        positive(working.maximum_ipap),
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.ipap_minimum",
                        positive(working.minimum_ipap),
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.pressure_support_minimum",
                        positive(working.minimum_pressure_support),
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.pressure_support_maximum",
                        positive(working.maximum_pressure_support),
                    );
                    self.push_number(
                        &mut settings,
                        "pap.setting.pressure_support",
                        positive(working.pressure_support),
                    );
                }
            }
        }

        if let Some(epr) = working.epr {
            self.push_integer(&mut settings, "pap.setting.resmed.epr", epr);
            if epr.value > 0 {
                self.push_number(
                    &mut settings,
                    "pap.setting.resmed.epr_level",
                    working.epr_level,
                );
            }
        }
        if let Some(ramp_enabled) = working.ramp_enabled {
            self.push_integer(
                &mut settings,
                "pap.setting.resmed.ramp_enabled",
                ramp_enabled,
            );
            if ramp_enabled.value >= 1 {
                self.push_number(&mut settings, "pap.setting.ramp_time", working.ramp_time);
                self.push_number(
                    &mut settings,
                    "pap.setting.ramp_pressure",
                    working.ramp_pressure,
                );
            }
        }

        self.push_boolean(
            &mut settings,
            "pap.setting.resmed.smart_start",
            working.smart_start,
        );
        self.push_boolean(
            &mut settings,
            "pap.setting.resmed.smart_stop",
            working.smart_stop,
        );
        self.push_boolean(
            &mut settings,
            "pap.setting.resmed.antibacterial_filter",
            working.antibacterial_filter,
        );
        self.push_integer_option(
            &mut settings,
            "pap.setting.resmed.climate_control",
            working.climate_control,
        );
        self.push_integer_option(
            &mut settings,
            "pap.setting.resmed.mask_type",
            working.mask_type,
        );
        self.push_integer_option(
            &mut settings,
            "pap.setting.resmed.patient_access",
            working.patient_access,
        );
        self.push_integer_option(
            &mut settings,
            "pap.setting.resmed.patient_view",
            working.patient_view,
        );
        self.push_boolean(
            &mut settings,
            "pap.setting.resmed.humidifier_enabled",
            working.humidifier_enabled,
        );
        if working
            .humidifier_enabled
            .is_some_and(|value| value.value == 1)
        {
            self.push_integer_option(
                &mut settings,
                "pap.setting.resmed.humidity_level",
                working.humidity_level,
            );
        }
        self.push_integer_option(
            &mut settings,
            "pap.setting.resmed.temperature_enabled",
            working.temperature_enabled,
        );
        if working
            .temperature_enabled
            .is_some_and(|value| value.value >= 1)
        {
            self.push_number(
                &mut settings,
                "pap.setting.resmed.temperature",
                working.temperature,
            );
        }
        self.push_integer_option(
            &mut settings,
            "pap.setting.resmed.comfort_response",
            working.comfort_response,
        );

        settings.sort_unstable_by(|left, right| left.key.cmp(&right.key));
        debug_assert!(settings.windows(2).all(|pair| pair[0].key != pair[1].key));
        settings
    }

    fn push_bilevel_controls(
        &mut self,
        settings: &mut Vec<Setting>,
        working: &WorkingSettings,
        include_rise: bool,
    ) {
        self.push_integer_option(settings, "pap.setting.resmed.cycle", working.cycle);
        self.push_integer_option(settings, "pap.setting.resmed.trigger", working.trigger);
        self.push_number(settings, "pap.setting.resmed.timax", working.ti_max);
        self.push_number(settings, "pap.setting.resmed.timin", working.ti_min);
        if include_rise {
            self.push_boolean(
                settings,
                "pap.setting.resmed.rise_enabled",
                working.rise_enabled,
            );
            self.push_number(settings, "pap.setting.resmed.rise_time", working.rise_time);
        }
    }

    fn push_fixed_bounds(
        &mut self,
        settings: &mut Vec<Setting>,
        minimum_key: &'static str,
        maximum_key: &'static str,
        value: Option<Number>,
    ) {
        let Some(value) = value else {
            return;
        };
        let derived = Number::derived(value.value, FIXED_BOUNDS_ALGORITHM);
        self.push_number(settings, minimum_key, Some(derived));
        self.push_number(settings, maximum_key, Some(derived));
    }

    fn push_number(
        &mut self,
        settings: &mut Vec<Setting>,
        key: &'static str,
        value: Option<Number>,
    ) {
        let Some(value) = value.filter(|value| value.value.is_finite()) else {
            return;
        };
        upsert_setting(
            settings,
            make_setting(
                key,
                SettingValue::Decimal(value.value),
                value.origin.domain(),
            ),
        );
    }

    fn push_integer(&mut self, settings: &mut Vec<Setting>, key: &'static str, value: Code) {
        upsert_setting(
            settings,
            make_setting(
                key,
                SettingValue::Integer(value.value),
                value.origin.domain(),
            ),
        );
    }

    fn push_integer_option(
        &mut self,
        settings: &mut Vec<Setting>,
        key: &'static str,
        value: Option<Code>,
    ) {
        if let Some(value) = value {
            self.push_integer(settings, key, value);
        }
    }

    fn push_boolean(
        &mut self,
        settings: &mut Vec<Setting>,
        key: &'static str,
        value: Option<Code>,
    ) {
        let Some(value) = value else {
            return;
        };
        if let Ok(boolean) = bool::try_from(value.value) {
            upsert_setting(
                settings,
                make_setting(key, SettingValue::Boolean(boolean), value.origin.domain()),
            );
        } else {
            self.output.diagnostics.invalid_categorical_values = self
                .output
                .diagnostics
                .invalid_categorical_values
                .saturating_add(1);
        }
    }

    fn push_text(
        &mut self,
        settings: &mut Vec<Setting>,
        key: &'static str,
        value: &'static str,
        origin: SettingOrigin,
    ) {
        upsert_setting(
            settings,
            make_setting(key, SettingValue::Text(value.to_owned()), origin.domain()),
        );
    }

    fn overwrite_number(target: &mut Option<Number>, reading: Reading<Number>) {
        if reading.present {
            *target = reading.value;
        }
    }

    fn derive_sum(&mut self, left: Option<Number>, right: Option<Number>) -> Option<Number> {
        let value = left?.value + right?.value;
        value
            .is_finite()
            .then_some(Number::derived(value, PRESSURE_ALGORITHM))
    }

    fn read_air11_code(&mut self, role: StrSettingRole, selector: Selector) -> Reading<Code> {
        let reading = self.read_code(role, selector);
        self.adjust_air11(reading)
    }

    fn adjust_air11(&mut self, mut reading: Reading<Code>) -> Reading<Code> {
        if self.generation != StrSettingsGeneration::Air11 {
            return reading;
        }
        if let Some(value) = reading.value {
            match value.value.checked_sub(1) {
                Some(normalized) if normalized >= 0 => {
                    reading.value = Some(Code {
                        value: normalized,
                        origin: SettingOrigin::Derived(AIR11_ENUM_ALGORITHM),
                    });
                }
                _ => {
                    reading.value = None;
                    self.output.diagnostics.negative_values_omitted = self
                        .output
                        .diagnostics
                        .negative_values_omitted
                        .saturating_add(1);
                }
            }
        }
        reading
    }

    fn read_code(&mut self, role: StrSettingRole, selector: Selector) -> Reading<Code> {
        let reading = self.read_number(role, selector);
        let Some(number) = reading.value else {
            return Reading {
                present: reading.present,
                value: None,
            };
        };
        let rounded = number.value.round();
        if (number.value - rounded).abs() > 1.0e-9
            || rounded < i64::MIN as f64
            || rounded > i64::MAX as f64
        {
            self.output.diagnostics.invalid_categorical_values = self
                .output
                .diagnostics
                .invalid_categorical_values
                .saturating_add(1);
            self.warn(role, StrSettingsWarningKind::InvalidCategoricalValue);
            return Reading {
                present: true,
                value: None,
            };
        }
        Reading {
            present: true,
            value: Some(Code::reported(rounded as i64)),
        }
    }

    fn read_number(&mut self, role: StrSettingRole, selector: Selector) -> Reading<Number> {
        let Some(signal_index) = self.signal_lookup.select(self.parsed, selector) else {
            return Reading::default();
        };
        let signal = self
            .parsed
            .signal(signal_index)
            .expect("selected signal index is in range");
        if signal.header.samples_per_record != 1 {
            self.output.diagnostics.invalid_signal_shapes = self
                .output
                .diagnostics
                .invalid_signal_shapes
                .saturating_add(1);
            self.warn(role, StrSettingsWarningKind::InvalidSignalShape);
            return Reading {
                present: true,
                value: None,
            };
        }
        let Some(raw) = self
            .parsed
            .record(self.record_index)
            .and_then(|record| record.digital_samples(signal_index))
            .and_then(|samples| samples.first())
            .copied()
        else {
            self.output.diagnostics.invalid_signal_shapes = self
                .output
                .diagnostics
                .invalid_signal_shapes
                .saturating_add(1);
            self.warn(role, StrSettingsWarningKind::InvalidSignalShape);
            return Reading {
                present: true,
                value: None,
            };
        };
        let value = match signal.header.physical_value(raw) {
            Ok(value) => value,
            Err(_) => {
                self.output.diagnostics.invalid_calibrations = self
                    .output
                    .diagnostics
                    .invalid_calibrations
                    .saturating_add(1);
                self.warn(role, StrSettingsWarningKind::InvalidCalibration);
                return Reading {
                    present: true,
                    value: None,
                };
            }
        };
        if value < 0.0 {
            self.output.diagnostics.negative_values_omitted = self
                .output
                .diagnostics
                .negative_values_omitted
                .saturating_add(1);
            return Reading {
                present: true,
                value: None,
            };
        }
        Reading {
            present: true,
            value: Some(Number::reported(value)),
        }
    }

    fn warn(&mut self, setting: StrSettingRole, kind: StrSettingsWarningKind) {
        if self.output.warnings.len() < MAX_WARNINGS {
            self.output.warnings.push(StrSettingsWarning {
                record_index: self.record_index,
                setting,
                kind,
            });
        } else {
            self.output.diagnostics.warnings_dropped =
                self.output.diagnostics.warnings_dropped.saturating_add(1);
        }
    }
}

fn positive(value: Option<Number>) -> Option<Number> {
    value.filter(|value| value.value > 0.0)
}

fn epap_auto_is_fixed(epap_auto: Option<Number>, epap: Option<Number>) -> bool {
    epap_auto.is_some_and(|value| value.value == 0.0) && epap.is_some()
}

fn make_setting(key: &'static str, value: SettingValue, origin: ValueOrigin) -> Setting {
    let definition = by_stable_key(key).expect("STR setting key must exist in channel registry");
    assert_eq!(
        definition.kind,
        ChannelKind::Setting,
        "STR output must use a setting channel"
    );
    let symbol = definition.unit.symbol();
    Setting {
        key: definition.key.as_str().to_owned(),
        label: definition.label.to_owned(),
        unit: (!symbol.is_empty()).then(|| symbol.to_owned()),
        value,
        origin,
    }
}

fn upsert_setting(settings: &mut Vec<Setting>, setting: Setting) {
    if let Some(existing) = settings
        .iter_mut()
        .find(|existing| existing.key == setting.key)
    {
        *existing = setting;
    } else {
        settings.push(setting);
    }
}

#[derive(Debug, Clone, Copy)]
struct Selector {
    labels: &'static [&'static str],
    occurrence: usize,
}

impl Selector {
    const fn first(labels: &'static [&'static str]) -> Self {
        Self {
            labels,
            occurrence: 0,
        }
    }

    const fn new(labels: &'static [&'static str], occurrence: usize) -> Self {
        Self { labels, occurrence }
    }
}

#[derive(Debug, Clone, Copy)]
struct SignalOccurrences {
    first: usize,
    second: Option<usize>,
}

/// Per-file exact-label index. OSCAR's selectors only request the first or
/// second occurrence of a label, so retaining two indices avoids rescanning
/// every EDF signal for every setting in every therapy-day record.
struct SignalLookup {
    labels: Vec<SignalOccurrences>,
}

impl SignalLookup {
    fn new(parsed: &EdfFile) -> Result<Self, StrSettingsDecodeError> {
        let mut labels: Vec<SignalOccurrences> = Vec::new();
        labels
            .try_reserve_exact(parsed.signals().len())
            .map_err(|_| StrSettingsDecodeError::AllocationFailed {
                resource: "STR signal-label index entries",
                requested: parsed.signals().len(),
            })?;

        for (index, signal) in parsed.signals().iter().enumerate() {
            if let Some(occurrences) = labels.iter_mut().find(|occurrences| {
                parsed.signals()[occurrences.first].header.label == signal.header.label
            }) {
                if occurrences.second.is_none() {
                    occurrences.second = Some(index);
                }
            } else {
                labels.push(SignalOccurrences {
                    first: index,
                    second: None,
                });
            }
        }
        labels.sort_unstable_by(|left, right| {
            parsed.signals()[left.first]
                .header
                .label
                .cmp(&parsed.signals()[right.first].header.label)
        });
        Ok(Self { labels })
    }

    fn select(&self, parsed: &EdfFile, selector: Selector) -> Option<usize> {
        for label in selector.labels {
            let Ok(position) = self.labels.binary_search_by(|occurrences| {
                parsed.signals()[occurrences.first]
                    .header
                    .label
                    .as_str()
                    .cmp(label)
            }) else {
                continue;
            };
            let occurrences = self.labels[position];
            match selector.occurrence {
                0 => return Some(occurrences.first),
                1 => {
                    if let Some(second) = occurrences.second {
                        return Some(second);
                    }
                }
                _ => return None,
            }
        }
        None
    }
}

const MODE: Selector = Selector::first(&["Mode", "Modus", "Funktion", "模式", "Mod"]);
const IPAP: Selector = Selector::first(&["Insp Pres", "IPAP", "S.BL.IPAP", "S.S.IPAP"]);
const EPAP: Selector = Selector::first(&[
    "Exp Pres",
    "EprPress.2s",
    "EPAP",
    "S.BL.EPAP",
    "EPRPress.2s",
    "S.S.EPAP",
]);
const MAX_PRESSURE: Selector = Selector::first(&[
    "Max Pressure",
    "Max. Druck",
    "Max druk",
    "最大压力",
    "Pression max.",
    "Max tryck",
    "S.AS.MaxPress",
    "S.A.MaxPress",
    "Azami Basınç",
]);
const MIN_PRESSURE: Selector = Selector::first(&[
    "Min Pressure",
    "Min. Druck",
    "Min druk",
    "最小压力",
    "Pression min.",
    "Min tryck",
    "S.AS.MinPress",
    "S.A.MinPress",
    "Min Basınç",
]);
const SET_PRESSURE: Selector = Selector::first(&[
    "Set Pressure",
    "Eingest. Druck",
    "Ingestelde druk",
    "设定压力",
    "Pres. prescrite",
    "Inställt tryck",
    "InstÃ¤llt tryck",
    "S.C.Press",
    "Basıncı Ayarl",
]);
const MAX_EPAP: Selector = Selector::first(&["Max EPAP"]);
const MIN_EPAP: Selector = Selector::first(&["Min EPAP", "S.VA.MinEPAP"]);
const MAX_IPAP: Selector = Selector::first(&["Max IPAP", "S.VA.MaxIPAP"]);
const MIN_IPAP: Selector = Selector::first(&["Min IPAP"]);
const PRESSURE_SUPPORT: Selector = Selector::first(&["PS", "S.VA.PS"]);

const CPAP_START_PRESSURE: Selector = Selector::first(&["S.C.StartPress"]);
const APAP_START_PRESSURE: Selector = Selector::first(&["S.AS.StartPress", "S.A.StartPress"]);
const APAP_FOR_HER_START_PRESSURE: Selector = Selector::first(&["S.AFH.StartPress"]);
const APAP_FOR_HER_MIN_PRESSURE: Selector = Selector::first(&["S.AFH.MinPress"]);
const APAP_FOR_HER_MAX_PRESSURE: Selector = Selector::first(&["S.AFH.MaxPress"]);
const BILEVEL_START_PRESSURE: Selector = Selector::first(&["S.BL.StartPress"]);
const VAUTO_START_PRESSURE: Selector = Selector::first(&["S.VA.StartPress"]);

const IVAPS_START_PRESSURE: Selector = Selector::first(&["S.i.StartPress"]);
const IVAPS_EPAP: Selector = Selector::first(&["S.i.EPAP"]);
const IVAPS_EPAP_AUTO: Selector = Selector::first(&["S.i.EPAPAuto"]);
const IVAPS_MIN_PS: Selector = Selector::first(&["S.i.MinPS"]);
const IVAPS_MIN_EPAP: Selector = Selector::first(&["S.i.MinEPAP"]);
const IVAPS_MAX_EPAP: Selector = Selector::first(&["S.i.MaxEPAP"]);
const IVAPS_MAX_PS: Selector = Selector::first(&["S.i.MaxPS"]);

const ASV_START_PRESSURE: Selector = Selector::first(&["S.AV.StartPress"]);
const ASV_EPAP: Selector = Selector::first(&["S.AV.EPAP"]);
const ASV_MIN_PS: Selector = Selector::first(&["S.AV.MinPS"]);
const ASV_MAX_PS: Selector = Selector::first(&["S.AV.MaxPS"]);

const ASV_AUTO_START_PRESSURE: Selector = Selector::first(&["S.AA.StartPress"]);
const ASV_AUTO_MIN_EPAP: Selector = Selector::first(&["S.AA.MinEPAP"]);
const ASV_AUTO_MAX_EPAP: Selector = Selector::first(&["S.AA.MaxEPAP"]);
const ASV_AUTO_MIN_PS: Selector = Selector::first(&["S.AA.MinPS"]);
const ASV_AUTO_MAX_PS: Selector = Selector::first(&["S.AA.MaxPS"]);

const EPR: Selector = Selector::first(&["EPR", "呼气释压(EP"]);
const EPR_LEVEL: Selector = Selector::first(&[
    "EPR Level",
    "EPR-Stufe",
    "EPR-niveau",
    "EPR 水平",
    "Niveau EPR",
    "EPR-nivå",
    "EPR-nivÃ¥",
    "S.EPR.Level",
    "EPR Düzeyi",
]);
const EPR_TYPE: Selector = Selector::first(&["S.EPR.EPRType"]);
const EPR_ENABLED: Selector = Selector::first(&["S.EPR.EPREnable"]);
const EPR_CLINICAL_ENABLED: Selector = Selector::first(&["S.EPR.ClinEnable"]);

const RAMP_TIME: Selector = Selector::first(&["S.RampTime"]);
const RAMP_ENABLED: Selector = Selector::first(&["S.RampEnable"]);
const ANTIBACTERIAL_FILTER: Selector = Selector::first(&["S.ABFilter"]);
const CLIMATE_CONTROL: Selector = Selector::first(&["S.ClimateControl"]);
const MASK_TYPE: Selector = Selector::first(&["S.Mask"]);
const PATIENT_ACCESS: Selector = Selector::first(&["S.PtAccess"]);
const SMART_START: Selector = Selector::first(&["S.SmartStart"]);
const SMART_STOP: Selector = Selector::first(&["S.SmartStop"]);
const HUMIDIFIER_ENABLED: Selector = Selector::first(&["S.HumEnable"]);
const HUMIDITY_LEVEL: Selector = Selector::first(&["S.HumLevel"]);
const TEMPERATURE_ENABLED: Selector = Selector::first(&["S.TempEnable"]);
const TEMPERATURE: Selector = Selector::first(&["S.Temp"]);
const COMFORT_RESPONSE: Selector = Selector::first(&["S.AS.Comfort"]);

const RISE_ENABLED: Selector = Selector::first(&["S.RiseEnable"]);
const RISE_TIME: Selector = Selector::first(&["S.RiseTime"]);
const CYCLE: Selector = Selector::first(&["S.Cycle"]);
const TRIGGER: Selector = Selector::first(&["S.Trigger"]);
const TI_MAX: Selector = Selector::first(&["S.TiMax"]);
const TI_MIN: Selector = Selector::first(&["S.TiMin"]);

const AIR11_S_RISE_ENABLED: Selector = Selector::first(&["S.S.RiseEnable"]);
const AIR11_S_RISE_TIME: Selector = Selector::first(&["S.S.RiseTime"]);
const AIR11_S_CYCLE: Selector = Selector::first(&["S.S.Cycle"]);
const AIR11_S_TRIGGER: Selector = Selector::first(&["S.S.Trigger"]);
const AIR11_S_TI_MAX: Selector = Selector::first(&["S.S.TiMax"]);
const AIR11_S_TI_MIN: Selector = Selector::first(&["S.S.TiMin"]);

const AIR11_VA_CYCLE: Selector = Selector::first(&["S.VA.Cycle"]);
const AIR11_VA_TRIGGER: Selector = Selector::first(&["S.VA.Trigger"]);
const AIR11_VA_TI_MAX: Selector = Selector::first(&["S.VA.TiMax"]);
const AIR11_VA_TI_MIN: Selector = Selector::first(&["S.VA.TiMin"]);

#[cfg(test)]
mod tests {
    use super::*;
    use opap_channels::by_stable_key;

    #[derive(Clone)]
    struct SignalFixture<'a> {
        label: &'a str,
        samples_per_record: usize,
        samples: Vec<i16>,
        physical_minimum: i32,
        physical_maximum: i32,
        digital_minimum: i32,
        digital_maximum: i32,
    }

    impl<'a> SignalFixture<'a> {
        fn new(label: &'a str, samples: &[i16]) -> Self {
            Self {
                label,
                samples_per_record: 1,
                samples: samples.to_vec(),
                physical_minimum: -32_768,
                physical_maximum: 32_767,
                digital_minimum: -32_768,
                digital_maximum: 32_767,
            }
        }

        fn samples_per_record(mut self, samples_per_record: usize) -> Self {
            self.samples_per_record = samples_per_record;
            self
        }

        fn calibration(
            mut self,
            physical_minimum: i32,
            physical_maximum: i32,
            digital_minimum: i32,
            digital_maximum: i32,
        ) -> Self {
            self.physical_minimum = physical_minimum;
            self.physical_maximum = physical_maximum;
            self.digital_minimum = digital_minimum;
            self.digital_maximum = digital_maximum;
            self
        }
    }

    fn field(value: &str, width: usize) -> Vec<u8> {
        assert!(value.is_ascii());
        assert!(value.len() <= width);
        let mut output = vec![b' '; width];
        output[..value.len()].copy_from_slice(value.as_bytes());
        output
    }

    fn synthetic_str(
        signals: &[SignalFixture<'_>],
        record_count: usize,
        start: &str,
        duration: &str,
        reserved: &str,
    ) -> Vec<u8> {
        assert_eq!(start.len(), 16);
        let header_bytes = 256 + signals.len() * 256;
        let mut bytes = Vec::new();
        bytes.extend(field("0", 8));
        bytes.extend(field("patient", 80));
        bytes.extend(field("ResMed SRN=fixture", 80));
        bytes.extend_from_slice(start.as_bytes());
        bytes.extend(field(&header_bytes.to_string(), 8));
        bytes.extend(field(reserved, 44));
        bytes.extend(field(&record_count.to_string(), 8));
        bytes.extend(field(duration, 8));
        bytes.extend(field(&signals.len().to_string(), 4));

        for signal in signals {
            bytes.extend(field(signal.label, 16));
        }
        for _ in signals {
            bytes.extend(field("", 80));
        }
        for _ in signals {
            bytes.extend(field("raw", 8));
        }
        for signal in signals {
            bytes.extend(field(&signal.physical_minimum.to_string(), 8));
        }
        for signal in signals {
            bytes.extend(field(&signal.physical_maximum.to_string(), 8));
        }
        for signal in signals {
            bytes.extend(field(&signal.digital_minimum.to_string(), 8));
        }
        for signal in signals {
            bytes.extend(field(&signal.digital_maximum.to_string(), 8));
        }
        for _ in signals {
            bytes.extend(field("", 80));
        }
        for signal in signals {
            bytes.extend(field(&signal.samples_per_record.to_string(), 8));
        }
        for _ in signals {
            bytes.extend(field("", 32));
        }
        assert_eq!(bytes.len(), header_bytes);

        for record in 0..record_count {
            for signal in signals {
                assert_eq!(
                    signal.samples.len(),
                    record_count * signal.samples_per_record
                );
                let start = record * signal.samples_per_record;
                let end = start + signal.samples_per_record;
                for sample in &signal.samples[start..end] {
                    bytes.extend_from_slice(&sample.to_le_bytes());
                }
            }
        }
        bytes
    }

    fn standard_str(signals: &[SignalFixture<'_>], record_count: usize) -> Vec<u8> {
        synthetic_str(signals, record_count, "01.01.2412.00.00", "86400", "")
    }

    fn decode(
        signals: &[SignalFixture<'_>],
        generation: StrSettingsGeneration,
    ) -> StrSettingsIndex {
        decode_str_settings(
            &standard_str(signals, 1),
            StrSettingsDecodeOptions { generation },
        )
        .expect("synthetic STR settings should decode")
    }

    fn setting<'a>(day: &'a StrDaySettings, key: &str) -> Option<&'a Setting> {
        day.settings.iter().find(|setting| setting.key == key)
    }

    fn integer(day: &StrDaySettings, key: &str) -> Option<i64> {
        match &setting(day, key)?.value {
            SettingValue::Integer(value) => Some(*value),
            _ => None,
        }
    }

    fn decimal(day: &StrDaySettings, key: &str) -> Option<f64> {
        match &setting(day, key)?.value {
            SettingValue::Decimal(value) => Some(*value),
            _ => None,
        }
    }

    fn boolean(day: &StrDaySettings, key: &str) -> Option<bool> {
        match &setting(day, key)?.value {
            SettingValue::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    fn text<'a>(day: &'a StrDaySettings, key: &str) -> Option<&'a str> {
        match &setting(day, key)?.value {
            SettingValue::Text(value) => Some(value),
            _ => None,
        }
    }

    fn derived_algorithm<'a>(day: &'a StrDaySettings, key: &str) -> Option<&'a str> {
        match &setting(day, key)?.origin {
            ValueOrigin::Derived { algorithm } => Some(algorithm),
            _ => None,
        }
    }

    #[test]
    fn cpap_uses_full_affine_calibration_and_preserves_provenance() {
        let result = decode(
            &[
                SignalFixture::new("Mode", &[0]),
                SignalFixture::new("Set Pressure", &[25]).calibration(4, 24, 0, 100),
                SignalFixture::new("EPR", &[2]),
                SignalFixture::new("EPR Level", &[3]),
                SignalFixture::new("S.C.StartPress", &[5]),
                SignalFixture::new("S.RampTime", &[20]),
                SignalFixture::new("S.RampEnable", &[1]),
                SignalFixture::new("S.SmartStart", &[1]),
                SignalFixture::new("S.HumEnable", &[1]),
                SignalFixture::new("S.HumLevel", &[4]),
                SignalFixture::new("S.TempEnable", &[1]),
                SignalFixture::new("S.Temp", &[27]),
                SignalFixture::new("S.Tube", &[2]),
            ],
            StrSettingsGeneration::PreAir11,
        );
        let day = &result.days[0];

        assert_eq!(integer(day, "pap.setting.resmed.therapy_mode"), Some(0));
        assert_eq!(text(day, "pap.setting.pap_mode"), Some("CPAP"));
        assert_eq!(
            derived_algorithm(day, "pap.setting.pap_mode"),
            Some(MODE_ALGORITHM)
        );
        assert_eq!(decimal(day, "pap.setting.resmed.set_pressure"), Some(9.0));
        assert_eq!(
            setting(day, "pap.setting.resmed.set_pressure").map(|setting| &setting.origin),
            Some(&ValueOrigin::DeviceReported)
        );
        assert_eq!(integer(day, "pap.setting.resmed.epr"), Some(2));
        assert_eq!(decimal(day, "pap.setting.resmed.epr_level"), Some(3.0));
        assert_eq!(decimal(day, "pap.setting.ramp_pressure"), Some(5.0));
        assert_eq!(decimal(day, "pap.setting.ramp_time"), Some(20.0));
        assert_eq!(boolean(day, "pap.setting.resmed.smart_start"), Some(true));
        assert_eq!(integer(day, "pap.setting.resmed.humidity_level"), Some(4));
        assert_eq!(decimal(day, "pap.setting.resmed.temperature"), Some(27.0));
        assert!(
            day.settings
                .iter()
                .all(|setting| !setting.key.contains("tube"))
        );
        assert_eq!(result.diagnostics, StrSettingsDiagnostics::default());
    }

    #[test]
    fn exact_alias_precedence_does_not_fall_through_negative_first_match() {
        let result = decode(
            &[
                SignalFixture::new("Mode", &[1]),
                SignalFixture::new("Min Pressure", &[-1]),
                SignalFixture::new("S.AS.MinPress", &[6]),
                SignalFixture::new("Max Pressure", &[13]),
                SignalFixture::new("min pressure", &[8]),
            ],
            StrSettingsGeneration::PreAir11,
        );
        let day = &result.days[0];
        assert_eq!(text(day, "pap.setting.pap_mode"), Some("APAP"));
        assert_eq!(decimal(day, "pap.setting.minimum_pressure"), None);
        assert_eq!(decimal(day, "pap.setting.maximum_pressure"), Some(13.0));
        assert_eq!(result.diagnostics.negative_values_omitted, 1);
    }

    #[test]
    fn non_ascii_localized_labels_expose_the_current_edf_parser_boundary() {
        let mut bytes = standard_str(&[SignalFixture::new("Mode", &[0])], 1);
        let label = "模式".as_bytes();
        bytes[256..272].fill(b' ');
        bytes[256..256 + label.len()].copy_from_slice(label);
        assert!(matches!(
            decode_str_settings(
                &bytes,
                StrSettingsDecodeOptions {
                    generation: StrSettingsGeneration::PreAir11,
                },
            ),
            Err(StrSettingsDecodeError::Parse(_))
        ));
    }

    #[test]
    fn air11_mode_table_preserves_raw_code_and_derives_generic_mode() {
        let cases = [
            (0, 16, "Unknown"),
            (1, 1, "APAP"),
            (2, 11, "APAP"),
            (3, 0, "CPAP"),
            (4, 3, "Bilevel fixed"),
            (5, 16, "Unknown"),
            (6, 7, "ASV"),
            (7, 8, "ASV variable EPAP"),
            (8, 6, "Bilevel auto fixed PS"),
            (9, 16, "Unknown"),
        ];
        for (raw, normalized, expected) in cases {
            let result = decode(
                &[SignalFixture::new("Mode", &[raw])],
                StrSettingsGeneration::Air11,
            );
            let day = &result.days[0];
            assert_eq!(
                integer(day, "pap.setting.resmed.therapy_mode"),
                Some(normalized)
            );
            assert_eq!(
                day.mode_evidence,
                Some(StrModeEvidence {
                    reported_code: i64::from(raw),
                    normalized_resmed_code: normalized,
                })
            );
            assert_eq!(
                derived_algorithm(day, "pap.setting.resmed.therapy_mode"),
                Some(MODE_ALGORITHM)
            );
            assert_eq!(text(day, "pap.setting.pap_mode"), Some(expected));
            assert_eq!(
                derived_algorithm(day, "pap.setting.pap_mode"),
                Some(MODE_ALGORITHM)
            );
        }
    }

    #[test]
    fn fixed_bilevel_maps_fixed_pressures_to_registry_bounds_and_controls() {
        let result = decode(
            &[
                SignalFixture::new("Mode", &[3]),
                SignalFixture::new("EPAP", &[5]),
                SignalFixture::new("IPAP", &[10]),
                SignalFixture::new("PS", &[5]),
                SignalFixture::new("S.BL.StartPress", &[4]),
                SignalFixture::new("S.RampEnable", &[1]),
                SignalFixture::new("S.RampTime", &[20]),
                SignalFixture::new("S.RiseEnable", &[1]),
                SignalFixture::new("S.RiseTime", &[300]),
                SignalFixture::new("S.Cycle", &[2]),
                SignalFixture::new("S.Trigger", &[3]),
                SignalFixture::new("S.TiMax", &[6]).calibration(0, 3, 0, 10),
                SignalFixture::new("S.TiMin", &[1]).calibration(0, 3, 0, 10),
                SignalFixture::new("S.EasyBreathe", &[1]),
            ],
            StrSettingsGeneration::PreAir11,
        );
        let day = &result.days[0];

        assert_eq!(decimal(day, "pap.setting.epap_minimum"), Some(5.0));
        assert_eq!(decimal(day, "pap.setting.epap_maximum"), Some(5.0));
        assert_eq!(decimal(day, "pap.setting.ipap_minimum"), Some(10.0));
        assert_eq!(decimal(day, "pap.setting.ipap_maximum"), Some(10.0));
        assert_eq!(
            derived_algorithm(day, "pap.setting.epap_minimum"),
            Some(FIXED_BOUNDS_ALGORITHM)
        );
        assert_eq!(decimal(day, "pap.setting.pressure_support"), Some(5.0));
        assert_eq!(integer(day, "pap.setting.resmed.cycle"), Some(2));
        assert_eq!(integer(day, "pap.setting.resmed.trigger"), Some(3));
        assert_eq!(boolean(day, "pap.setting.resmed.rise_enabled"), Some(true));
        assert_eq!(decimal(day, "pap.setting.resmed.rise_time"), Some(300.0));
        assert!((decimal(day, "pap.setting.resmed.timax").unwrap() - 1.8).abs() < 1.0e-12);
        assert!((decimal(day, "pap.setting.resmed.timin").unwrap() - 0.3).abs() < 1.0e-12);
        assert!(
            day.settings
                .iter()
                .all(|setting| !setting.key.contains("easy"))
        );
    }

    #[test]
    fn auto_bilevel_stores_min_epap_max_ipap_and_excludes_rise_controls() {
        let result = decode(
            &[
                SignalFixture::new("Mode", &[6]),
                SignalFixture::new("Min EPAP", &[5]),
                SignalFixture::new("Max IPAP", &[15]),
                SignalFixture::new("PS", &[4]),
                SignalFixture::new("S.Cycle", &[1]),
                SignalFixture::new("S.Trigger", &[4]),
                SignalFixture::new("S.TiMax", &[2]),
                SignalFixture::new("S.TiMin", &[1]),
                SignalFixture::new("S.RiseEnable", &[1]),
            ],
            StrSettingsGeneration::PreAir11,
        );
        let day = &result.days[0];
        assert_eq!(decimal(day, "pap.setting.epap_minimum"), Some(5.0));
        assert_eq!(decimal(day, "pap.setting.ipap_maximum"), Some(15.0));
        assert_eq!(decimal(day, "pap.setting.pressure_support"), Some(4.0));
        assert_eq!(integer(day, "pap.setting.resmed.cycle"), Some(1));
        assert_eq!(setting(day, "pap.setting.resmed.rise_enabled"), None);
    }

    #[test]
    fn asv_uses_first_duplicate_ps_occurrence_and_recomputes_ipap() {
        let result = decode(
            &[
                SignalFixture::new("Mode", &[7]),
                SignalFixture::new("S.AV.EPAP", &[5]),
                SignalFixture::new("S.AV.MinPS", &[2]),
                SignalFixture::new("S.AV.MaxPS", &[9]),
                SignalFixture::new("Min PS", &[1]),
                SignalFixture::new("Min PS", &[3]),
                SignalFixture::new("Max PS", &[7]),
                SignalFixture::new("Max PS", &[8]),
            ],
            StrSettingsGeneration::PreAir11,
        );
        let day = &result.days[0];
        assert_eq!(
            decimal(day, "pap.setting.pressure_support_minimum"),
            Some(1.0)
        );
        assert_eq!(
            decimal(day, "pap.setting.pressure_support_maximum"),
            Some(7.0)
        );
        assert_eq!(decimal(day, "pap.setting.ipap_maximum"), Some(12.0));
        assert_eq!(
            derived_algorithm(day, "pap.setting.ipap_maximum"),
            Some(PRESSURE_ALGORITHM)
        );
    }

    #[test]
    fn asv_auto_uses_second_duplicate_ps_occurrence_without_fallback() {
        let with_duplicates = decode(
            &[
                SignalFixture::new("Mode", &[8]),
                SignalFixture::new("S.AA.MinEPAP", &[5]),
                SignalFixture::new("S.AA.MaxEPAP", &[10]),
                SignalFixture::new("S.AA.MinPS", &[2]),
                SignalFixture::new("S.AA.MaxPS", &[9]),
                SignalFixture::new("Min PS", &[1]),
                SignalFixture::new("Min PS", &[3]),
                SignalFixture::new("Max PS", &[7]),
                SignalFixture::new("Max PS", &[8]),
            ],
            StrSettingsGeneration::PreAir11,
        );
        let day = &with_duplicates.days[0];
        assert_eq!(
            decimal(day, "pap.setting.pressure_support_minimum"),
            Some(3.0)
        );
        assert_eq!(
            decimal(day, "pap.setting.pressure_support_maximum"),
            Some(8.0)
        );
        assert_eq!(decimal(day, "pap.setting.ipap_minimum"), Some(8.0));
        assert_eq!(decimal(day, "pap.setting.ipap_maximum"), Some(18.0));

        let without_second_occurrence = decode(
            &[
                SignalFixture::new("Mode", &[8]),
                SignalFixture::new("S.AA.MinEPAP", &[5]),
                SignalFixture::new("S.AA.MaxEPAP", &[10]),
                SignalFixture::new("S.AA.MinPS", &[2]),
                SignalFixture::new("S.AA.MaxPS", &[9]),
                SignalFixture::new("Min PS", &[1]),
                SignalFixture::new("Max PS", &[7]),
            ],
            StrSettingsGeneration::PreAir11,
        );
        let day = &without_second_occurrence.days[0];
        assert_eq!(
            decimal(day, "pap.setting.pressure_support_minimum"),
            Some(2.0)
        );
        assert_eq!(
            decimal(day, "pap.setting.pressure_support_maximum"),
            Some(9.0)
        );
    }

    #[test]
    fn ivaps_derives_ipap_from_fixed_or_variable_epap_branch() {
        let fixed = decode(
            &[
                SignalFixture::new("Mode", &[9]),
                SignalFixture::new("S.i.EPAP", &[5]),
                SignalFixture::new("S.i.EPAPAuto", &[0]),
                SignalFixture::new("S.i.MinPS", &[3]),
                SignalFixture::new("S.i.MaxPS", &[7]),
            ],
            StrSettingsGeneration::PreAir11,
        );
        let day = &fixed.days[0];
        assert_eq!(decimal(day, "pap.setting.epap_minimum"), Some(5.0));
        assert_eq!(decimal(day, "pap.setting.epap_maximum"), Some(5.0));
        assert_eq!(decimal(day, "pap.setting.ipap_minimum"), Some(8.0));
        assert_eq!(decimal(day, "pap.setting.ipap_maximum"), Some(12.0));

        let variable = decode(
            &[
                SignalFixture::new("Mode", &[9]),
                SignalFixture::new("S.i.EPAPAuto", &[1]),
                SignalFixture::new("S.i.MinEPAP", &[4]),
                SignalFixture::new("S.i.MaxEPAP", &[8]),
                SignalFixture::new("S.i.MinPS", &[2]),
                SignalFixture::new("S.i.MaxPS", &[6]),
            ],
            StrSettingsGeneration::PreAir11,
        );
        let day = &variable.days[0];
        assert_eq!(decimal(day, "pap.setting.ipap_minimum"), Some(6.0));
        assert_eq!(decimal(day, "pap.setting.ipap_maximum"), Some(14.0));
    }

    #[test]
    fn ivaps_negative_zero_epap_auto_uses_fixed_epap_like_oscar() {
        assert!(epap_auto_is_fixed(
            Some(Number::reported(-0.0)),
            Some(Number::reported(5.0))
        ));
        assert!(!epap_auto_is_fixed(Some(Number::reported(-0.0)), None));
    }

    #[test]
    fn air11_epr_reconciliation_and_auto_ramp_match_store_settings() {
        let enabled = decode(
            &[
                SignalFixture::new("Mode", &[3]),
                SignalFixture::new("EPR", &[3]),
                SignalFixture::new("EPR Level", &[3]),
                SignalFixture::new("S.EPR.EPRType", &[2]),
                SignalFixture::new("S.EPR.EPREnable", &[2]),
                SignalFixture::new("S.EPR.ClinEnable", &[2]),
                SignalFixture::new("S.C.StartPress", &[5]),
                SignalFixture::new("S.RampTime", &[30]),
                SignalFixture::new("S.RampEnable", &[3]),
            ],
            StrSettingsGeneration::Air11,
        );
        let day = &enabled.days[0];
        assert_eq!(integer(day, "pap.setting.resmed.epr"), Some(2));
        assert_eq!(
            derived_algorithm(day, "pap.setting.resmed.epr"),
            Some(EPR_ALGORITHM)
        );
        assert_eq!(decimal(day, "pap.setting.resmed.epr_level"), Some(3.0));
        assert_eq!(integer(day, "pap.setting.resmed.ramp_enabled"), Some(2));
        assert_eq!(setting(day, "pap.setting.ramp_time"), None);
        assert_eq!(decimal(day, "pap.setting.ramp_pressure"), Some(5.0));

        let disabled = decode(
            &[
                SignalFixture::new("Mode", &[3]),
                SignalFixture::new("EPR Level", &[3]),
                SignalFixture::new("S.EPR.EPRType", &[2]),
                SignalFixture::new("S.EPR.EPREnable", &[2]),
                SignalFixture::new("S.EPR.ClinEnable", &[1]),
            ],
            StrSettingsGeneration::Air11,
        );
        let day = &disabled.days[0];
        assert_eq!(integer(day, "pap.setting.resmed.epr"), Some(0));
        assert_eq!(setting(day, "pap.setting.resmed.epr_level"), None);
    }

    #[test]
    fn air11_environment_access_and_bilevel_controls_are_normalized() {
        let result = decode(
            &[
                SignalFixture::new("Mode", &[4]),
                SignalFixture::new("S.S.EPAP", &[5]),
                SignalFixture::new("S.S.IPAP", &[10]),
                SignalFixture::new("S.ABFilter", &[2]),
                SignalFixture::new("S.ClimateControl", &[2]),
                SignalFixture::new("S.Mask", &[2]),
                SignalFixture::new("S.PtAccess", &[2]),
                SignalFixture::new("S.SmartStart", &[2]),
                SignalFixture::new("S.SmartStop", &[2]),
                SignalFixture::new("S.HumEnable", &[2]),
                SignalFixture::new("S.HumLevel", &[5]),
                SignalFixture::new("S.TempEnable", &[2]),
                SignalFixture::new("S.Temp", &[27]),
                SignalFixture::new("S.AS.Comfort", &[2]),
                SignalFixture::new("S.S.RiseEnable", &[2]),
                SignalFixture::new("S.S.RiseTime", &[300]),
                SignalFixture::new("S.S.Cycle", &[3]),
                SignalFixture::new("S.S.Trigger", &[5]),
                SignalFixture::new("S.S.TiMax", &[2]),
                SignalFixture::new("S.S.TiMin", &[1]),
                SignalFixture::new("S.S.EasyBreathe", &[2]),
                SignalFixture::new("S.Tube", &[1]),
            ],
            StrSettingsGeneration::Air11,
        );
        let day = &result.days[0];
        assert_eq!(
            boolean(day, "pap.setting.resmed.antibacterial_filter"),
            Some(true)
        );
        assert_eq!(
            derived_algorithm(day, "pap.setting.resmed.antibacterial_filter"),
            Some(AIR11_ENUM_ALGORITHM)
        );
        assert_eq!(integer(day, "pap.setting.resmed.climate_control"), Some(1));
        assert_eq!(integer(day, "pap.setting.resmed.mask_type"), Some(0));
        assert_eq!(
            derived_algorithm(day, "pap.setting.resmed.mask_type"),
            Some(AIR11_ENUM_ALGORITHM)
        );
        assert_eq!(setting(day, "pap.setting.resmed.patient_access"), None);
        assert_eq!(integer(day, "pap.setting.resmed.patient_view"), Some(1));
        assert_eq!(
            boolean(day, "pap.setting.resmed.humidifier_enabled"),
            Some(true)
        );
        assert_eq!(integer(day, "pap.setting.resmed.humidity_level"), Some(5));
        assert_eq!(
            integer(day, "pap.setting.resmed.temperature_enabled"),
            Some(1)
        );
        assert_eq!(decimal(day, "pap.setting.resmed.temperature"), Some(27.0));
        assert_eq!(integer(day, "pap.setting.resmed.comfort_response"), Some(1));
        assert_eq!(boolean(day, "pap.setting.resmed.rise_enabled"), Some(true));
        assert_eq!(integer(day, "pap.setting.resmed.cycle"), Some(2));
        assert_eq!(integer(day, "pap.setting.resmed.trigger"), Some(4));
        assert!(
            day.settings
                .iter()
                .all(|setting| !setting.key.contains("tube") && !setting.key.contains("easy"))
        );
    }

    #[test]
    fn malformed_known_signals_warn_and_fail_closed_per_setting() {
        let result = decode(
            &[
                SignalFixture::new("Mode", &[0, 0]).samples_per_record(2),
                SignalFixture::new("S.SmartStart", &[1]).calibration(0, 1, 0, 0),
                SignalFixture::new("S.ClimateControl", &[1]).calibration(0, 1, 0, 2),
            ],
            StrSettingsGeneration::PreAir11,
        );
        assert!(result.days[0].settings.is_empty());
        assert_eq!(result.diagnostics.invalid_signal_shapes, 1);
        assert_eq!(result.diagnostics.invalid_calibrations, 1);
        assert_eq!(result.diagnostics.invalid_categorical_values, 1);
        assert_eq!(
            result
                .warnings
                .iter()
                .map(|warning| warning.kind)
                .collect::<Vec<_>>(),
            vec![
                StrSettingsWarningKind::InvalidSignalShape,
                StrSettingsWarningKind::InvalidCategoricalValue,
                StrSettingsWarningKind::InvalidCalibration,
            ]
        );
    }

    #[test]
    fn warnings_are_globally_capped_and_count_drops() {
        let record_count = MAX_WARNINGS + 76;
        let bytes = standard_str(
            &[SignalFixture::new("Mode", &vec![0; record_count]).calibration(0, 1, 0, 0)],
            record_count,
        );
        let result = decode_str_settings(
            &bytes,
            StrSettingsDecodeOptions {
                generation: StrSettingsGeneration::PreAir11,
            },
        )
        .expect("bounded warning fixture should decode");
        assert_eq!(result.warnings.len(), MAX_WARNINGS);
        assert_eq!(result.diagnostics.invalid_calibrations, 1_100);
        assert_eq!(result.diagnostics.warnings_dropped, 76);
    }

    #[test]
    fn settings_without_mode_are_retained_but_pressure_block_is_suppressed() {
        let result = decode(
            &[
                SignalFixture::new("Set Pressure", &[8]),
                SignalFixture::new("S.SmartStart", &[1]),
            ],
            StrSettingsGeneration::PreAir11,
        );
        let day = &result.days[0];
        assert_eq!(setting(day, "pap.setting.resmed.set_pressure"), None);
        assert_eq!(boolean(day, "pap.setting.resmed.smart_start"), Some(true));
    }

    #[test]
    fn signal_order_does_not_change_canonical_setting_order() {
        let forward = [
            SignalFixture::new("Mode", &[0]),
            SignalFixture::new("Set Pressure", &[8]),
            SignalFixture::new("S.SmartStart", &[1]),
            SignalFixture::new("S.ClimateControl", &[0]),
        ];
        let reverse = [
            SignalFixture::new("S.ClimateControl", &[0]),
            SignalFixture::new("S.SmartStart", &[1]),
            SignalFixture::new("Set Pressure", &[8]),
            SignalFixture::new("Mode", &[0]),
        ];
        let left = decode(&forward, StrSettingsGeneration::PreAir11);
        let right = decode(&reverse, StrSettingsGeneration::PreAir11);
        assert_eq!(left.days[0].settings, right.days[0].settings);
        assert!(
            left.days[0]
                .settings
                .windows(2)
                .all(|pair| pair[0].key < pair[1].key)
        );
        for setting in &left.days[0].settings {
            let definition =
                by_stable_key(&setting.key).expect("output key must remain registered");
            assert_eq!(definition.kind, ChannelKind::Setting);
            assert_eq!(setting.label, definition.label);
        }
    }

    #[test]
    fn first_duplicate_mode_occurrence_is_authoritative() {
        let result = decode(
            &[
                SignalFixture::new("Mode", &[0]),
                SignalFixture::new("Mode", &[1]),
            ],
            StrSettingsGeneration::PreAir11,
        );
        assert_eq!(text(&result.days[0], "pap.setting.pap_mode"), Some("CPAP"));
    }

    #[test]
    fn multiple_records_produce_independent_therapy_day_settings() {
        let bytes = standard_str(
            &[
                SignalFixture::new("Mode", &[0, 1]),
                SignalFixture::new("Set Pressure", &[8, 9]),
                SignalFixture::new("Min Pressure", &[5, 6]),
                SignalFixture::new("Max Pressure", &[12, 13]),
            ],
            2,
        );
        let result = decode_str_settings(
            &bytes,
            StrSettingsDecodeOptions {
                generation: StrSettingsGeneration::PreAir11,
            },
        )
        .expect("multi-record fixture should decode");
        assert_eq!(result.days.len(), 2);
        assert_eq!(result.days[0].record_index, 0);
        assert_eq!(result.days[1].record_index, 1);
        assert_eq!(
            decimal(&result.days[0], "pap.setting.resmed.set_pressure"),
            Some(8.0)
        );
        assert_eq!(
            decimal(&result.days[1], "pap.setting.minimum_pressure"),
            Some(6.0)
        );
        assert_eq!(
            decimal(&result.days[1], "pap.setting.maximum_pressure"),
            Some(13.0)
        );
    }

    #[test]
    fn structural_policy_rejects_trailing_non_daily_and_edf_plus_input() {
        let signal = SignalFixture::new("Mode", &[0]);

        let mut trailing = standard_str(std::slice::from_ref(&signal), 1);
        trailing.push(0);
        assert_eq!(
            decode_str_settings(
                &trailing,
                StrSettingsDecodeOptions {
                    generation: StrSettingsGeneration::PreAir11,
                },
            ),
            Err(StrSettingsDecodeError::TrailingData { bytes: 1 })
        );

        let hourly = synthetic_str(
            std::slice::from_ref(&signal),
            1,
            "01.01.2412.00.00",
            "3600",
            "",
        );
        assert_eq!(
            decode_str_settings(
                &hourly,
                StrSettingsDecodeOptions {
                    generation: StrSettingsGeneration::PreAir11,
                },
            ),
            Err(StrSettingsDecodeError::InvalidRecordDuration)
        );

        let morning = synthetic_str(
            std::slice::from_ref(&signal),
            1,
            "01.01.2411.00.00",
            "86400",
            "",
        );
        assert_eq!(
            decode_str_settings(
                &morning,
                StrSettingsDecodeOptions {
                    generation: StrSettingsGeneration::PreAir11,
                },
            ),
            Err(StrSettingsDecodeError::InvalidRecordStart)
        );

        let annotation_signal =
            SignalFixture::new("EDF Annotations", &[0; 8]).samples_per_record(8);
        let edf_plus = synthetic_str(
            std::slice::from_ref(&annotation_signal),
            1,
            "01.01.2412.00.00",
            "86400",
            "EDF+C",
        );
        assert_eq!(
            decode_str_settings(
                &edf_plus,
                StrSettingsDecodeOptions {
                    generation: StrSettingsGeneration::PreAir11,
                },
            ),
            Err(StrSettingsDecodeError::UnsupportedEdfPlus)
        );
    }

    #[test]
    fn file_size_limit_is_checked_before_parsing() {
        let bytes = vec![0; RESMED_STR_SETTINGS_MAX_FILE_BYTES + 1];
        assert_eq!(
            decode_str_settings(
                &bytes,
                StrSettingsDecodeOptions {
                    generation: StrSettingsGeneration::PreAir11,
                },
            ),
            Err(StrSettingsDecodeError::FileTooLarge {
                limit: RESMED_STR_SETTINGS_MAX_FILE_BYTES,
                actual: RESMED_STR_SETTINGS_MAX_FILE_BYTES + 1,
            })
        );
    }
}
