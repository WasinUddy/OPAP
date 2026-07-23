// SPDX-License-Identifier: GPL-3.0-only
//
// The compatibility values below are derived from OSCAR-code at commit
// 64c5e90a26f91fb15868bcfcccde0c1e1522ac86.
// Copyright (c) 2019-2025 The OSCAR Team
// Copyright (c) 2011-2018 Mark Watkins

use crate::{
    AnalyticsRole, ChannelDefinition, ChannelKind, EventPayload, EventSemantics, EventTimestamp,
    LegacyOscarChannelId, LegacyOscarMetadata, ResmedFileKind, ResmedSignalDescriptor,
    ResmedSpanEndpointDescriptor, SpanEndpointRole, SpanEndpointTimestamp, SpanPayload,
    SpanSemantics, StableChannelKey, Unit,
};

const RESMED_EVE_EVENT: EventSemantics = EventSemantics {
    timestamp: EventTimestamp::ResmedEdfAnnotationOnset,
    payload: EventPayload::ResmedEdfAnnotationDurationSecondsOrMissing,
    count_each_record: true,
};

const LOADER_EVENT: EventSemantics = EventSemantics {
    timestamp: EventTimestamp::LoaderDefined,
    payload: EventPayload::LoaderDefined,
    count_each_record: true,
};

const RESMED_CSL_CSR_SPAN: SpanSemantics = SpanSemantics {
    endpoint_timestamp: SpanEndpointTimestamp::ResmedEdfAnnotationOnset,
    stored_timestamp: SpanEndpointRole::End,
    payload: SpanPayload::ElapsedSecondsBetweenEndpoints,
    endpoints: &[
        ResmedSpanEndpointDescriptor {
            file: ResmedFileKind::Csl,
            alias: "CSR Start",
            role: SpanEndpointRole::Start,
        },
        ResmedSpanEndpointDescriptor {
            file: ResmedFileKind::Csl,
            alias: "CSR End",
            role: SpanEndpointRole::End,
        },
    ],
};

/// Complete OPAP channel registry in stable-key order.
///
/// The slice is intentionally static and allocation-free. New entries require
/// provenance, invariant tests, and a stable-key compatibility decision.
pub static CHANNELS: &[ChannelDefinition] = &[
    ChannelDefinition {
        key: StableChannelKey::new("pap.event.clear_airway"),
        label: "Clear airway",
        kind: ChannelKind::Event,
        unit: Unit::EventsPerHour,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1001),
            cpp_symbol: "CPAP_ClearAirway",
            lookup_code: "ClearAirway",
            english_label: "Clear Airway (CA)",
            short_label: "CA",
            unit_label: "Events/hr",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Eve,
            aliases: &["Central apnea"],
        }],
        event_semantics: Some(RESMED_EVE_EVENT),
        span_semantics: None,
        analytics_role: Some(AnalyticsRole::AhiEventCount),
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.event.device_reported_apnea"),
        label: "Device-reported apnea",
        kind: ChannelKind::Event,
        unit: Unit::EventsPerHour,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1010),
            cpp_symbol: "CPAP_AllApnea",
            lookup_code: "AllApnea",
            english_label: "Apnea (A)",
            short_label: "A",
            unit_label: "Events/hr",
        },
        resmed_signals: &[],
        event_semantics: Some(LOADER_EVENT),
        span_semantics: None,
        analytics_role: Some(AnalyticsRole::AhiEventCount),
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.event.hypopnea"),
        label: "Hypopnea",
        kind: ChannelKind::Event,
        unit: Unit::EventsPerHour,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1003),
            cpp_symbol: "CPAP_Hypopnea",
            lookup_code: "Hypopnea",
            english_label: "Hypopnea (H)",
            short_label: "H",
            unit_label: "Events/hr",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Eve,
            aliases: &["Hypopnea"],
        }],
        event_semantics: Some(RESMED_EVE_EVENT),
        span_semantics: None,
        analytics_role: Some(AnalyticsRole::AhiEventCount),
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.event.obstructive_apnea"),
        label: "Obstructive apnea",
        kind: ChannelKind::Event,
        unit: Unit::EventsPerHour,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1002),
            cpp_symbol: "CPAP_Obstructive",
            lookup_code: "Obstructive",
            english_label: "Obstructive Apnea (OA)",
            short_label: "OA",
            unit_label: "Events/hr",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Eve,
            aliases: &["Obstructive apnea"],
        }],
        event_semantics: Some(RESMED_EVE_EVENT),
        span_semantics: None,
        analytics_role: Some(AnalyticsRole::AhiEventCount),
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.event.rera"),
        label: "RERA",
        kind: ChannelKind::Event,
        unit: Unit::EventsPerHour,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1006),
            cpp_symbol: "CPAP_RERA",
            lookup_code: "RERA",
            english_label: "RERA (RE)",
            short_label: "RE",
            unit_label: "Events/hr",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Eve,
            aliases: &["Arousal"],
        }],
        event_semantics: Some(RESMED_EVE_EVENT),
        span_semantics: None,
        analytics_role: Some(AnalyticsRole::RdiAdditionalEventCount),
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.event.unclassified_apnea"),
        label: "Unclassified apnea",
        kind: ChannelKind::Event,
        unit: Unit::EventsPerHour,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1004),
            cpp_symbol: "CPAP_Apnea",
            lookup_code: "Apnea",
            english_label: "Unclassified Apnea (UA)",
            short_label: "UA",
            unit_label: "Events/hr",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Eve,
            aliases: &["Apnea"],
        }],
        event_semantics: Some(RESMED_EVE_EVENT),
        span_semantics: None,
        analytics_role: Some(AnalyticsRole::AhiEventCount),
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.epap"),
        label: "Expiratory pressure",
        kind: ChannelKind::SampledSeries,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x110e),
            cpp_symbol: "CPAP_EPAP",
            lookup_code: "EPAP",
            english_label: "EPAP",
            short_label: "EPAP",
            unit_label: "cmH2O",
        },
        resmed_signals: &[
            ResmedSignalDescriptor {
                file: ResmedFileKind::Pld,
                aliases: &[
                    "Exp Pres",
                    "EprPress.2s",
                    "EPAP",
                    "S.BL.EPAP",
                    "EPRPress.2s",
                    "S.S.EPAP",
                ],
            },
            ResmedSignalDescriptor {
                file: ResmedFileKind::Str,
                aliases: &["Exp Pres", "EPAP", "S.BL.EPAP", "S.S.EPAP"],
            },
        ],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.expiratory_time"),
        label: "Expiratory time",
        kind: ChannelKind::SampledSeries,
        unit: Unit::Seconds,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x110a),
            cpp_symbol: "CPAP_Te",
            lookup_code: "Te",
            english_label: "Expiratory Time",
            short_label: "Exp. Time",
            unit_label: "Seconds",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["Te", "B5ETime.2s"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.flow_limitation"),
        label: "Flow limitation",
        kind: ChannelKind::SampledSeries,
        unit: Unit::SeverityZeroToOne,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1113),
            cpp_symbol: "CPAP_FLG",
            lookup_code: "FLG",
            english_label: "Flow Limitation",
            short_label: "Flow Limit.",
            unit_label: "Severity (0-1)",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["FFL Index", "FlowLim.2s"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.flow_rate"),
        label: "Flow rate",
        kind: ChannelKind::SampledSeries,
        unit: Unit::LitersPerMinute,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1100),
            cpp_symbol: "CPAP_FlowRate",
            lookup_code: "FlowRate",
            english_label: "Flow Rate",
            short_label: "Flow Rate",
            unit_label: "l/min",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Brp,
            aliases: &["Flow", "Flow.40ms"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.inspiratory_time"),
        label: "Inspiratory time",
        kind: ChannelKind::SampledSeries,
        unit: Unit::Seconds,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x110b),
            cpp_symbol: "CPAP_Ti",
            lookup_code: "Ti",
            english_label: "Inspiratory Time",
            short_label: "Insp. Time",
            unit_label: "Seconds",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["Ti", "B5ITime.2s"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.ipap"),
        label: "Inspiratory pressure",
        kind: ChannelKind::SampledSeries,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x110d),
            cpp_symbol: "CPAP_IPAP",
            lookup_code: "IPAP",
            english_label: "IPAP",
            short_label: "IPAP",
            unit_label: "cmH2O",
        },
        resmed_signals: &[
            ResmedSignalDescriptor {
                file: ResmedFileKind::Pld,
                aliases: &["Insp Pres", "IPAP", "S.BL.IPAP", "S.S.IPAP"],
            },
            ResmedSignalDescriptor {
                file: ResmedFileKind::Str,
                aliases: &["Insp Pres", "IPAP", "S.BL.IPAP", "S.S.IPAP"],
            },
        ],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.leak_rate"),
        label: "Leak rate",
        kind: ChannelKind::SampledSeries,
        unit: Unit::LitersPerMinute,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1108),
            cpp_symbol: "CPAP_Leak",
            lookup_code: "Leak",
            english_label: "Leak Rate",
            short_label: "Leak Rate",
            unit_label: "l/min",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &[
                "Leak",
                "Leck",
                "Fuites",
                "Fuite",
                "Fuga",
                "漏气",
                "Lekk",
                "Läck",
                "LÃ¤ck",
                "Leak.2s",
                "Sızıntı",
            ],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: Some(AnalyticsRole::LeakSummary),
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.mask_pressure"),
        label: "Mask pressure",
        kind: ChannelKind::SampledSeries,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1101),
            cpp_symbol: "CPAP_MaskPressure",
            lookup_code: "MaskPressure",
            english_label: "Mask Pressure",
            short_label: "Mask Pressure",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["Mask Pres", "MaskPress.2s"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.mask_pressure_high_rate"),
        label: "Mask pressure (high rate)",
        kind: ChannelKind::SampledSeries,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1102),
            cpp_symbol: "CPAP_MaskPressureHi",
            lookup_code: "MaskPressureHi",
            english_label: "Mask Pressure",
            short_label: "Mask Pressure",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Brp,
            aliases: &["Mask Pres", "Press.40ms"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.minute_ventilation"),
        label: "Minute ventilation",
        kind: ChannelKind::SampledSeries,
        unit: Unit::LitersPerMinute,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1105),
            cpp_symbol: "CPAP_MinuteVent",
            lookup_code: "MinuteVent",
            english_label: "Minute Ventilation",
            short_label: "Minute Vent.",
            unit_label: "l/min",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["MV", "VM", "MinVent.2s"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.respiratory_event"),
        label: "Respiratory event signal",
        kind: ChannelKind::SampledSeries,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1112),
            cpp_symbol: "CPAP_RespEvent",
            lookup_code: "RespEvent",
            english_label: "Respiratory Event",
            short_label: "Resp. Event",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Brp,
            aliases: &["Resp Event", "TrigCycEvt.40ms"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.respiratory_rate"),
        label: "Respiratory rate",
        kind: ChannelKind::SampledSeries,
        unit: Unit::BreathsPerMinute,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1106),
            cpp_symbol: "CPAP_RespRate",
            lookup_code: "RespRate",
            english_label: "Respiratory Rate",
            short_label: "Resp. Rate",
            unit_label: "Breaths/min",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["RR", "AF", "FR", "RespRate.2s"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.snore"),
        label: "Snore",
        kind: ChannelKind::SampledSeries,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1104),
            cpp_symbol: "CPAP_Snore",
            lookup_code: "Snore",
            english_label: "Snore",
            short_label: "Snore",
            unit_label: "?",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["Snore", "Snore.2s"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.target_minute_ventilation"),
        label: "Target minute ventilation",
        kind: ChannelKind::SampledSeries,
        unit: Unit::LitersPerMinute,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1114),
            cpp_symbol: "CPAP_TgMV",
            lookup_code: "TgMV",
            english_label: "Target Minute Ventilation",
            short_label: "Target Vent.",
            unit_label: "l/min",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["TgMV", "TgtVent.2s"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.therapy_pressure"),
        label: "Therapy pressure",
        kind: ChannelKind::SampledSeries,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x110c),
            cpp_symbol: "CPAP_Pressure",
            lookup_code: "Pressure",
            english_label: "Pressure",
            short_label: "Pressure",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["Therapy Pres", "Press.2s"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: Some(AnalyticsRole::PressureSummary),
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.tidal_volume"),
        label: "Tidal volume",
        kind: ChannelKind::SampledSeries,
        unit: Unit::Milliliters,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1103),
            cpp_symbol: "CPAP_TidalVolume",
            lookup_code: "TidalVolume",
            english_label: "Tidal Volume",
            short_label: "Tidal Volume",
            unit_label: "ml",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["Vt", "VC", "TidVol.2s"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.epap_maximum"),
        label: "Maximum expiratory pressure",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x111d),
            cpp_symbol: "CPAP_EPAPHi",
            lookup_code: "EPAPHi",
            english_label: "Max EPAP",
            short_label: "Max EPAP",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["Max EPAP", "S.i.MaxEPAP", "S.AA.MaxEPAP"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.epap_minimum"),
        label: "Minimum expiratory pressure",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x111c),
            cpp_symbol: "CPAP_EPAPLo",
            lookup_code: "EPAPLo",
            english_label: "Min EPAP",
            short_label: "Min EPAP",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["Min EPAP", "S.VA.MinEPAP", "S.i.MinEPAP", "S.AA.MinEPAP"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.ipap_maximum"),
        label: "Maximum inspiratory pressure",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1111),
            cpp_symbol: "CPAP_IPAPHi",
            lookup_code: "IPAPHi",
            english_label: "Max IPAP",
            short_label: "Max IPAP",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["Max IPAP", "S.VA.MaxIPAP"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.ipap_minimum"),
        label: "Minimum inspiratory pressure",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1110),
            cpp_symbol: "CPAP_IPAPLo",
            lookup_code: "IPAPLo",
            english_label: "Min IPAP",
            short_label: "Min IPAP",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["Min IPAP"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.maximum_pressure"),
        label: "Maximum therapy pressure",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1021),
            cpp_symbol: "CPAP_PressureMax",
            lookup_code: "PressureMax",
            english_label: "Max Pressure",
            short_label: "Pressure Max",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &[
                "Max Pressure",
                "Max. Druck",
                "Max druk",
                "最大压力",
                "Pression max.",
                "Max tryck",
                "S.AS.MaxPress",
                "S.A.MaxPress",
                "Azami Basınç",
                "S.AFH.MaxPress",
            ],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.minimum_pressure"),
        label: "Minimum therapy pressure",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1020),
            cpp_symbol: "CPAP_PressureMin",
            lookup_code: "PressureMin",
            english_label: "Min Pressure",
            short_label: "Pressure Min",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &[
                "Min Pressure",
                "Min. Druck",
                "Min druk",
                "最小压力",
                "Pression min.",
                "Min tryck",
                "S.AS.MinPress",
                "S.A.MinPress",
                "Min Basınç",
                "S.AFH.MinPress",
            ],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.pap_mode"),
        label: "PAP mode",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1200),
            cpp_symbol: "CPAP_Mode",
            lookup_code: "PAPMode",
            english_label: "PAP Mode",
            short_label: "PAP Mode",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["Mode", "Modus", "Funktion", "模式", "Mod"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.pressure_support"),
        label: "Pressure support",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x110f),
            cpp_symbol: "CPAP_PS",
            lookup_code: "PS",
            english_label: "PS",
            short_label: "PS",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["PS", "S.VA.PS"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.pressure_support_maximum"),
        label: "Maximum pressure support",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x111b),
            cpp_symbol: "CPAP_PSMax",
            lookup_code: "PSMax",
            english_label: "PS Max",
            short_label: "PS Max",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["Max PS", "S.i.MaxPS", "S.AV.MaxPS", "S.AA.MaxPS"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.pressure_support_minimum"),
        label: "Minimum pressure support",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x111a),
            cpp_symbol: "CPAP_PSMin",
            lookup_code: "PSMin",
            english_label: "PS Min",
            short_label: "PS Min",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["Min PS", "S.i.MinPS", "S.AV.MinPS", "S.AA.MinPS"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.ramp_pressure"),
        label: "Ramp pressure",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1023),
            cpp_symbol: "CPAP_RampPressure",
            lookup_code: "RampPressure",
            english_label: "Ramp Pressure",
            short_label: "Ramp Pressure",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &[
                "S.C.StartPress",
                "S.AS.StartPress",
                "S.A.StartPress",
                "S.AFH.StartPress",
                "S.BL.StartPress",
                "S.VA.StartPress",
                "S.i.StartPress",
                "S.AV.StartPress",
                "S.AA.StartPress",
            ],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.ramp_time"),
        label: "Ramp time",
        kind: ChannelKind::Setting,
        unit: Unit::Minutes,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1022),
            cpp_symbol: "CPAP_RampTime",
            lookup_code: "RampTime",
            english_label: "Ramp Time",
            short_label: "Ramp Time",
            unit_label: "Minutes",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.RampTime"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.antibacterial_filter"),
        label: "Antibacterial filter",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe209),
            cpp_symbol: "RMS9_ABFilter",
            lookup_code: "RMS9_ABFilter",
            english_label: "AB Filter",
            short_label: "Antibacterial Filter",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.ABFilter"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.climate_control"),
        label: "Climate control",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe20b),
            cpp_symbol: "RMS9_ClimateControl",
            lookup_code: "RMS9_ClimateControl",
            english_label: "Climate Control",
            short_label: "Climate Control",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.ClimateControl"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.comfort_response"),
        label: "Comfort response",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe20e),
            cpp_symbol: "RMAS1x_Comfort",
            lookup_code: "RMAS1x_Comfort",
            english_label: "Response",
            short_label: "Response",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.AS.Comfort"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.cycle"),
        label: "Cycle sensitivity",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe214),
            cpp_symbol: "RMAS1x_Cycle",
            lookup_code: "RMAS1x_Cycle",
            english_label: "Cycle",
            short_label: "Cycle",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.Cycle", "S.S.Cycle", "S.VA.Cycle"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.epr"),
        label: "Expiratory pressure relief",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe201),
            cpp_symbol: "RMS9_EPR",
            lookup_code: "EPR",
            english_label: "EPR",
            short_label: "EPR",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["EPR", "呼气释压(EP"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.epr_level"),
        label: "Expiratory pressure relief level",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe202),
            cpp_symbol: "RMS9_EPRLevel",
            lookup_code: "EPRLevel",
            english_label: "EPR Level",
            short_label: "EPR Level",
            unit_label: "cmH2O",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &[
                "EPR Level",
                "EPR-Stufe",
                "EPR-niveau",
                "EPR 水平",
                "Niveau EPR",
                "EPR-nivå",
                "EPR-nivÃ¥",
                "S.EPR.Level",
                "EPR Düzeyi",
            ],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.humidifier_enabled"),
        label: "Humidifier enabled",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe205),
            cpp_symbol: "RMS9_HumidStatus",
            lookup_code: "RMS9_HumidStat",
            english_label: "Humid. Status",
            short_label: "Humidifier Status",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.HumEnable"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.humidity_level"),
        label: "Humidity level",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe206),
            cpp_symbol: "RMS9_HumidLevel",
            lookup_code: "RMS9_HumidLevel",
            english_label: "Humid. Level",
            short_label: "Humidity Level",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.HumLevel"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.mask_type"),
        label: "Mask type",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe20c),
            cpp_symbol: "RMS9_Mask",
            lookup_code: "RMS9_Mask",
            english_label: "Mask",
            short_label: "Mask",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.Mask"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.patient_access"),
        label: "Patient access",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe20a),
            cpp_symbol: "RMS9_PtAccess",
            lookup_code: "RMS9_PTAccess",
            english_label: "Pt. Access",
            short_label: "Essentials",
            unit_label: "",
        },
        resmed_signals: &[],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.patient_view"),
        label: "Patient view",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe210),
            cpp_symbol: "RMAS11_PtView",
            lookup_code: "RMAS11_PTView",
            english_label: "Patient View",
            short_label: "Patient View",
            unit_label: "",
        },
        resmed_signals: &[],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.ramp_enabled"),
        label: "Ramp enabled",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe20d),
            cpp_symbol: "RMS9_RampEnable",
            lookup_code: "RMS9_RampEnable",
            english_label: "Ramp",
            short_label: "Ramp",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.RampEnable"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.rise_enabled"),
        label: "Rise enabled",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe212),
            cpp_symbol: "RMAS1x_RiseEnable",
            lookup_code: "RMAS1x_RiseEnable",
            english_label: "RiseEnable",
            short_label: "RiseEnable",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.RiseEnable", "S.S.RiseEnable"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.rise_time"),
        label: "Rise time",
        kind: ChannelKind::Setting,
        unit: Unit::Milliseconds,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe213),
            cpp_symbol: "RMAS1x_RiseTime",
            lookup_code: "RMAS1x_RiseTime",
            english_label: "RiseTime",
            short_label: "RiseTime",
            unit_label: "milliSeconds",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.RiseTime", "S.S.RiseTime"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.set_pressure"),
        label: "Set pressure",
        kind: ChannelKind::Setting,
        unit: Unit::CentimetersOfWater,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1162),
            cpp_symbol: "RMS9_SetPressure",
            lookup_code: "SetPressure",
            english_label: "Set Pressure",
            short_label: "Pressure",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &[
                "Set Pressure",
                "Eingest. Druck",
                "Ingestelde druk",
                "设定压力",
                "Pres. prescrite",
                "Inställt tryck",
                "InstÃ¤llt tryck",
                "S.C.Press",
                "Basıncı Ayarl",
            ],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.smart_start"),
        label: "SmartStart",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe204),
            cpp_symbol: "RMS9_SmartStart",
            lookup_code: "RMS9_SmartStart",
            english_label: "SmartStart",
            short_label: "Smart Start",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.SmartStart"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.smart_stop"),
        label: "SmartStop",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe20f),
            cpp_symbol: "RMAS11_SmartStop",
            lookup_code: "RMAS11_SmartStop",
            english_label: "SmartStop",
            short_label: "Smart Stop",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.SmartStop"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.temperature"),
        label: "ClimateLine temperature",
        kind: ChannelKind::Setting,
        unit: Unit::DegreesCelsius,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe207),
            cpp_symbol: "RMS9_Temp",
            lookup_code: "RMS9_Temp",
            english_label: "Temperature",
            short_label: "Temperature",
            unit_label: "ºC",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.Temp"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.temperature_enabled"),
        label: "ClimateLine temperature enabled",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe208),
            cpp_symbol: "RMS9_TempEnable",
            lookup_code: "RMS9_TempEnable",
            english_label: "Temp. Enable",
            short_label: "Temperature Enable",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.TempEnable"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.therapy_mode"),
        label: "ResMed therapy mode",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe203),
            cpp_symbol: "RMS9_Mode",
            lookup_code: "RMS9_Mode",
            english_label: "Mode",
            short_label: "Mode",
            unit_label: "",
        },
        resmed_signals: &[],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.timax"),
        label: "Maximum inspiratory time",
        kind: ChannelKind::Setting,
        unit: Unit::Seconds,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe216),
            cpp_symbol: "RMAS1x_TiMax",
            lookup_code: "RMAS1x_TiMax",
            english_label: "TiMax",
            short_label: "TiMax",
            unit_label: "Seconds",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.TiMax", "S.S.TiMax", "S.VA.TiMax"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.timin"),
        label: "Minimum inspiratory time",
        kind: ChannelKind::Setting,
        unit: Unit::Seconds,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe217),
            cpp_symbol: "RMAS1x_TiMin",
            lookup_code: "RMAS1x_TiMin",
            english_label: "TiMin",
            short_label: "TiMin",
            unit_label: "Seconds",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.TiMin", "S.S.TiMin", "S.VA.TiMin"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.setting.resmed.trigger"),
        label: "Trigger sensitivity",
        kind: ChannelKind::Setting,
        unit: Unit::Unspecified,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe215),
            cpp_symbol: "RMAS1x_Trigger",
            lookup_code: "RMAS1x_Trigger",
            english_label: "Trigger",
            short_label: "Trigger",
            unit_label: "",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Str,
            aliases: &["S.Trigger", "S.S.Trigger", "S.VA.Trigger"],
        }],
        event_semantics: None,
        span_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.span.cheyne_stokes_respiration"),
        label: "Cheyne Stokes respiration",
        kind: ChannelKind::Span,
        unit: Unit::Percent,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0x1000),
            cpp_symbol: "CPAP_CSR",
            lookup_code: "CSR",
            english_label: "Cheyne Stokes Respiration (CSR)",
            short_label: "CSR",
            unit_label: "%",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Csl,
            aliases: &["CSR Start", "CSR End"],
        }],
        event_semantics: None,
        span_semantics: Some(RESMED_CSL_CSR_SPAN),
        analytics_role: None,
    },
];

/// Look up a channel by its exact, case-sensitive stable OPAP key.
///
/// Returns `None` if no entry or more than one entry matches.
#[must_use]
pub fn by_stable_key(key: &str) -> Option<&'static ChannelDefinition> {
    let mut matches = CHANNELS
        .iter()
        .filter(|channel| channel.key.as_str() == key);
    let channel = matches.next()?;
    matches.next().is_none().then_some(channel)
}

/// Look up a channel by its typed legacy OSCAR numeric ID.
///
/// Returns `None` if no entry or more than one entry matches.
#[must_use]
pub fn by_legacy_id(id: LegacyOscarChannelId) -> Option<&'static ChannelDefinition> {
    let mut matches = CHANNELS
        .iter()
        .filter(|channel| channel.legacy_oscar.id == id);
    let channel = matches.next()?;
    matches.next().is_none().then_some(channel)
}

/// Look up a channel by a raw legacy OSCAR numeric ID.
///
/// Prefer [`by_legacy_id`] in typed domain code. This convenience function is
/// intended for database and import boundaries that receive raw integers.
#[must_use]
pub fn by_legacy_numeric_id(id: u32) -> Option<&'static ChannelDefinition> {
    by_legacy_id(LegacyOscarChannelId(id))
}

/// Resolve an exact, case-sensitive `ResMed` signal or annotation label.
///
/// File-family scoping is mandatory because `Mask Pres` intentionally maps to
/// different pressure channels in BRP and PLD files in the pinned loader.
/// Unlike OSCAR's permissive case-insensitive prefix matcher, this canonical
/// metadata lookup accepts only a complete alias. It returns `None` if no entry
/// or more than one entry matches.
#[must_use]
pub fn resmed_signal(file: ResmedFileKind, label: &str) -> Option<&'static ChannelDefinition> {
    let mut matches = CHANNELS.iter().filter(|channel| {
        channel
            .resmed_signals
            .iter()
            .any(|signal| signal.file == file && signal.aliases.contains(&label))
    });
    let channel = matches.next()?;
    matches.next().is_none().then_some(channel)
}

/// Resolve the start/end role of an exact `ResMed` span annotation.
///
/// This first applies the same strict, file-scoped channel resolution as
/// [`resmed_signal`], then requires exactly one endpoint declaration on that
/// channel. Non-span signals and ambiguous endpoint metadata return `None`.
#[must_use]
pub fn resmed_span_endpoint_role(file: ResmedFileKind, label: &str) -> Option<SpanEndpointRole> {
    let channel = resmed_signal(file, label)?;
    let semantics = channel.span_semantics?;
    let mut matches = semantics
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.file == file && endpoint.alias == label);
    let endpoint = matches.next()?;
    matches.next().is_none().then_some(endpoint.role)
}

/// Resolve a `ResMed` signal or annotation label using OSCAR's permissive
/// label-starts-with-alias direction and case-insensitive comparison.
///
/// File-family scoping remains mandatory. Multiple matching aliases belonging
/// to one channel count as one match, while labels matching aliases from more
/// than one channel fail closed and return `None`.
///
/// Matching is locale-independent and performs Unicode lowercase comparison
/// without normalization. This reproduces OSCAR's behavior for the ASCII labels
/// used by its BRP/EVE paths and gives deterministic casing behavior for the
/// registry's non-ASCII aliases. It does not claim equivalence with every
/// version-specific edge case of Qt's Unicode case folding.
#[must_use]
pub fn resmed_signal_prefix(
    file: ResmedFileKind,
    label: &str,
) -> Option<&'static ChannelDefinition> {
    let mut matches = CHANNELS.iter().filter(|channel| {
        channel.resmed_signals.iter().any(|signal| {
            signal.file == file
                && signal
                    .aliases
                    .iter()
                    .any(|alias| starts_with_case_insensitive(label, alias))
        })
    });
    let channel = matches.next()?;
    matches.next().is_none().then_some(channel)
}

fn starts_with_case_insensitive(value: &str, prefix: &str) -> bool {
    let mut value_lowercase = value.chars().flat_map(char::to_lowercase);
    prefix
        .chars()
        .flat_map(char::to_lowercase)
        .all(|expected| value_lowercase.next() == Some(expected))
}
