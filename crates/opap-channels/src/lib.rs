// SPDX-License-Identifier: GPL-3.0-only
//
// Compatibility metadata in this crate is derived from OSCAR-code at commit
// 64c5e90a26f91fb15868bcfcccde0c1e1522ac86:
// Copyright (c) 2019-2025 The OSCAR Team
// Copyright (c) 2011-2018 Mark Watkins
//
// OPAP Rust implementation:
// Copyright (c) 2026 OPAP contributors

//! Stable, evidence-backed channel metadata for OPAP.
//!
//! This crate deliberately separates OPAP's stable string keys from OSCAR's
//! legacy numeric channel IDs. Numeric IDs are compatibility metadata, never
//! the primary identity of an OPAP channel.
//!
//! The registry is intentionally small. It covers channels directly used by
//! OSCAR's pinned `ResMed` EVE/BRP/PLD paths plus the event, pressure, and leak
//! inputs used by OPAP analytics. It does not guess unsupported signals,
//! clinical thresholds, or diagnoses.
//!
//! Event channels require special care: [`Unit::EventsPerHour`] describes the
//! summary/display unit. A `ResMed` EVE record's stored value is the source EDF
//! annotation duration in seconds, or the parser's missing-duration sentinel,
//! as described by [`EventSemantics`].

#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

extern crate alloc;

mod model;
mod registry;

pub use model::{
    AnalyticsRole, ChannelDefinition, ChannelDto, ChannelKind, EventPayload, EventSemantics,
    EventTimestamp, LegacyOscarChannelId, LegacyOscarMetadata, LegacyOscarMetadataDto,
    ResmedFileKind, ResmedSignalDescriptor, ResmedSignalDto, StableChannelKey, Unit,
};
pub use registry::{CHANNELS, by_legacy_id, by_legacy_numeric_id, by_stable_key, resmed_signal};
