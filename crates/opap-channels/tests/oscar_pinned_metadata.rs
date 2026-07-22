use opap_channels::{
    AnalyticsRole, CHANNELS, ChannelKind, EventPayload, EventSemantics, EventTimestamp,
    ResmedFileKind, Unit, by_stable_key, resmed_signal,
};

// The legacy fields and ResMed aliases in this hand-maintained metadata
// snapshot were transcribed from OSCAR-code commit
// 64c5e90a26f91fb15868bcfcccde0c1e1522ac86. This is a source-derived
// regression check, not an executable OSCAR fixture or an end-to-end parity
// claim. OPAP-created keys, labels, kinds, and roles are locked alongside the
// source-derived fields. See OSCAR_PROVENANCE.md.
#[derive(Debug)]
struct ExpectedChannel {
    key: &'static str,
    opap_label: &'static str,
    kind: ChannelKind,
    unit: Unit,
    id: u32,
    cpp_symbol: &'static str,
    lookup_code: &'static str,
    english_label: &'static str,
    short_label: &'static str,
    unit_label: &'static str,
    signals: &'static [(ResmedFileKind, &'static [&'static str])],
    event_semantics: Option<EventSemantics>,
    analytics_role: Option<AnalyticsRole>,
}

macro_rules! expected_channel {
    (
        $key:expr, $opap_label:expr, $kind:expr, $unit:expr,
        $id:expr, $cpp_symbol:expr, $lookup_code:expr,
        $english_label:expr, $short_label:expr, $unit_label:expr,
        $signals:expr, $event_semantics:expr, $analytics_role:expr
    ) => {
        ExpectedChannel {
            key: $key,
            opap_label: $opap_label,
            kind: $kind,
            unit: $unit,
            id: $id,
            cpp_symbol: $cpp_symbol,
            lookup_code: $lookup_code,
            english_label: $english_label,
            short_label: $short_label,
            unit_label: $unit_label,
            signals: $signals,
            event_semantics: $event_semantics,
            analytics_role: $analytics_role,
        }
    };
}

const EVE_EVENT: Option<EventSemantics> = Some(EventSemantics {
    timestamp: EventTimestamp::ResmedEdfAnnotationOnset,
    payload: EventPayload::ResmedEdfAnnotationDurationSecondsOrMissing,
    count_each_record: true,
});
const LOADER_EVENT: Option<EventSemantics> = Some(EventSemantics {
    timestamp: EventTimestamp::LoaderDefined,
    payload: EventPayload::LoaderDefined,
    count_each_record: true,
});

const EXPECTED_CHANNELS: &[ExpectedChannel] = &[
    expected_channel!(
        "pap.event.clear_airway",
        "Clear airway",
        ChannelKind::Event,
        Unit::EventsPerHour,
        0x1001,
        "CPAP_ClearAirway",
        "ClearAirway",
        "Clear Airway (CA)",
        "CA",
        "Events/hr",
        &[(ResmedFileKind::Eve, &["Central apnea"])],
        EVE_EVENT,
        Some(AnalyticsRole::AhiEventCount)
    ),
    expected_channel!(
        "pap.event.device_reported_apnea",
        "Device-reported apnea",
        ChannelKind::Event,
        Unit::EventsPerHour,
        0x1010,
        "CPAP_AllApnea",
        "AllApnea",
        "Apnea (A)",
        "A",
        "Events/hr",
        &[],
        LOADER_EVENT,
        Some(AnalyticsRole::AhiEventCount)
    ),
    expected_channel!(
        "pap.event.hypopnea",
        "Hypopnea",
        ChannelKind::Event,
        Unit::EventsPerHour,
        0x1003,
        "CPAP_Hypopnea",
        "Hypopnea",
        "Hypopnea (H)",
        "H",
        "Events/hr",
        &[(ResmedFileKind::Eve, &["Hypopnea"])],
        EVE_EVENT,
        Some(AnalyticsRole::AhiEventCount)
    ),
    expected_channel!(
        "pap.event.obstructive_apnea",
        "Obstructive apnea",
        ChannelKind::Event,
        Unit::EventsPerHour,
        0x1002,
        "CPAP_Obstructive",
        "Obstructive",
        "Obstructive Apnea (OA)",
        "OA",
        "Events/hr",
        &[(ResmedFileKind::Eve, &["Obstructive apnea"])],
        EVE_EVENT,
        Some(AnalyticsRole::AhiEventCount)
    ),
    expected_channel!(
        "pap.event.rera",
        "RERA",
        ChannelKind::Event,
        Unit::EventsPerHour,
        0x1006,
        "CPAP_RERA",
        "RERA",
        "RERA (RE)",
        "RE",
        "Events/hr",
        &[(ResmedFileKind::Eve, &["Arousal"])],
        EVE_EVENT,
        Some(AnalyticsRole::RdiAdditionalEventCount)
    ),
    expected_channel!(
        "pap.event.unclassified_apnea",
        "Unclassified apnea",
        ChannelKind::Event,
        Unit::EventsPerHour,
        0x1004,
        "CPAP_Apnea",
        "Apnea",
        "Unclassified Apnea (UA)",
        "UA",
        "Events/hr",
        &[(ResmedFileKind::Eve, &["Apnea"])],
        EVE_EVENT,
        Some(AnalyticsRole::AhiEventCount)
    ),
    expected_channel!(
        "pap.series.epap",
        "Expiratory pressure",
        ChannelKind::SampledSeries,
        Unit::CentimetersOfWater,
        0x110e,
        "CPAP_EPAP",
        "EPAP",
        "EPAP",
        "EPAP",
        "cmH2O",
        &[(
            ResmedFileKind::Pld,
            &[
                "Exp Pres",
                "EprPress.2s",
                "EPAP",
                "S.BL.EPAP",
                "EPRPress.2s",
                "S.S.EPAP"
            ]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.series.expiratory_time",
        "Expiratory time",
        ChannelKind::SampledSeries,
        Unit::Seconds,
        0x110a,
        "CPAP_Te",
        "Te",
        "Expiratory Time",
        "Exp. Time",
        "Seconds",
        &[(ResmedFileKind::Pld, &["Te", "B5ETime.2s"])],
        None,
        None
    ),
    expected_channel!(
        "pap.series.flow_limitation",
        "Flow limitation",
        ChannelKind::SampledSeries,
        Unit::SeverityZeroToOne,
        0x1113,
        "CPAP_FLG",
        "FLG",
        "Flow Limitation",
        "Flow Limit.",
        "Severity (0-1)",
        &[(ResmedFileKind::Pld, &["FFL Index", "FlowLim.2s"])],
        None,
        None
    ),
    expected_channel!(
        "pap.series.flow_rate",
        "Flow rate",
        ChannelKind::SampledSeries,
        Unit::LitersPerMinute,
        0x1100,
        "CPAP_FlowRate",
        "FlowRate",
        "Flow Rate",
        "Flow Rate",
        "l/min",
        &[(ResmedFileKind::Brp, &["Flow", "Flow.40ms"])],
        None,
        None
    ),
    expected_channel!(
        "pap.series.inspiratory_time",
        "Inspiratory time",
        ChannelKind::SampledSeries,
        Unit::Seconds,
        0x110b,
        "CPAP_Ti",
        "Ti",
        "Inspiratory Time",
        "Insp. Time",
        "Seconds",
        &[(ResmedFileKind::Pld, &["Ti", "B5ITime.2s"])],
        None,
        None
    ),
    expected_channel!(
        "pap.series.ipap",
        "Inspiratory pressure",
        ChannelKind::SampledSeries,
        Unit::CentimetersOfWater,
        0x110d,
        "CPAP_IPAP",
        "IPAP",
        "IPAP",
        "IPAP",
        "cmH2O",
        &[(
            ResmedFileKind::Pld,
            &["Insp Pres", "IPAP", "S.BL.IPAP", "S.S.IPAP"]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.series.leak_rate",
        "Leak rate",
        ChannelKind::SampledSeries,
        Unit::LitersPerMinute,
        0x1108,
        "CPAP_Leak",
        "Leak",
        "Leak Rate",
        "Leak Rate",
        "l/min",
        &[(
            ResmedFileKind::Pld,
            &[
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
                "Sızıntı"
            ]
        )],
        None,
        Some(AnalyticsRole::LeakSummary)
    ),
    expected_channel!(
        "pap.series.mask_pressure",
        "Mask pressure",
        ChannelKind::SampledSeries,
        Unit::CentimetersOfWater,
        0x1101,
        "CPAP_MaskPressure",
        "MaskPressure",
        "Mask Pressure",
        "Mask Pressure",
        "cmH2O",
        &[(ResmedFileKind::Pld, &["Mask Pres", "MaskPress.2s"])],
        None,
        None
    ),
    expected_channel!(
        "pap.series.mask_pressure_high_rate",
        "Mask pressure (high rate)",
        ChannelKind::SampledSeries,
        Unit::CentimetersOfWater,
        0x1102,
        "CPAP_MaskPressureHi",
        "MaskPressureHi",
        "Mask Pressure",
        "Mask Pressure",
        "cmH2O",
        &[(ResmedFileKind::Brp, &["Mask Pres", "Press.40ms"])],
        None,
        None
    ),
    expected_channel!(
        "pap.series.minute_ventilation",
        "Minute ventilation",
        ChannelKind::SampledSeries,
        Unit::LitersPerMinute,
        0x1105,
        "CPAP_MinuteVent",
        "MinuteVent",
        "Minute Ventilation",
        "Minute Vent.",
        "l/min",
        &[(ResmedFileKind::Pld, &["MV", "VM", "MinVent.2s"])],
        None,
        None
    ),
    expected_channel!(
        "pap.series.respiratory_event",
        "Respiratory event signal",
        ChannelKind::SampledSeries,
        Unit::CentimetersOfWater,
        0x1112,
        "CPAP_RespEvent",
        "RespEvent",
        "Respiratory Event",
        "Resp. Event",
        "cmH2O",
        &[(ResmedFileKind::Brp, &["Resp Event", "TrigCycEvt.40ms"])],
        None,
        None
    ),
    expected_channel!(
        "pap.series.respiratory_rate",
        "Respiratory rate",
        ChannelKind::SampledSeries,
        Unit::BreathsPerMinute,
        0x1106,
        "CPAP_RespRate",
        "RespRate",
        "Respiratory Rate",
        "Resp. Rate",
        "Breaths/min",
        &[(ResmedFileKind::Pld, &["RR", "AF", "FR", "RespRate.2s"])],
        None,
        None
    ),
    expected_channel!(
        "pap.series.snore",
        "Snore",
        ChannelKind::SampledSeries,
        Unit::Unspecified,
        0x1104,
        "CPAP_Snore",
        "Snore",
        "Snore",
        "Snore",
        "?",
        &[(ResmedFileKind::Pld, &["Snore", "Snore.2s"])],
        None,
        None
    ),
    expected_channel!(
        "pap.series.target_minute_ventilation",
        "Target minute ventilation",
        ChannelKind::SampledSeries,
        Unit::LitersPerMinute,
        0x1114,
        "CPAP_TgMV",
        "TgMV",
        "Target Minute Ventilation",
        "Target Vent.",
        "l/min",
        &[(ResmedFileKind::Pld, &["TgMV", "TgtVent.2s"])],
        None,
        None
    ),
    expected_channel!(
        "pap.series.therapy_pressure",
        "Therapy pressure",
        ChannelKind::SampledSeries,
        Unit::CentimetersOfWater,
        0x110c,
        "CPAP_Pressure",
        "Pressure",
        "Pressure",
        "Pressure",
        "cmH2O",
        &[(ResmedFileKind::Pld, &["Therapy Pres", "Press.2s"])],
        None,
        Some(AnalyticsRole::PressureSummary)
    ),
    expected_channel!(
        "pap.series.tidal_volume",
        "Tidal volume",
        ChannelKind::SampledSeries,
        Unit::Milliliters,
        0x1103,
        "CPAP_TidalVolume",
        "TidalVolume",
        "Tidal Volume",
        "Tidal Volume",
        "ml",
        &[(ResmedFileKind::Pld, &["Vt", "VC", "TidVol.2s"])],
        None,
        None
    ),
];

#[test]
fn exhaustive_registry_matches_pinned_source_metadata_snapshot() {
    assert_eq!(CHANNELS.len(), EXPECTED_CHANNELS.len());

    for (channel, expected) in CHANNELS.iter().zip(EXPECTED_CHANNELS) {
        assert_eq!(channel.key.as_str(), expected.key);
        assert_eq!(channel.label, expected.opap_label, "{}", expected.key);
        assert_eq!(channel.kind, expected.kind, "{}", expected.key);
        assert_eq!(channel.unit, expected.unit, "{}", expected.key);
        assert_eq!(
            channel.legacy_oscar.id.get(),
            expected.id,
            "{}",
            expected.key
        );
        assert_eq!(
            channel.legacy_oscar.cpp_symbol, expected.cpp_symbol,
            "{}",
            expected.key
        );
        assert_eq!(
            channel.legacy_oscar.lookup_code, expected.lookup_code,
            "{}",
            expected.key
        );
        assert_eq!(
            channel.legacy_oscar.english_label, expected.english_label,
            "{}",
            expected.key
        );
        assert_eq!(
            channel.legacy_oscar.short_label, expected.short_label,
            "{}",
            expected.key
        );
        assert_eq!(
            channel.legacy_oscar.unit_label, expected.unit_label,
            "{}",
            expected.key
        );
        assert_eq!(
            channel.event_semantics, expected.event_semantics,
            "{}",
            expected.key
        );
        assert_eq!(
            channel.analytics_role, expected.analytics_role,
            "{}",
            expected.key
        );
        assert_eq!(
            channel.resmed_signals.len(),
            expected.signals.len(),
            "{}",
            expected.key
        );
        for (signal, (file, aliases)) in channel.resmed_signals.iter().zip(expected.signals) {
            assert_eq!(signal.file, *file, "{}", expected.key);
            assert_eq!(signal.aliases, *aliases, "{}", expected.key);
        }
    }
}

#[test]
fn resmed_eve_aliases_and_payload_semantics_match_loader() {
    let cases = [
        ("Obstructive apnea", "pap.event.obstructive_apnea"),
        ("Hypopnea", "pap.event.hypopnea"),
        ("Apnea", "pap.event.unclassified_apnea"),
        ("Arousal", "pap.event.rera"),
        ("Central apnea", "pap.event.clear_airway"),
    ];

    for (alias, key) in cases {
        let channel = resmed_signal(ResmedFileKind::Eve, alias).expect("pinned alias");
        assert_eq!(channel.key.as_str(), key);
        assert_eq!(channel.kind, ChannelKind::Event);
        assert_eq!(channel.unit, Unit::EventsPerHour);

        let semantics = channel.event_semantics.expect("EVE semantics");
        assert_eq!(
            semantics.timestamp,
            EventTimestamp::ResmedEdfAnnotationOnset
        );
        assert_eq!(
            semantics.payload,
            EventPayload::ResmedEdfAnnotationDurationSecondsOrMissing
        );
        assert!(semantics.count_each_record);
    }
}

#[test]
fn representative_brp_and_pld_aliases_match_source_translation_map() {
    let cases = [
        (ResmedFileKind::Brp, "Flow.40ms", "pap.series.flow_rate"),
        (
            ResmedFileKind::Brp,
            "Press.40ms",
            "pap.series.mask_pressure_high_rate",
        ),
        (
            ResmedFileKind::Brp,
            "TrigCycEvt.40ms",
            "pap.series.respiratory_event",
        ),
        (
            ResmedFileKind::Pld,
            "Press.2s",
            "pap.series.therapy_pressure",
        ),
        (ResmedFileKind::Pld, "EPRPress.2s", "pap.series.epap"),
        (ResmedFileKind::Pld, "TidVol.2s", "pap.series.tidal_volume"),
        (ResmedFileKind::Pld, "Leak.2s", "pap.series.leak_rate"),
        (
            ResmedFileKind::Pld,
            "FlowLim.2s",
            "pap.series.flow_limitation",
        ),
    ];

    for (file, alias, key) in cases {
        assert_eq!(
            resmed_signal(file, alias)
                .expect("pinned signal alias")
                .key
                .as_str(),
            key
        );
    }
}

#[test]
fn source_skipped_pld_signals_are_not_registered_as_channels() {
    for alias in ["AlvMinVent.2s", "CLRatio.2s", "TRRatio.2s"] {
        assert_eq!(resmed_signal(ResmedFileKind::Pld, alias), None, "{alias}");
    }
}

#[test]
fn analytics_roles_match_formula_inputs_without_thresholds() {
    let ahi_keys = [
        "pap.event.clear_airway",
        "pap.event.device_reported_apnea",
        "pap.event.hypopnea",
        "pap.event.obstructive_apnea",
        "pap.event.unclassified_apnea",
    ];
    for key in ahi_keys {
        assert_eq!(
            by_stable_key(key).expect("AHI channel").analytics_role,
            Some(AnalyticsRole::AhiEventCount)
        );
    }

    assert_eq!(
        by_stable_key("pap.event.rera")
            .expect("RERA channel")
            .analytics_role,
        Some(AnalyticsRole::RdiAdditionalEventCount)
    );
    assert_eq!(
        by_stable_key("pap.series.leak_rate")
            .expect("leak channel")
            .analytics_role,
        Some(AnalyticsRole::LeakSummary)
    );
    assert_eq!(
        by_stable_key("pap.series.therapy_pressure")
            .expect("pressure channel")
            .analytics_role,
        Some(AnalyticsRole::PressureSummary)
    );
}
