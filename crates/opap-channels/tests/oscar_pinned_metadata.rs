use opap_channels::{
    AnalyticsRole, CHANNELS, ChannelKind, EventPayload, EventSemantics, EventTimestamp,
    ResmedFileKind, SpanEndpointRole, SpanEndpointTimestamp, SpanPayload, Unit,
    by_legacy_numeric_id, by_stable_key, resmed_signal, resmed_span_endpoint_role,
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
        &[
            (
                ResmedFileKind::Pld,
                &[
                    "Exp Pres",
                    "EprPress.2s",
                    "EPAP",
                    "S.BL.EPAP",
                    "EPRPress.2s",
                    "S.S.EPAP"
                ]
            ),
            (
                ResmedFileKind::Str,
                &["Exp Pres", "EPAP", "S.BL.EPAP", "S.S.EPAP"]
            )
        ],
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
        &[
            (
                ResmedFileKind::Pld,
                &["Insp Pres", "IPAP", "S.BL.IPAP", "S.S.IPAP"]
            ),
            (
                ResmedFileKind::Str,
                &["Insp Pres", "IPAP", "S.BL.IPAP", "S.S.IPAP"]
            )
        ],
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
    expected_channel!(
        "pap.setting.epap_maximum",
        "Maximum expiratory pressure",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0x111d,
        "CPAP_EPAPHi",
        "EPAPHi",
        "Max EPAP",
        "Max EPAP",
        "cmH2O",
        &[(
            ResmedFileKind::Str,
            &["Max EPAP", "S.i.MaxEPAP", "S.AA.MaxEPAP"]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.epap_minimum",
        "Minimum expiratory pressure",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0x111c,
        "CPAP_EPAPLo",
        "EPAPLo",
        "Min EPAP",
        "Min EPAP",
        "cmH2O",
        &[(
            ResmedFileKind::Str,
            &["Min EPAP", "S.VA.MinEPAP", "S.i.MinEPAP", "S.AA.MinEPAP"]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.ipap_maximum",
        "Maximum inspiratory pressure",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0x1111,
        "CPAP_IPAPHi",
        "IPAPHi",
        "Max IPAP",
        "Max IPAP",
        "cmH2O",
        &[(ResmedFileKind::Str, &["Max IPAP", "S.VA.MaxIPAP"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.ipap_minimum",
        "Minimum inspiratory pressure",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0x1110,
        "CPAP_IPAPLo",
        "IPAPLo",
        "Min IPAP",
        "Min IPAP",
        "cmH2O",
        &[(ResmedFileKind::Str, &["Min IPAP"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.maximum_pressure",
        "Maximum therapy pressure",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0x1021,
        "CPAP_PressureMax",
        "PressureMax",
        "Max Pressure",
        "Pressure Max",
        "cmH2O",
        &[(
            ResmedFileKind::Str,
            &[
                "Max Pressure",
                "Max. Druck",
                "Max druk",
                "最大压力",
                "Pression max.",
                "Max tryck",
                "S.AS.MaxPress",
                "S.A.MaxPress",
                "Azami Basınç",
                "S.AFH.MaxPress"
            ]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.minimum_pressure",
        "Minimum therapy pressure",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0x1020,
        "CPAP_PressureMin",
        "PressureMin",
        "Min Pressure",
        "Pressure Min",
        "cmH2O",
        &[(
            ResmedFileKind::Str,
            &[
                "Min Pressure",
                "Min. Druck",
                "Min druk",
                "最小压力",
                "Pression min.",
                "Min tryck",
                "S.AS.MinPress",
                "S.A.MinPress",
                "Min Basınç",
                "S.AFH.MinPress"
            ]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.pap_mode",
        "PAP mode",
        ChannelKind::Setting,
        Unit::Unspecified,
        0x1200,
        "CPAP_Mode",
        "PAPMode",
        "PAP Mode",
        "PAP Mode",
        "",
        &[(
            ResmedFileKind::Str,
            &["Mode", "Modus", "Funktion", "模式", "Mod"]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.pressure_support",
        "Pressure support",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0x110f,
        "CPAP_PS",
        "PS",
        "PS",
        "PS",
        "cmH2O",
        &[(ResmedFileKind::Str, &["PS", "S.VA.PS"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.pressure_support_maximum",
        "Maximum pressure support",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0x111b,
        "CPAP_PSMax",
        "PSMax",
        "PS Max",
        "PS Max",
        "cmH2O",
        &[(
            ResmedFileKind::Str,
            &["Max PS", "S.i.MaxPS", "S.AV.MaxPS", "S.AA.MaxPS"]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.pressure_support_minimum",
        "Minimum pressure support",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0x111a,
        "CPAP_PSMin",
        "PSMin",
        "PS Min",
        "PS Min",
        "cmH2O",
        &[(
            ResmedFileKind::Str,
            &["Min PS", "S.i.MinPS", "S.AV.MinPS", "S.AA.MinPS"]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.ramp_pressure",
        "Ramp pressure",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0x1023,
        "CPAP_RampPressure",
        "RampPressure",
        "Ramp Pressure",
        "Ramp Pressure",
        "cmH2O",
        &[(
            ResmedFileKind::Str,
            &[
                "S.C.StartPress",
                "S.AS.StartPress",
                "S.A.StartPress",
                "S.AFH.StartPress",
                "S.BL.StartPress",
                "S.VA.StartPress",
                "S.i.StartPress",
                "S.AV.StartPress",
                "S.AA.StartPress"
            ]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.ramp_time",
        "Ramp time",
        ChannelKind::Setting,
        Unit::Minutes,
        0x1022,
        "CPAP_RampTime",
        "RampTime",
        "Ramp Time",
        "Ramp Time",
        "Minutes",
        &[(ResmedFileKind::Str, &["S.RampTime"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.antibacterial_filter",
        "Antibacterial filter",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe209,
        "RMS9_ABFilter",
        "RMS9_ABFilter",
        "AB Filter",
        "Antibacterial Filter",
        "",
        &[(ResmedFileKind::Str, &["S.ABFilter"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.climate_control",
        "Climate control",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe20b,
        "RMS9_ClimateControl",
        "RMS9_ClimateControl",
        "Climate Control",
        "Climate Control",
        "",
        &[(ResmedFileKind::Str, &["S.ClimateControl"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.comfort_response",
        "Comfort response",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe20e,
        "RMAS1x_Comfort",
        "RMAS1x_Comfort",
        "Response",
        "Response",
        "",
        &[(ResmedFileKind::Str, &["S.AS.Comfort"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.cycle",
        "Cycle sensitivity",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe214,
        "RMAS1x_Cycle",
        "RMAS1x_Cycle",
        "Cycle",
        "Cycle",
        "",
        &[(ResmedFileKind::Str, &["S.Cycle", "S.S.Cycle", "S.VA.Cycle"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.epr",
        "Expiratory pressure relief",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe201,
        "RMS9_EPR",
        "EPR",
        "EPR",
        "EPR",
        "",
        &[(ResmedFileKind::Str, &["EPR", "呼气释压(EP"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.epr_level",
        "Expiratory pressure relief level",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0xe202,
        "RMS9_EPRLevel",
        "EPRLevel",
        "EPR Level",
        "EPR Level",
        "cmH2O",
        &[(
            ResmedFileKind::Str,
            &[
                "EPR Level",
                "EPR-Stufe",
                "EPR-niveau",
                "EPR 水平",
                "Niveau EPR",
                "EPR-nivå",
                "EPR-nivÃ¥",
                "S.EPR.Level",
                "EPR Düzeyi"
            ]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.humidifier_enabled",
        "Humidifier enabled",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe205,
        "RMS9_HumidStatus",
        "RMS9_HumidStat",
        "Humid. Status",
        "Humidifier Status",
        "",
        &[(ResmedFileKind::Str, &["S.HumEnable"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.humidity_level",
        "Humidity level",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe206,
        "RMS9_HumidLevel",
        "RMS9_HumidLevel",
        "Humid. Level",
        "Humidity Level",
        "",
        &[(ResmedFileKind::Str, &["S.HumLevel"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.mask_type",
        "Mask type",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe20c,
        "RMS9_Mask",
        "RMS9_Mask",
        "Mask",
        "Mask",
        "",
        &[(ResmedFileKind::Str, &["S.Mask"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.patient_access",
        "Patient access",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe20a,
        "RMS9_PtAccess",
        "RMS9_PTAccess",
        "Pt. Access",
        "Essentials",
        "",
        &[],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.patient_view",
        "Patient view",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe210,
        "RMAS11_PtView",
        "RMAS11_PTView",
        "Patient View",
        "Patient View",
        "",
        &[],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.ramp_enabled",
        "Ramp enabled",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe20d,
        "RMS9_RampEnable",
        "RMS9_RampEnable",
        "Ramp",
        "Ramp",
        "",
        &[(ResmedFileKind::Str, &["S.RampEnable"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.rise_enabled",
        "Rise enabled",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe212,
        "RMAS1x_RiseEnable",
        "RMAS1x_RiseEnable",
        "RiseEnable",
        "RiseEnable",
        "",
        &[(ResmedFileKind::Str, &["S.RiseEnable", "S.S.RiseEnable"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.rise_time",
        "Rise time",
        ChannelKind::Setting,
        Unit::Milliseconds,
        0xe213,
        "RMAS1x_RiseTime",
        "RMAS1x_RiseTime",
        "RiseTime",
        "RiseTime",
        "milliSeconds",
        &[(ResmedFileKind::Str, &["S.RiseTime", "S.S.RiseTime"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.set_pressure",
        "Set pressure",
        ChannelKind::Setting,
        Unit::CentimetersOfWater,
        0x1162,
        "RMS9_SetPressure",
        "SetPressure",
        "Set Pressure",
        "Pressure",
        "",
        &[(
            ResmedFileKind::Str,
            &[
                "Set Pressure",
                "Eingest. Druck",
                "Ingestelde druk",
                "设定压力",
                "Pres. prescrite",
                "Inställt tryck",
                "InstÃ¤llt tryck",
                "S.C.Press",
                "Basıncı Ayarl"
            ]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.smart_start",
        "SmartStart",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe204,
        "RMS9_SmartStart",
        "RMS9_SmartStart",
        "SmartStart",
        "Smart Start",
        "",
        &[(ResmedFileKind::Str, &["S.SmartStart"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.smart_stop",
        "SmartStop",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe20f,
        "RMAS11_SmartStop",
        "RMAS11_SmartStop",
        "SmartStop",
        "Smart Stop",
        "",
        &[(ResmedFileKind::Str, &["S.SmartStop"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.temperature",
        "ClimateLine temperature",
        ChannelKind::Setting,
        Unit::DegreesCelsius,
        0xe207,
        "RMS9_Temp",
        "RMS9_Temp",
        "Temperature",
        "Temperature",
        "ºC",
        &[(ResmedFileKind::Str, &["S.Temp"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.temperature_enabled",
        "ClimateLine temperature enabled",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe208,
        "RMS9_TempEnable",
        "RMS9_TempEnable",
        "Temp. Enable",
        "Temperature Enable",
        "",
        &[(ResmedFileKind::Str, &["S.TempEnable"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.therapy_mode",
        "ResMed therapy mode",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe203,
        "RMS9_Mode",
        "RMS9_Mode",
        "Mode",
        "Mode",
        "",
        &[],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.timax",
        "Maximum inspiratory time",
        ChannelKind::Setting,
        Unit::Seconds,
        0xe216,
        "RMAS1x_TiMax",
        "RMAS1x_TiMax",
        "TiMax",
        "TiMax",
        "Seconds",
        &[(ResmedFileKind::Str, &["S.TiMax", "S.S.TiMax", "S.VA.TiMax"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.timin",
        "Minimum inspiratory time",
        ChannelKind::Setting,
        Unit::Seconds,
        0xe217,
        "RMAS1x_TiMin",
        "RMAS1x_TiMin",
        "TiMin",
        "TiMin",
        "Seconds",
        &[(ResmedFileKind::Str, &["S.TiMin", "S.S.TiMin", "S.VA.TiMin"])],
        None,
        None
    ),
    expected_channel!(
        "pap.setting.resmed.trigger",
        "Trigger sensitivity",
        ChannelKind::Setting,
        Unit::Unspecified,
        0xe215,
        "RMAS1x_Trigger",
        "RMAS1x_Trigger",
        "Trigger",
        "Trigger",
        "",
        &[(
            ResmedFileKind::Str,
            &["S.Trigger", "S.S.Trigger", "S.VA.Trigger"]
        )],
        None,
        None
    ),
    expected_channel!(
        "pap.span.cheyne_stokes_respiration",
        "Cheyne Stokes respiration",
        ChannelKind::Span,
        Unit::Percent,
        0x1000,
        "CPAP_CSR",
        "CSR",
        "Cheyne Stokes Respiration (CSR)",
        "CSR",
        "%",
        &[(ResmedFileKind::Csl, &["CSR Start", "CSR End"])],
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
        (ResmedFileKind::Pld, "S.BL.EPAP", "pap.series.epap"),
        (ResmedFileKind::Pld, "S.S.IPAP", "pap.series.ipap"),
        (ResmedFileKind::Str, "S.BL.EPAP", "pap.series.epap"),
        (ResmedFileKind::Str, "S.S.IPAP", "pap.series.ipap"),
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
fn csl_csr_metadata_and_endpoint_storage_match_pinned_loader() {
    let csr = by_stable_key("pap.span.cheyne_stokes_respiration").expect("CSR channel");
    assert_eq!(csr.kind, ChannelKind::Span);
    assert_eq!(csr.unit, Unit::Percent);
    assert_eq!(csr.legacy_oscar.id.get(), 0x1000);
    assert_eq!(csr.legacy_oscar.cpp_symbol, "CPAP_CSR");
    assert_eq!(csr.legacy_oscar.lookup_code, "CSR");
    assert_eq!(
        csr.legacy_oscar.english_label,
        "Cheyne Stokes Respiration (CSR)"
    );
    assert_eq!(csr.legacy_oscar.short_label, "CSR");
    assert_eq!(csr.legacy_oscar.unit_label, "%");

    let semantics = csr.span_semantics.expect("CSR span semantics");
    assert_eq!(
        semantics.endpoint_timestamp,
        SpanEndpointTimestamp::ResmedEdfAnnotationOnset
    );
    assert_eq!(semantics.stored_timestamp, SpanEndpointRole::End);
    assert_eq!(
        semantics.payload,
        SpanPayload::ElapsedSecondsBetweenEndpoints
    );
    assert_eq!(semantics.endpoints.len(), 2);
    assert_eq!(
        resmed_span_endpoint_role(ResmedFileKind::Csl, "CSR Start"),
        Some(SpanEndpointRole::Start)
    );
    assert_eq!(
        resmed_span_endpoint_role(ResmedFileKind::Csl, "CSR End"),
        Some(SpanEndpointRole::End)
    );
}

#[test]
fn resmed_setting_ids_are_pinned_without_inventing_easy_breathe() {
    let cases = [
        (0xe201, "RMS9_EPR"),
        (0xe202, "RMS9_EPRLevel"),
        (0xe203, "RMS9_Mode"),
        (0xe204, "RMS9_SmartStart"),
        (0xe205, "RMS9_HumidStatus"),
        (0xe206, "RMS9_HumidLevel"),
        (0xe207, "RMS9_Temp"),
        (0xe208, "RMS9_TempEnable"),
        (0xe209, "RMS9_ABFilter"),
        (0xe20a, "RMS9_PtAccess"),
        (0xe20b, "RMS9_ClimateControl"),
        (0xe20c, "RMS9_Mask"),
        (0xe20d, "RMS9_RampEnable"),
        (0xe20e, "RMAS1x_Comfort"),
        (0xe20f, "RMAS11_SmartStop"),
        (0xe210, "RMAS11_PtView"),
        (0xe212, "RMAS1x_RiseEnable"),
        (0xe213, "RMAS1x_RiseTime"),
        (0xe214, "RMAS1x_Cycle"),
        (0xe215, "RMAS1x_Trigger"),
        (0xe216, "RMAS1x_TiMax"),
        (0xe217, "RMAS1x_TiMin"),
    ];

    for (id, symbol) in cases {
        assert_eq!(
            by_legacy_numeric_id(id)
                .expect("pinned ResMed setting ID")
                .legacy_oscar
                .cpp_symbol,
            symbol
        );
    }

    assert_eq!(by_legacy_numeric_id(0xe211), None);
    assert!(
        CHANNELS
            .iter()
            .all(|channel| channel.legacy_oscar.cpp_symbol != "RMAS1x_EasyBreathe")
    );
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
