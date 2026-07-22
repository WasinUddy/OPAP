// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (c) 2026 OPAP contributors
//
// Selectively reimplements behavior inspected in OSCAR-code commit
// 64c5e90a26f91fb15868bcfcccde0c1e1522ac86. The pinned edfparser files are
// copyright (c) 2019-2025 The OSCAR Team and (c) 2011-2018 Mark Watkins.
// Exact source URLs, hashes, and intentional differences are in README.md.

//! Safe, dependency-free parsing of EDF and EDF+ data used by PAP devices.
//!
//! The parser accepts an in-memory byte slice so filesystem and decompression
//! policy remain in the caller. This also keeps the crate suitable for WASM.
//!
//! # Conformance and selective OSCAR compatibility
//!
//! EDF+C and EDF+D are parsed strictly: both require an exact primary
//! `EDF Annotations` signal and an empty timekeeping TAL in every record. EDF+C
//! record clocks must be contiguous. For plain EDF and compatible inputs,
//! signal labels containing `Annotations`, trailing bytes after declared
//! records, and lossy UTF-8 annotation text are behaviors verified against the
//! OSCAR source pinned in the crate README. This is not a claim of full parser
//! parity.

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
