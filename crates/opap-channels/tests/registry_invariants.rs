use std::collections::BTreeSet;

use opap_channels::{
    CHANNELS, ChannelDto, ChannelKind, LegacyOscarChannelId, ResmedFileKind, SpanEndpointRole,
    Unit, by_legacy_id, by_legacy_numeric_id, by_stable_key, resmed_signal, resmed_signal_prefix,
    resmed_span_endpoint_role,
};

#[test]
fn stable_keys_and_legacy_ids_are_unique() {
    let mut keys = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut cpp_symbols = BTreeSet::new();
    let mut lookup_codes = BTreeSet::new();

    for channel in CHANNELS {
        let key = channel.key.as_str();
        assert!(!channel.label.is_empty());
        assert!(!channel.legacy_oscar.english_label.is_empty());
        assert!(!channel.legacy_oscar.short_label.is_empty());
        assert!(keys.insert(key), "duplicate stable key: {key}");
        assert!(
            ids.insert(channel.legacy_oscar.id),
            "duplicate OSCAR ID: {:#x}",
            channel.legacy_oscar.id.get()
        );
        assert!(
            cpp_symbols.insert(channel.legacy_oscar.cpp_symbol),
            "duplicate OSCAR C++ symbol: {}",
            channel.legacy_oscar.cpp_symbol
        );
        assert!(
            lookup_codes.insert(channel.legacy_oscar.lookup_code),
            "duplicate OSCAR lookup code: {}",
            channel.legacy_oscar.lookup_code
        );

        assert!(
            key.starts_with("pap.") || key.starts_with("oximetry."),
            "unexpected key namespace: {key}"
        );
        assert!(
            key.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_' || byte == b'.'),
            "unstable key grammar: {key}"
        );
        assert!(
            key.split('.').all(|segment| !segment.is_empty()),
            "empty key segment: {key}"
        );
    }

    let sorted: Vec<_> = CHANNELS
        .iter()
        .map(|channel| channel.key.as_str())
        .collect();
    let mut expected = sorted.clone();
    expected.sort_unstable();
    assert_eq!(sorted, expected, "registry must remain stable-key sorted");
}

#[test]
fn event_and_series_invariants_are_explicit() {
    for channel in CHANNELS {
        match channel.kind {
            ChannelKind::Event => {
                let semantics = channel
                    .event_semantics
                    .expect("every event requires explicit semantics");
                assert!(semantics.count_each_record);
                assert_eq!(channel.unit, Unit::EventsPerHour);
                assert_eq!(channel.span_semantics, None);
            }
            ChannelKind::SampledSeries | ChannelKind::Setting => {
                assert_eq!(channel.event_semantics, None);
                assert_eq!(channel.span_semantics, None);
                assert_ne!(channel.unit, Unit::EventsPerHour);
            }
            ChannelKind::Span => {
                assert_eq!(channel.event_semantics, None);
                let semantics = channel
                    .span_semantics
                    .expect("every span requires explicit semantics");
                assert!(!semantics.endpoints.is_empty());
            }
        }
    }
}

#[test]
fn aliases_are_unique_within_a_file_family() {
    let mut aliases = BTreeSet::new();

    for channel in CHANNELS {
        for signal in channel.resmed_signals {
            assert!(!signal.aliases.is_empty());
            for alias in signal.aliases {
                assert!(!alias.is_empty());
                assert!(
                    aliases.insert((signal.file, *alias)),
                    "ambiguous {:?} alias: {alias}",
                    signal.file
                );
                assert_eq!(
                    resmed_signal(signal.file, alias),
                    Some(channel),
                    "alias must resolve to its declaring channel"
                );
            }
        }
    }

    // OSCAR intentionally gives this text different meanings by file family.
    assert_eq!(
        resmed_signal(ResmedFileKind::Brp, "Mask Pres")
            .expect("BRP mapping")
            .key
            .as_str(),
        "pap.series.mask_pressure_high_rate"
    );
    assert_eq!(
        resmed_signal(ResmedFileKind::Pld, "Mask Pres")
            .expect("PLD mapping")
            .key
            .as_str(),
        "pap.series.mask_pressure"
    );
}

#[test]
fn sad_and_sa2_oximetry_mappings_are_equivalent_but_file_scoped() {
    let pulse = by_stable_key("oximetry.series.pulse_rate").expect("pulse channel");
    let oxygen =
        by_stable_key("oximetry.series.oxygen_saturation").expect("oxygen saturation channel");

    assert_eq!(pulse.unit, Unit::BeatsPerMinute);
    assert_eq!(pulse.unit.symbol(), "bpm");
    assert_eq!(oxygen.unit, Unit::Percent);

    for file in [ResmedFileKind::Sad, ResmedFileKind::Sa2] {
        for alias in ["Pulse", "Puls", "Pouls", "Pols", "Pulse.1s", "Nabiz"] {
            assert_eq!(resmed_signal(file, alias), Some(pulse), "{file:?} {alias}");
        }
        for alias in ["SpO2", "SpO2.1s"] {
            assert_eq!(resmed_signal(file, alias), Some(oxygen), "{file:?} {alias}");
        }
    }

    assert_eq!(resmed_signal(ResmedFileKind::Sad, "pulse"), None);
    assert_eq!(resmed_signal(ResmedFileKind::Sa2, "SpO2.1s extra"), None);
    assert_eq!(resmed_signal(ResmedFileKind::Pld, "Pulse"), None);
    assert_eq!(resmed_signal(ResmedFileKind::Brp, "SpO2"), None);
}

#[test]
fn source_coverage_allows_only_settings_and_one_analytics_exception_without_aliases() {
    let without_resmed_aliases: Vec<_> = CHANNELS
        .iter()
        .filter(|channel| channel.resmed_signals.is_empty() && channel.kind != ChannelKind::Setting)
        .map(|channel| channel.key.as_str())
        .collect();
    assert_eq!(without_resmed_aliases, ["pap.event.device_reported_apnea"]);

    let alias_free_settings: Vec<_> = CHANNELS
        .iter()
        .filter(|channel| channel.resmed_signals.is_empty() && channel.kind == ChannelKind::Setting)
        .map(|channel| channel.key.as_str())
        .collect();
    assert_eq!(
        alias_free_settings,
        [
            "pap.setting.resmed.patient_access",
            "pap.setting.resmed.patient_view",
            "pap.setting.resmed.therapy_mode",
        ]
    );
}

#[test]
fn lookup_boundaries_are_exact_and_fail_closed() {
    let obstructive = by_stable_key("pap.event.obstructive_apnea").expect("registered");
    assert_eq!(
        by_legacy_id(LegacyOscarChannelId(0x1002)),
        Some(obstructive)
    );
    assert_eq!(by_legacy_numeric_id(0x1002), Some(obstructive));

    assert_eq!(by_stable_key("PAP.EVENT.OBSTRUCTIVE_APNEA"), None);
    assert_eq!(by_stable_key("pap.event.obstructive-apnea"), None);
    assert_eq!(by_legacy_numeric_id(u32::MAX), None);
    assert_eq!(
        resmed_signal(ResmedFileKind::Eve, "obstructive apnea"),
        None
    );
    assert_eq!(resmed_signal(ResmedFileKind::Pld, "Leak.2s extra"), None);
    assert_eq!(resmed_signal(ResmedFileKind::Pld, "Flow.40ms"), None);
}

#[test]
fn prefix_lookup_matches_oscar_case_and_cropped_alias_policy() {
    let flow = by_stable_key("pap.series.flow_rate").expect("flow channel");
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Brp, "fLoW.40MS diagnostic suffix"),
        Some(flow)
    );

    let high_rate_pressure =
        by_stable_key("pap.series.mask_pressure_high_rate").expect("BRP pressure channel");
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Brp, "MASK PRESSURE waveform"),
        Some(high_rate_pressure)
    );

    let obstructive =
        by_stable_key("pap.event.obstructive_apnea").expect("obstructive event channel");
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Eve, "OBSTRUCTIVE APNEA (OA)"),
        Some(obstructive)
    );

    let ipap = by_stable_key("pap.series.ipap").expect("IPAP channel");
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Pld, "s.bl.ipap diagnostic suffix"),
        Some(ipap)
    );
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Str, "s.bl.ipap diagnostic suffix"),
        Some(ipap)
    );

    let pulse = by_stable_key("oximetry.series.pulse_rate").expect("pulse channel");
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Sad, "POULS waveform"),
        Some(pulse)
    );
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Sa2, "PULSE.1S waveform"),
        Some(pulse)
    );

    let oxygen =
        by_stable_key("oximetry.series.oxygen_saturation").expect("oxygen saturation channel");
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Sad, "SPO2.1S waveform"),
        Some(oxygen)
    );
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Sa2, "spo2 waveform"),
        Some(oxygen)
    );
}

#[test]
fn prefix_lookup_remains_file_scoped_and_fails_closed() {
    let high_rate_pressure =
        by_stable_key("pap.series.mask_pressure_high_rate").expect("BRP pressure channel");
    let regular_pressure = by_stable_key("pap.series.mask_pressure").expect("PLD pressure channel");
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Brp, "Mask Pressure samples"),
        Some(high_rate_pressure)
    );
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Pld, "Mask Pressure samples"),
        Some(regular_pressure)
    );

    // "TidVol.2s" is both an exact tidal-volume alias and starts with the
    // inspiratory-time alias "Ti", so an unordered registry lookup must not
    // reproduce OSCAR's loader-branch precedence implicitly.
    assert_eq!(resmed_signal_prefix(ResmedFileKind::Pld, "TidVol.2s"), None);
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Pld, "unknown signal"),
        None
    );

    // Exact STR aliases can still overlap under permissive prefix matching.
    // "EPR" and "S.Temp" must not steal more specific setting labels.
    assert_eq!(resmed_signal_prefix(ResmedFileKind::Str, "EPR Level"), None);
    assert_eq!(
        resmed_signal_prefix(ResmedFileKind::Str, "S.TempEnable"),
        None
    );
}

#[test]
fn csl_span_endpoints_are_exact_file_scoped_and_role_aware() {
    let csr = by_stable_key("pap.span.cheyne_stokes_respiration").expect("CSR span");
    assert_eq!(resmed_signal(ResmedFileKind::Csl, "CSR Start"), Some(csr));
    assert_eq!(resmed_signal(ResmedFileKind::Csl, "CSR End"), Some(csr));
    assert_eq!(
        resmed_span_endpoint_role(ResmedFileKind::Csl, "CSR Start"),
        Some(SpanEndpointRole::Start)
    );
    assert_eq!(
        resmed_span_endpoint_role(ResmedFileKind::Csl, "CSR End"),
        Some(SpanEndpointRole::End)
    );
    assert_eq!(resmed_signal(ResmedFileKind::Eve, "CSR Start"), None);
    assert_eq!(
        resmed_span_endpoint_role(ResmedFileKind::Csl, "csr start"),
        None
    );
    assert_eq!(
        resmed_span_endpoint_role(ResmedFileKind::Str, "S.RampTime"),
        None
    );
}

#[test]
fn str_settings_are_exact_and_do_not_leak_into_detailed_file_scopes() {
    let cases = [
        ("Min Pressure", "pap.setting.minimum_pressure"),
        ("S.RampTime", "pap.setting.ramp_time"),
        ("S.AA.StartPress", "pap.setting.ramp_pressure"),
        ("S.VA.MaxIPAP", "pap.setting.ipap_maximum"),
        ("S.i.MinEPAP", "pap.setting.epap_minimum"),
        ("S.AV.MaxPS", "pap.setting.pressure_support_maximum"),
        ("Mode", "pap.setting.pap_mode"),
        ("S.S.RiseTime", "pap.setting.resmed.rise_time"),
    ];

    for (alias, key) in cases {
        assert_eq!(
            resmed_signal(ResmedFileKind::Str, alias)
                .expect("STR setting alias")
                .key
                .as_str(),
            key
        );
        assert_eq!(resmed_signal(ResmedFileKind::Pld, alias), None, "{alias}");
        assert_eq!(resmed_signal(ResmedFileKind::Csl, alias), None, "{alias}");
    }
}

#[test]
fn every_exact_resmed_alias_resolves_without_ambiguity() {
    for channel in CHANNELS {
        for signal in channel.resmed_signals {
            for alias in signal.aliases {
                assert_eq!(
                    resmed_signal(signal.file, alias),
                    Some(channel),
                    "{:?} {alias}",
                    signal.file
                );
            }
        }
    }
}

#[test]
fn prefix_lookup_does_not_change_the_exact_resolver() {
    let flow = by_stable_key("pap.series.flow_rate").expect("flow channel");
    assert_eq!(resmed_signal(ResmedFileKind::Brp, "Flow.40ms"), Some(flow));
    assert_eq!(resmed_signal(ResmedFileKind::Brp, "flow.40ms"), None);
    assert_eq!(
        resmed_signal(ResmedFileKind::Brp, "Flow.40ms diagnostic suffix"),
        None
    );

    let tidal_volume = by_stable_key("pap.series.tidal_volume").expect("tidal-volume channel");
    assert_eq!(
        resmed_signal(ResmedFileKind::Pld, "TidVol.2s"),
        Some(tidal_volume)
    );
}

#[test]
fn every_registry_item_has_a_round_trippable_owned_dto() {
    for channel in CHANNELS {
        let dto = channel.to_dto();
        let encoded = serde_json::to_string(&dto).expect("serialize DTO");
        let decoded: ChannelDto = serde_json::from_str(&encoded).expect("deserialize DTO");
        assert_eq!(decoded, dto);
        assert_eq!(decoded.key, channel.key.as_str());
        assert_eq!(decoded.legacy_oscar.id, channel.legacy_oscar.id);
        assert_eq!(decoded.registered_definition(), Some(channel));
        assert!(decoded.is_canonical_snapshot());
    }
}

#[test]
fn deserialized_metadata_cannot_replace_the_canonical_registry() {
    let channel = by_stable_key("pap.series.leak_rate").expect("registered");
    let mut dto = channel.to_dto();
    dto.unit = Unit::Percent;
    dto.legacy_oscar.id = LegacyOscarChannelId(1);

    assert_eq!(dto.registered_definition(), Some(channel));
    assert!(!dto.is_canonical_snapshot());

    dto.key = String::from("pap.series.not_registered");
    assert_eq!(dto.registered_definition(), None);
    assert!(!dto.is_canonical_snapshot());
}
