// SPDX-License-Identifier: GPL-3.0-only
//
// The compatibility values below are derived from OSCAR-SQL at commit
// 3741e5b423e4b5796c51a9d447e83b2525963d50.
// Copyright (c) 2019-2026 The OSCAR Team
// Copyright (C) 2011-2018 Mark Watkins

use crate::{
    AnalyticsRole, ChannelDefinition, ChannelKind, EventPayload, EventSemantics, EventTimestamp,
    LegacyOscarChannelId, LegacyOscarMetadata, ResmedFileKind, ResmedSignalDescriptor,
    StableChannelKey, Unit,
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
        analytics_role: Some(AnalyticsRole::AhiEventCount),
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.alveolar_minute_ventilation"),
        label: "Alveolar minute ventilation",
        kind: ChannelKind::SampledSeries,
        unit: Unit::LitersPerMinute,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe218),
            cpp_symbol: "RMVENT_AlvMinVent",
            lookup_code: "RMVENT_AlvMinVent",
            english_label: "Alv. Min. Vent.",
            short_label: "Alv MV",
            unit_label: "L/min",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["AlvMinVent.2s"],
        }],
        event_semantics: None,
        analytics_role: None,
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
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &[
                "Exp Pres",
                "EprPress.2s",
                "EPAP",
                "S.BL.EPAP",
                "EPRPress.2s",
                "S.S.EPAP",
            ],
        }],
        event_semantics: None,
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
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["Insp Pres", "IPAP", "S.BL.IPAP", "S.S.IPAP"],
        }],
        event_semantics: None,
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
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.spontaneous_cycle_percent"),
        label: "Spontaneous cycle percentage",
        kind: ChannelKind::SampledSeries,
        unit: Unit::Percent,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe219),
            cpp_symbol: "RMVENT_SpontCyc",
            lookup_code: "RMVENT_SpontCyc",
            english_label: "Spont. Cycle%",
            short_label: "Spont Cyc%",
            unit_label: "%",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["CLRatio.2s"],
        }],
        event_semantics: None,
        analytics_role: None,
    },
    ChannelDefinition {
        key: StableChannelKey::new("pap.series.spontaneous_trigger_percent"),
        label: "Spontaneous trigger percentage",
        kind: ChannelKind::SampledSeries,
        unit: Unit::Percent,
        legacy_oscar: LegacyOscarMetadata {
            id: LegacyOscarChannelId(0xe21a),
            cpp_symbol: "RMVENT_SpontTrig",
            lookup_code: "RMVENT_SpontTrig",
            english_label: "Spont. Trig%",
            short_label: "Spont Trig%",
            unit_label: "%",
        },
        resmed_signals: &[ResmedSignalDescriptor {
            file: ResmedFileKind::Pld,
            aliases: &["TRRatio.2s"],
        }],
        event_semantics: None,
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
