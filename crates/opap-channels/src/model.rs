// SPDX-License-Identifier: GPL-3.0-only

use alloc::{string::String, vec::Vec};
use core::fmt;

use serde::{Deserialize, Serialize};

/// A stable, human-readable OPAP channel key.
///
/// These values are the primary identifiers persisted by OPAP. The wrapped
/// string is private so callers cannot manufacture a supposedly registered key
/// without going through the registry.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StableChannelKey(&'static str);

impl StableChannelKey {
    pub(crate) const fn new(value: &'static str) -> Self {
        Self(value)
    }

    /// Return the stable string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Debug for StableChannelKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("StableChannelKey")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for StableChannelKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Serialize for StableChannelKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0)
    }
}

/// An OSCAR numeric channel ID retained solely for compatibility.
///
/// OPAP must not assign new meanings to values in this namespace. Use
/// [`StableChannelKey`] as the durable identity of new records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LegacyOscarChannelId(pub u32);

impl LegacyOscarChannelId {
    /// Return the numeric compatibility value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// The storage shape of values associated with a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    /// One occurrence per imported device event record.
    Event,
    /// A time-ordered sampled or stepwise physical series.
    SampledSeries,
    /// A bounded interval represented by paired start and end records.
    Span,
    /// A session-level machine configuration value.
    Setting,
}

/// Canonical OPAP units after importer normalization.
///
/// For event channels, `EventsPerHour` is an aggregate/display unit. Consult
/// [`EventSemantics`] before interpreting an individual event payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Count of occurrences divided by active therapy hours.
    EventsPerHour,
    /// Heart beats per minute.
    BeatsPerMinute,
    /// Litres per minute.
    LitersPerMinute,
    /// Centimetres of water pressure.
    CentimetersOfWater,
    /// Millilitres.
    Milliliters,
    /// Breaths per minute.
    BreathsPerMinute,
    /// Seconds.
    Seconds,
    /// Minutes.
    Minutes,
    /// Milliseconds.
    Milliseconds,
    /// Degrees Celsius.
    DegreesCelsius,
    /// Percentage.
    Percent,
    /// A dimensionless ratio.
    Ratio,
    /// Device-provided flow-limitation severity on the OSCAR schema's 0–1 scale.
    SeverityZeroToOne,
    /// No evidence-backed physical unit is assigned.
    Unspecified,
}

impl Unit {
    /// Return a compact, non-localized display symbol.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::EventsPerHour => "events/h",
            Self::BeatsPerMinute => "bpm",
            Self::LitersPerMinute => "L/min",
            Self::CentimetersOfWater => "cmH2O",
            Self::Milliliters => "mL",
            Self::BreathsPerMinute => "breaths/min",
            Self::Seconds => "s",
            Self::Minutes => "min",
            Self::Milliseconds => "ms",
            Self::DegreesCelsius => "°C",
            Self::Percent => "%",
            Self::Ratio => "ratio",
            Self::SeverityZeroToOne => "0-1",
            Self::Unspecified => "",
        }
    }
}

/// `ResMed` file family in the supported registry scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResmedFileKind {
    /// Annotation events (`*_EVE.edf`).
    Eve,
    /// High-rate breathing/pressure data (`*_BRP.edf`).
    Brp,
    /// Low-rate detailed data (`*_PLD.edf`).
    Pld,
    /// Cheyne-Stokes respiration span annotations (`*_CSL.edf`).
    Csl,
    /// Daily summary and machine settings (`STR.edf`).
    Str,
    /// Oximetry detail (`*_SAD.edf`).
    Sad,
    /// Oximetry detail (`*_SA2.edf`).
    Sa2,
}

/// How a `ResMed` EVE event timestamp is represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventTimestamp {
    /// EDF recording start plus the annotation onset offset.
    ResmedEdfAnnotationOnset,
    /// Defined by the device loader; no `ResMed` EVE claim is made.
    LoaderDefined,
}

/// What the value attached to an individual event means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventPayload {
    /// Source duration in seconds, or the parser's `-1.0` missing sentinel.
    ResmedEdfAnnotationDurationSecondsOrMissing,
    /// Defined by the device loader; analytics count the occurrence only.
    LoaderDefined,
}

/// Evidence-backed semantics for an event channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSemantics {
    /// Meaning of the event timestamp.
    pub timestamp: EventTimestamp,
    /// Meaning of the event's attached numeric value.
    pub payload: EventPayload,
    /// Whether each imported record contributes one occurrence to event counts.
    pub count_each_record: bool,
}

/// How a paired span endpoint participates in the interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanEndpointRole {
    /// Opens a span.
    Start,
    /// Closes a span.
    End,
}

/// How a `ResMed` span endpoint timestamp is represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanEndpointTimestamp {
    /// EDF recording start plus the annotation onset offset.
    ResmedEdfAnnotationOnset,
}

/// What the numeric payload attached to a completed span means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanPayload {
    /// Elapsed seconds from the paired start endpoint to the end endpoint.
    ElapsedSecondsBetweenEndpoints,
}

/// One exact `ResMed` endpoint label and its role in a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResmedSpanEndpointDescriptor {
    /// File family in which the endpoint label has this meaning.
    pub file: ResmedFileKind,
    /// Exact annotation label accepted by the pinned loader.
    pub alias: &'static str,
    /// Whether this annotation opens or closes the span.
    pub role: SpanEndpointRole,
}

/// Evidence-backed semantics for a paired span channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SpanSemantics {
    /// Meaning of each endpoint timestamp.
    pub endpoint_timestamp: SpanEndpointTimestamp,
    /// Endpoint whose timestamp is stored for the completed span.
    pub stored_timestamp: SpanEndpointRole,
    /// Meaning of the completed span's numeric payload.
    pub payload: SpanPayload,
    /// Exact endpoint annotations accepted by the pinned loader.
    pub endpoints: &'static [ResmedSpanEndpointDescriptor],
}

/// A formula-level role intended for OPAP analytics integration.
///
/// These roles describe computation inputs only. They do not encode a clinical
/// threshold or interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsRole {
    /// One of OSCAR's five AHI-count inputs.
    AhiEventCount,
    /// RERA count added to AHI-event count for RDI.
    RdiAdditionalEventCount,
    /// Time-weighted leak summary input.
    LeakSummary,
    /// Time-weighted pressure summary input.
    PressureSummary,
}

/// Exact legacy OSCAR metadata from the pinned source revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LegacyOscarMetadata {
    /// OSCAR's numeric `ChannelID`.
    pub id: LegacyOscarChannelId,
    /// OSCAR's C++ global variable name.
    pub cpp_symbol: &'static str,
    /// OSCAR's untranslated schema lookup code.
    pub lookup_code: &'static str,
    /// OSCAR's default English full label.
    pub english_label: &'static str,
    /// OSCAR's default English short label.
    pub short_label: &'static str,
    /// OSCAR's default English unit label.
    pub unit_label: &'static str,
}

/// An exact `ResMed` signal/annotation alias set scoped to one file family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResmedSignalDescriptor {
    /// File family in which the aliases have this meaning.
    pub file: ResmedFileKind,
    /// Exact strings in OSCAR's translation table.
    ///
    /// OSCAR treats these as case-insensitive prefixes; OPAP's canonical
    /// registry resolver deliberately requires an exact, case-sensitive value.
    pub aliases: &'static [&'static str],
}

/// One immutable registry entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ChannelDefinition {
    /// Stable OPAP identity.
    pub key: StableChannelKey,
    /// Neutral OPAP display label.
    pub label: &'static str,
    /// Storage shape.
    pub kind: ChannelKind,
    /// Canonical OPAP unit after importer normalization.
    pub unit: Unit,
    /// Compatibility-only OSCAR metadata.
    pub legacy_oscar: LegacyOscarMetadata,
    /// `ResMed` aliases supported by the pinned detailed and STR code paths.
    pub resmed_signals: &'static [ResmedSignalDescriptor],
    /// Individual-record semantics for event channels.
    pub event_semantics: Option<EventSemantics>,
    /// Paired-endpoint semantics for span channels.
    pub span_semantics: Option<SpanSemantics>,
    /// Formula-level analytics use, if any.
    pub analytics_role: Option<AnalyticsRole>,
}

impl ChannelDefinition {
    /// Convert the static registry item to an owned serde DTO.
    #[must_use]
    pub fn to_dto(self) -> ChannelDto {
        ChannelDto {
            key: String::from(self.key.as_str()),
            label: String::from(self.label),
            kind: self.kind,
            unit: self.unit,
            legacy_oscar: self.legacy_oscar.into(),
            resmed_signals: self
                .resmed_signals
                .iter()
                .copied()
                .map(ResmedSignalDto::from)
                .collect(),
            event_semantics: self.event_semantics,
            span_semantics: self.span_semantics.map(SpanSemanticsDto::from),
            analytics_role: self.analytics_role,
        }
    }
}

/// Owned, round-trippable OSCAR compatibility DTO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyOscarMetadataDto {
    /// OSCAR's numeric `ChannelID`.
    pub id: LegacyOscarChannelId,
    /// OSCAR's C++ global variable name.
    pub cpp_symbol: String,
    /// OSCAR's untranslated schema lookup code.
    pub lookup_code: String,
    /// OSCAR's default English full label.
    pub english_label: String,
    /// OSCAR's default English short label.
    pub short_label: String,
    /// OSCAR's default English unit label.
    pub unit_label: String,
}

impl From<LegacyOscarMetadata> for LegacyOscarMetadataDto {
    fn from(metadata: LegacyOscarMetadata) -> Self {
        Self {
            id: metadata.id,
            cpp_symbol: String::from(metadata.cpp_symbol),
            lookup_code: String::from(metadata.lookup_code),
            english_label: String::from(metadata.english_label),
            short_label: String::from(metadata.short_label),
            unit_label: String::from(metadata.unit_label),
        }
    }
}

/// Owned, round-trippable `ResMed` alias DTO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResmedSignalDto {
    /// File family in which the aliases have this meaning.
    pub file: ResmedFileKind,
    /// Exact strings copied from OSCAR's translation table.
    pub aliases: Vec<String>,
}

impl From<ResmedSignalDescriptor> for ResmedSignalDto {
    fn from(signal: ResmedSignalDescriptor) -> Self {
        Self {
            file: signal.file,
            aliases: signal
                .aliases
                .iter()
                .map(|alias| String::from(*alias))
                .collect(),
        }
    }
}

/// Owned, round-trippable `ResMed` span-endpoint DTO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResmedSpanEndpointDto {
    /// File family in which the endpoint label has this meaning.
    pub file: ResmedFileKind,
    /// Exact annotation label accepted by the pinned loader.
    pub alias: String,
    /// Whether this annotation opens or closes the span.
    pub role: SpanEndpointRole,
}

impl From<ResmedSpanEndpointDescriptor> for ResmedSpanEndpointDto {
    fn from(endpoint: ResmedSpanEndpointDescriptor) -> Self {
        Self {
            file: endpoint.file,
            alias: String::from(endpoint.alias),
            role: endpoint.role,
        }
    }
}

/// Owned, round-trippable span semantics DTO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanSemanticsDto {
    /// Meaning of each endpoint timestamp.
    pub endpoint_timestamp: SpanEndpointTimestamp,
    /// Endpoint whose timestamp is stored for the completed span.
    pub stored_timestamp: SpanEndpointRole,
    /// Meaning of the completed span's numeric payload.
    pub payload: SpanPayload,
    /// Exact endpoint annotations accepted by the pinned loader.
    pub endpoints: Vec<ResmedSpanEndpointDto>,
}

impl From<SpanSemantics> for SpanSemanticsDto {
    fn from(semantics: SpanSemantics) -> Self {
        Self {
            endpoint_timestamp: semantics.endpoint_timestamp,
            stored_timestamp: semantics.stored_timestamp,
            payload: semantics.payload,
            endpoints: semantics
                .endpoints
                .iter()
                .copied()
                .map(ResmedSpanEndpointDto::from)
                .collect(),
        }
    }
}

/// Owned, round-trippable channel DTO for storage and IPC boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelDto {
    /// Stable OPAP string key.
    pub key: String,
    /// Neutral OPAP display label.
    pub label: String,
    /// Storage shape.
    pub kind: ChannelKind,
    /// Canonical OPAP unit.
    pub unit: Unit,
    /// Compatibility-only OSCAR metadata.
    pub legacy_oscar: LegacyOscarMetadataDto,
    /// Supported `ResMed` aliases grouped by file family.
    pub resmed_signals: Vec<ResmedSignalDto>,
    /// Individual-record semantics for event channels.
    pub event_semantics: Option<EventSemantics>,
    /// Paired-endpoint semantics for span channels.
    #[serde(default)]
    pub span_semantics: Option<SpanSemanticsDto>,
    /// Formula-level analytics use, if any.
    pub analytics_role: Option<AnalyticsRole>,
}

impl ChannelDto {
    /// Resolve this DTO's stable key to the canonical static registry entry.
    ///
    /// Deserialized labels, units, aliases, and legacy metadata are data, not
    /// authority. Consumers making domain decisions should call this method and
    /// use the returned definition.
    #[must_use]
    pub fn registered_definition(&self) -> Option<&'static ChannelDefinition> {
        crate::by_stable_key(&self.key)
    }

    /// Return whether every field exactly matches the current registry entry.
    ///
    /// This is useful when a serialized DTO is intended to be a canonical
    /// metadata snapshot rather than a presentation override.
    #[must_use]
    pub fn is_canonical_snapshot(&self) -> bool {
        self.registered_definition()
            .is_some_and(|definition| definition.to_dto() == *self)
    }
}
