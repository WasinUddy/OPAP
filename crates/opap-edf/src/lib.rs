// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (c) 2026 OPAP contributors
//
// This clean Rust implementation is informed by OSCAR's EDF parser.
// OSCAR is copyright (c) 2019-2026 The OSCAR Team and derives from
// SleepyHead, copyright (c) 2011-2018 Mark Watkins.

//! Safe, dependency-free parsing of EDF and EDF+ data used by PAP devices.
//!
//! The parser accepts an in-memory byte slice so filesystem and decompression
//! policy remain in the caller. This also keeps the crate suitable for WASM.
//!
//! # Conformance and OSCAR compatibility
//!
//! EDF+D is parsed strictly: it requires an exact primary `EDF Annotations`
//! signal and an empty timekeeping TAL in every record. For continuous files,
//! signal labels containing `Annotations`, trailing bytes after declared
//! records, and lossy UTF-8 annotation text follow OSCAR compatibility behavior.

#![forbid(unsafe_code)]

mod error;
mod model;
mod parser;

pub use error::{CalibrationError, ParseError, ParseErrorKind};
pub use model::{
    Annotation, AnnotationRecord, EdfDateTime, EdfFile, EdfHeader, PhysicalSamples, Record,
    Records, Signal, SignalData, SignalHeader,
};
pub use parser::{Limits, Parser};

/// Parse a complete EDF/EDF+ byte stream with conservative default limits.
///
/// # Errors
///
/// Returns [`ParseError`] when a field, size, annotation, or data boundary is
/// invalid.
pub fn parse(bytes: &[u8]) -> Result<EdfFile, ParseError> {
    Parser::default().parse(bytes)
}

/// Parse only the fixed and per-signal headers with default limits.
///
/// Data records are not decoded or validated by this function.
///
/// # Errors
///
/// Returns [`ParseError`] when the input does not contain a valid complete EDF
/// header.
pub fn parse_header(bytes: &[u8]) -> Result<EdfHeader, ParseError> {
    Parser::default().parse_header(bytes)
}
