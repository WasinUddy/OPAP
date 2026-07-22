//! Deterministic CPAP summary calculations for OPAP.
//!
//! This crate is deliberately pure: it performs no I/O and does not depend on
//! `SQLite`, Tauri, a timezone database, or a device parser. Inputs therefore
//! carry explicit time and weighting semantics.
//!
//! The calculations are descriptive transformations of device-provided data.
//! They do not diagnose, score treatment effectiveness, or provide medical
//! advice. See `OSCAR_PROVENANCE.md` in this crate for exact pinned OSCAR source
//! references, deliberate behavioral differences, and the non-parity scope.

#![forbid(unsafe_code)]

mod day;
mod error;
mod events;
mod signal;
mod usage;

pub use day::{
    CivilDate, CorrectedLocalTimestampMs, DailySummary, SessionAnalyticsInput, TherapyDay,
    aggregate_by_noon, therapy_day_from_local_epoch_ms, therapy_day_from_normalized_utc,
    therapy_day_from_raw_device_local_epoch_ms,
};
pub use error::AnalyticsError;
pub use events::{AhiEventCounts, EventIndices, calculate_event_indices};
pub use signal::{
    LeakPressureSummary, SignalSummary, WeightedSample, summarize_leak_and_pressure,
    summarize_regular_signal, summarize_signal, weighted_percentile,
};
pub use usage::{
    SessionUsageInput, SliceState, TherapySlice, UsageSummary, summarize_session_usage,
    summarize_therapy_usage,
};

/// Intentional lossy boundary used only for descriptive floating-point output.
/// Integer validation and overflow checks happen before this conversion.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn u64_as_f64(value: u64) -> f64 {
    value as f64
}
