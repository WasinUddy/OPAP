// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (c) 2026 OPAP contributors
//
// Selective behavior source (not a full-parity claim): OSCAR-code
// 64c5e90a26f91fb15868bcfcccde0c1e1522ac86, edfparser.cpp, SHA-256
// e86ae3953dbda904d12c602a3652bf6445e9eb4cea0ea3b77af810ccaae84086.
// Signal ordering, case-sensitive "Annotations" recognition, and declared-count
// trailing-data tolerance were verified there. Bounds, strict validation,
// timeline handling, and arithmetic intentionally differ; see README.md.

use crate::{
    Annotation, AnnotationRecord, EdfDateTime, EdfFile, EdfHeader, ParseError, ParseErrorKind,
    Signal, SignalData, SignalHeader,
};

const FIXED_HEADER_BYTES: usize = 256;
const SIGNAL_HEADER_BYTES: usize = 256;
const EDF_MAX_SIGNALS: usize = 256;
const ANNOTATION_SEPARATOR: u8 = 0x14;
const ANNOTATION_DURATION: u8 = 0x15;

/// Resource ceilings applied before parser-controlled allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_signals: usize,
    pub max_records: usize,
    /// Bounds the nested record-by-signal decode loop and annotation metadata.
    pub max_signal_records: usize,
    pub max_total_samples: usize,
    pub max_annotation_bytes: usize,
    pub max_annotation_records: usize,
    pub max_annotations: usize,
    pub max_annotation_text_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_signals: EDF_MAX_SIGNALS,
            max_records: 1_000_000,
            max_signal_records: 4_000_000,
            max_total_samples: 100_000_000,
            max_annotation_bytes: 64 * 1024 * 1024,
            max_annotation_records: 1_000_000,
            max_annotations: 1_000_000,
            max_annotation_text_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Configurable, reusable EDF parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Parser {
    limits: Limits,
}

impl Parser {
    #[must_use]
    pub const fn new(limits: Limits) -> Self {
        Self { limits }
    }

    #[must_use]
    pub const fn limits(&self) -> Limits {
        self.limits
    }

    /// Parse fixed and signal headers without decoding records.
    ///
    /// # Errors
    ///
    /// Returns an error for truncated, malformed, inconsistent, or over-limit
    /// headers.
    #[allow(clippy::too_many_lines)]
    pub fn parse_header(&self, bytes: &[u8]) -> Result<EdfHeader, ParseError> {
        if bytes.len() < FIXED_HEADER_BYTES {
            return Err(ParseError::new(
                bytes.len(),
                ParseErrorKind::UnexpectedEof {
                    context: "fixed header",
                    needed: FIXED_HEADER_BYTES,
                    available: bytes.len(),
                },
            ));
        }

        let version = ascii_field(bytes, 0, 8, "version")?;
        parse_number::<u32>(&version, 0, "version")?;
        let patient_id = ascii_field(bytes, 8, 80, "patient identification")?;
        let recording_id = ascii_field(bytes, 88, 80, "recording identification")?;
        let start = parse_datetime(&bytes[168..184], 168)?;
        let header_bytes_text = ascii_field(bytes, 184, 8, "header byte count")?;
        let header_bytes = parse_number::<usize>(&header_bytes_text, 184, "header byte count")?;
        let reserved = ascii_field(bytes, 192, 44, "reserved header")?;
        let record_count_text = ascii_field(bytes, 236, 8, "data record count")?;
        let record_count_signed =
            parse_number::<i64>(&record_count_text, 236, "data record count")?;
        let declared_record_count = match record_count_signed {
            -1 => None,
            0.. => Some(usize::try_from(record_count_signed).map_err(|_| {
                ParseError::new(
                    236,
                    ParseErrorKind::ValueOutOfRange {
                        field: "data record count",
                        value: record_count_text.clone(),
                    },
                )
            })?),
            _ => {
                return Err(ParseError::new(
                    236,
                    ParseErrorKind::ValueOutOfRange {
                        field: "data record count",
                        value: record_count_text,
                    },
                ));
            }
        };
        let duration_text = ascii_field(bytes, 244, 8, "record duration")?;
        let record_duration_seconds = parse_finite_float(&duration_text, 244, "record duration")?;
        if record_duration_seconds < 0.0 {
            return Err(ParseError::new(
                244,
                ParseErrorKind::ValueOutOfRange {
                    field: "record duration",
                    value: duration_text,
                },
            ));
        }
        let signal_count_text = ascii_field(bytes, 252, 4, "signal count")?;
        let signal_count = parse_number::<usize>(&signal_count_text, 252, "signal count")?;
        let maximum_signals = self.limits.max_signals.min(EDF_MAX_SIGNALS);
        if signal_count == 0 || signal_count > maximum_signals {
            return Err(ParseError::new(
                252,
                ParseErrorKind::LimitExceeded {
                    resource: "signals",
                    limit: maximum_signals,
                    actual: signal_count,
                },
            ));
        }

        let descriptor_bytes =
            checked_mul(signal_count, SIGNAL_HEADER_BYTES, 252, "signal headers")?;
        let expected_header_bytes =
            checked_add(FIXED_HEADER_BYTES, descriptor_bytes, 252, "complete header")?;
        if header_bytes != expected_header_bytes {
            return Err(ParseError::new(
                184,
                ParseErrorKind::HeaderLengthMismatch {
                    declared: header_bytes,
                    expected: expected_header_bytes,
                },
            ));
        }
        if bytes.len() < expected_header_bytes {
            return Err(ParseError::new(
                bytes.len(),
                ParseErrorKind::UnexpectedEof {
                    context: "signal headers",
                    needed: expected_header_bytes,
                    available: bytes.len(),
                },
            ));
        }

        let signals = parse_signal_headers(bytes, signal_count)?;
        let is_continuous = reserved.starts_with("EDF+C");
        let is_discontinuous = reserved.starts_with("EDF+D");
        let primary_annotation_index = signals
            .iter()
            .position(SignalHeader::is_standard_annotation_signal);
        let zero_duration_is_valid = is_discontinuous
            && signals
                .iter()
                .filter(|signal| !signal.is_standard_annotation_signal())
                .all(|signal| signal.samples_per_record == 1);
        if record_duration_seconds == 0.0 && !zero_duration_is_valid {
            return Err(ParseError::new(
                244,
                ParseErrorKind::ValueOutOfRange {
                    field: "record duration",
                    value: duration_text,
                },
            ));
        }
        if (is_continuous || is_discontinuous) && primary_annotation_index.is_none() {
            return Err(ParseError::new(
                FIXED_HEADER_BYTES,
                ParseErrorKind::MissingTimekeepingSignal,
            ));
        }
        Ok(EdfHeader {
            version,
            patient_id,
            recording_id,
            start,
            header_bytes,
            reserved,
            declared_record_count,
            record_duration_seconds,
            signals,
        })
    }

    /// Parse and decode a complete EDF/EDF+ stream.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid headers, unsafe sizes, truncated records, or
    /// malformed EDF+ annotations.
    #[allow(clippy::too_many_lines)]
    pub fn parse(&self, bytes: &[u8]) -> Result<EdfFile, ParseError> {
        let header = self.parse_header(bytes)?;
        let data_offset = header.header_bytes;
        let available_data = bytes.len() - data_offset;
        let primary_annotation_index = header
            .signals
            .iter()
            .position(SignalHeader::is_standard_annotation_signal);

        let bytes_per_record = header.signals.iter().enumerate().try_fold(
            0usize,
            |total, (signal_index, signal)| {
                let signal_bytes = checked_mul(
                    signal.samples_per_record,
                    2,
                    data_offset,
                    "signal bytes per record",
                )
                .map_err(|error| error.signal(signal_index))?;
                checked_add(total, signal_bytes, data_offset, "bytes per data record")
                    .map_err(|error| error.signal(signal_index))
            },
        )?;

        let record_count = match header.declared_record_count {
            Some(count) => count,
            None if bytes_per_record == 0 => {
                return Err(ParseError::new(
                    data_offset,
                    ParseErrorKind::UnknownRecordCountWithEmptyRecord,
                ));
            }
            None => {
                if available_data % bytes_per_record != 0 {
                    let complete_bytes = (available_data / bytes_per_record) * bytes_per_record;
                    return Err(ParseError::new(
                        data_offset + complete_bytes,
                        ParseErrorKind::DataLengthMismatch {
                            expected: complete_bytes + bytes_per_record,
                            available: available_data,
                        },
                    ));
                }
                available_data / bytes_per_record
            }
        };
        enforce_limit(
            "records",
            self.limits.max_records,
            record_count,
            data_offset,
        )?;
        if record_count > 0 && bytes_per_record == 0 {
            return Err(ParseError::new(
                data_offset,
                ParseErrorKind::ZeroByteRecords { record_count },
            ));
        }

        let signal_records = checked_mul(
            header.signals.len(),
            record_count,
            data_offset,
            "signal-record blocks",
        )?;
        enforce_limit(
            "signal-record blocks",
            self.limits.max_signal_records,
            signal_records,
            data_offset,
        )?;
        let annotation_signal_count = header
            .signals
            .iter()
            .filter(|signal| signal.is_annotation_signal())
            .count();
        let annotation_records = checked_mul(
            annotation_signal_count,
            record_count,
            data_offset,
            "annotation records",
        )?;
        enforce_limit(
            "annotation records",
            self.limits.max_annotation_records,
            annotation_records,
            data_offset,
        )?;

        let expected_data = checked_mul(
            bytes_per_record,
            record_count,
            data_offset,
            "complete record data",
        )?;
        if available_data < expected_data {
            return Err(ParseError::new(
                bytes.len(),
                ParseErrorKind::DataLengthMismatch {
                    expected: expected_data,
                    available: available_data,
                },
            ));
        }

        let (total_samples, annotation_bytes) = header.signals.iter().enumerate().try_fold(
            (0usize, 0usize),
            |(sample_total, annotation_total), (signal_index, signal)| {
                let signal_samples = checked_mul(
                    signal.samples_per_record,
                    record_count,
                    data_offset,
                    "samples in signal",
                )
                .map_err(|error| error.signal(signal_index))?;
                let sample_total =
                    checked_add(sample_total, signal_samples, data_offset, "total samples")
                        .map_err(|error| error.signal(signal_index))?;
                let annotation_total = if signal.is_annotation_signal() {
                    let bytes_for_signal =
                        checked_mul(signal_samples, 2, data_offset, "annotation bytes")
                            .map_err(|error| error.signal(signal_index))?;
                    checked_add(
                        annotation_total,
                        bytes_for_signal,
                        data_offset,
                        "total annotation bytes",
                    )
                    .map_err(|error| error.signal(signal_index))?
                } else {
                    annotation_total
                };
                Ok((sample_total, annotation_total))
            },
        )?;
        enforce_limit(
            "samples",
            self.limits.max_total_samples,
            total_samples,
            data_offset,
        )?;
        enforce_limit(
            "annotation bytes",
            self.limits.max_annotation_bytes,
            annotation_bytes,
            data_offset,
        )?;

        let mut decoded = Vec::new();
        try_reserve_vec(
            &mut decoded,
            header.signals.len(),
            data_offset,
            "decoded signals",
        )?;
        for (signal_index, signal) in header.signals.iter().enumerate() {
            if signal.is_annotation_signal() {
                let mut records = Vec::new();
                try_reserve_vec(
                    &mut records,
                    record_count,
                    data_offset,
                    "annotation records",
                )
                .map_err(|error| error.signal(signal_index))?;
                decoded.push(SignalData::Annotations(records));
            } else {
                let sample_count = checked_mul(
                    signal.samples_per_record,
                    record_count,
                    data_offset,
                    "decoded signal samples",
                )
                .map_err(|error| error.signal(signal_index))?;
                let mut samples = Vec::new();
                try_reserve_vec(&mut samples, sample_count, data_offset, "digital samples")
                    .map_err(|error| error.signal(signal_index))?;
                decoded.push(SignalData::Digital(samples));
            }
        }

        let mut cursor = data_offset;
        let mut annotation_budget = AnnotationBudget::default();
        let mut continuous_previous_onset = None;
        for record_index in 0..record_count {
            for (signal_index, signal) in header.signals.iter().enumerate() {
                let byte_count =
                    checked_mul(signal.samples_per_record, 2, cursor, "signal record bytes")
                        .map_err(|error| error.signal(signal_index).record(record_index))?;
                let end = checked_add(cursor, byte_count, cursor, "signal record boundary")
                    .map_err(|error| error.signal(signal_index).record(record_index))?;
                let record_bytes = &bytes[cursor..end];
                match &mut decoded[signal_index] {
                    SignalData::Digital(samples) => {
                        samples.extend(
                            record_bytes
                                .chunks_exact(2)
                                .map(|pair| i16::from_le_bytes([pair[0], pair[1]])),
                        );
                    }
                    SignalData::Annotations(records) => {
                        let parsed = parse_annotations(
                            record_bytes,
                            cursor,
                            &mut annotation_budget,
                            &self.limits,
                        )
                        .map_err(|error| error.signal(signal_index).record(record_index))?;
                        if header.is_edf_plus() && primary_annotation_index == Some(signal_index) {
                            let Some(record_onset) = parsed.record_onset_seconds else {
                                return Err(ParseError::new(
                                    cursor,
                                    ParseErrorKind::MissingRecordTimekeepingOnset,
                                )
                                .signal(signal_index)
                                .record(record_index));
                            };
                            if record_index == 0 && !(0.0..1.0).contains(&record_onset) {
                                return Err(ParseError::new(
                                    cursor,
                                    ParseErrorKind::InvalidFirstRecordTimekeepingOnset,
                                )
                                .signal(signal_index)
                                .record(record_index));
                            }
                            if header.is_continuous() {
                                if record_index == 0 {
                                    continuous_previous_onset = Some(record_onset);
                                } else {
                                    let expected = continuous_previous_onset.unwrap_or(f64::NAN)
                                        + header.record_duration_seconds;
                                    if !expected.is_finite()
                                        || !edf_time_is_close(record_onset, expected)
                                    {
                                        return Err(ParseError::new(
                                            cursor,
                                            ParseErrorKind::NonContiguousRecordTimekeepingOnset,
                                        )
                                        .signal(signal_index)
                                        .record(record_index));
                                    }
                                    continuous_previous_onset = Some(record_onset);
                                }
                            }
                        }
                        records.push(AnnotationRecord {
                            record_index,
                            record_onset_seconds: parsed.record_onset_seconds,
                            annotations: parsed.annotations,
                        });
                    }
                }
                cursor = end;
            }
        }

        let mut signals = Vec::new();
        try_reserve_vec(
            &mut signals,
            header.signals.len(),
            data_offset,
            "decoded signals",
        )?;
        for (header, data) in header.signals.iter().cloned().zip(decoded) {
            signals.push(Signal { header, data });
        }
        let trailing_data_bytes = available_data - expected_data;

        Ok(EdfFile {
            header,
            signals,
            record_count,
            trailing_data_bytes,
        })
    }
}

fn edf_time_is_close(actual: f64, expected: f64) -> bool {
    let scale = actual.abs().max(expected.abs()).max(1.0);
    let tolerance = (16.0 * f64::EPSILON * scale).clamp(1e-12, 1e-6);
    (actual - expected).abs() <= tolerance
}

fn parse_signal_headers(
    bytes: &[u8],
    signal_count: usize,
) -> Result<Vec<SignalHeader>, ParseError> {
    let mut signals = Vec::new();
    try_reserve_vec(
        &mut signals,
        signal_count,
        FIXED_HEADER_BYTES,
        "signal headers",
    )?;
    signals.resize_with(signal_count, SignalHeader::empty);
    let mut cursor = FIXED_HEADER_BYTES;

    for (index, signal) in signals.iter_mut().enumerate() {
        signal.label = signal_ascii_field(bytes, &mut cursor, 16, "signal label", index)?;
    }
    for (index, signal) in signals.iter_mut().enumerate() {
        signal.transducer_type =
            signal_ascii_field(bytes, &mut cursor, 80, "transducer type", index)?;
    }
    for (index, signal) in signals.iter_mut().enumerate() {
        signal.physical_dimension =
            signal_ascii_field(bytes, &mut cursor, 8, "physical dimension", index)?;
    }
    for (index, signal) in signals.iter_mut().enumerate() {
        let offset = cursor;
        let text = signal_ascii_field(bytes, &mut cursor, 8, "physical minimum", index)?;
        signal.physical_minimum = parse_finite_float(&text, offset, "physical minimum")
            .map_err(|error| error.signal(index))?;
    }
    for (index, signal) in signals.iter_mut().enumerate() {
        let offset = cursor;
        let text = signal_ascii_field(bytes, &mut cursor, 8, "physical maximum", index)?;
        signal.physical_maximum = parse_finite_float(&text, offset, "physical maximum")
            .map_err(|error| error.signal(index))?;
    }
    for (index, signal) in signals.iter_mut().enumerate() {
        let offset = cursor;
        let text = signal_ascii_field(bytes, &mut cursor, 8, "digital minimum", index)?;
        signal.digital_minimum = parse_digital_bound(&text, offset, "digital minimum")
            .map_err(|error| error.signal(index))?;
    }
    for (index, signal) in signals.iter_mut().enumerate() {
        let offset = cursor;
        let text = signal_ascii_field(bytes, &mut cursor, 8, "digital maximum", index)?;
        signal.digital_maximum = parse_digital_bound(&text, offset, "digital maximum")
            .map_err(|error| error.signal(index))?;
    }
    for (index, signal) in signals.iter_mut().enumerate() {
        signal.prefiltering = signal_ascii_field(bytes, &mut cursor, 80, "prefiltering", index)?;
    }
    for (index, signal) in signals.iter_mut().enumerate() {
        let offset = cursor;
        let text = signal_ascii_field(bytes, &mut cursor, 8, "samples per record", index)?;
        signal.samples_per_record = parse_number::<usize>(&text, offset, "samples per record")
            .map_err(|error| error.signal(index))?;
    }
    for (index, signal) in signals.iter_mut().enumerate() {
        signal.reserved = signal_ascii_field(bytes, &mut cursor, 32, "signal reserved", index)?;
    }

    Ok(signals)
}

fn signal_ascii_field(
    bytes: &[u8],
    cursor: &mut usize,
    width: usize,
    field: &'static str,
    signal_index: usize,
) -> Result<String, ParseError> {
    let offset = *cursor;
    *cursor = checked_add(*cursor, width, offset, "signal header cursor")
        .map_err(|error| error.signal(signal_index))?;
    ascii_field(bytes, offset, width, field).map_err(|error| error.signal(signal_index))
}

fn ascii_field(
    bytes: &[u8],
    offset: usize,
    width: usize,
    field: &'static str,
) -> Result<String, ParseError> {
    let end = offset.checked_add(width).ok_or_else(|| {
        ParseError::new(
            offset,
            ParseErrorKind::ArithmeticOverflow {
                operation: "fixed-width field boundary",
            },
        )
    })?;
    let slice = bytes.get(offset..end).ok_or_else(|| {
        ParseError::new(
            offset,
            ParseErrorKind::UnexpectedEof {
                context: field,
                needed: width,
                available: bytes.len().saturating_sub(offset),
            },
        )
    })?;
    if !slice.is_ascii() {
        return Err(ParseError::new(
            offset,
            ParseErrorKind::InvalidAscii { field },
        ));
    }
    let text = core::str::from_utf8(slice).expect("ASCII is valid UTF-8");
    Ok(text
        .trim_matches(|character: char| character.is_ascii_whitespace() || character == '\0')
        .to_owned())
}

fn parse_number<T>(text: &str, offset: usize, field: &'static str) -> Result<T, ParseError>
where
    T: core::str::FromStr,
{
    text.parse::<T>().map_err(|_| {
        ParseError::new(
            offset,
            ParseErrorKind::InvalidNumber {
                field,
                value: text.to_owned(),
            },
        )
    })
}

fn parse_finite_float(text: &str, offset: usize, field: &'static str) -> Result<f64, ParseError> {
    let value = parse_number::<f64>(text, offset, field)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ParseError::new(
            offset,
            ParseErrorKind::ValueOutOfRange {
                field,
                value: text.to_owned(),
            },
        ))
    }
}

fn parse_digital_bound(text: &str, offset: usize, field: &'static str) -> Result<i32, ParseError> {
    let value = parse_number::<i32>(text, offset, field)?;
    if (i32::from(i16::MIN)..=i32::from(i16::MAX)).contains(&value) {
        Ok(value)
    } else {
        Err(ParseError::new(
            offset,
            ParseErrorKind::ValueOutOfRange {
                field,
                value: text.to_owned(),
            },
        ))
    }
}

fn parse_datetime(bytes: &[u8], offset: usize) -> Result<EdfDateTime, ParseError> {
    if !bytes.is_ascii() {
        return Err(ParseError::new(
            offset,
            ParseErrorKind::InvalidAscii {
                field: "start date/time",
            },
        ));
    }
    let value = core::str::from_utf8(bytes)
        .expect("ASCII is valid UTF-8")
        .to_owned();
    let separators_are_valid = bytes.len() == 16
        && bytes[2] == b'.'
        && bytes[5] == b'.'
        && bytes[10] == b'.'
        && bytes[13] == b'.';
    if !separators_are_valid {
        return Err(ParseError::new(
            offset,
            ParseErrorKind::InvalidDateTime { value },
        ));
    }

    let parse_pair = |start: usize| -> Option<u8> {
        let tens = bytes.get(start)?.checked_sub(b'0')?;
        let ones = bytes.get(start + 1)?.checked_sub(b'0')?;
        if tens <= 9 && ones <= 9 {
            tens.checked_mul(10)?.checked_add(ones)
        } else {
            None
        }
    };
    let Some(day) = parse_pair(0) else {
        return Err(ParseError::new(
            offset,
            ParseErrorKind::InvalidDateTime { value },
        ));
    };
    let Some(month) = parse_pair(3) else {
        return Err(ParseError::new(
            offset,
            ParseErrorKind::InvalidDateTime { value },
        ));
    };
    let Some(year_short) = parse_pair(6) else {
        return Err(ParseError::new(
            offset,
            ParseErrorKind::InvalidDateTime { value },
        ));
    };
    let Some(hour) = parse_pair(8) else {
        return Err(ParseError::new(
            offset,
            ParseErrorKind::InvalidDateTime { value },
        ));
    };
    let Some(minute) = parse_pair(11) else {
        return Err(ParseError::new(
            offset,
            ParseErrorKind::InvalidDateTime { value },
        ));
    };
    let Some(second) = parse_pair(14) else {
        return Err(ParseError::new(
            offset,
            ParseErrorKind::InvalidDateTime { value },
        ));
    };
    // EDF's 1985 pivot maps 85..99 to 1985..1999 and 00..84 to 2000..2084.
    // Preserve the raw two digits so device-specific layers can apply a
    // different policy without reparsing the header.
    let year = if year_short >= 85 {
        1900 + u16::from(year_short)
    } else {
        2000 + u16::from(year_short)
    };
    if month == 0
        || month > 12
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(ParseError::new(
            offset,
            ParseErrorKind::InvalidDateTime { value },
        ));
    }
    Ok(EdfDateTime {
        year,
        year_two_digits: year_short,
        month,
        day,
        hour,
        minute,
        second,
    })
}

const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 0,
    }
}

#[allow(clippy::too_many_lines)]
fn parse_annotations(
    bytes: &[u8],
    base_offset: usize,
    budget: &mut AnnotationBudget,
    limits: &Limits,
) -> Result<ParsedAnnotationRecord, ParseError> {
    let mut annotations = Vec::new();
    let mut record_onset_seconds = None;
    let mut cursor = 0usize;
    let mut tal_index = 0usize;
    while cursor < bytes.len() {
        let padding_start = cursor;
        while bytes.get(cursor) == Some(&0) {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        if cursor != padding_start {
            return Err(annotation_error(
                base_offset + padding_start,
                "a TAL cannot follow NUL padding",
            ));
        }
        let tal_start = cursor;
        let onset_has_positive_sign = bytes[cursor] == b'+';
        let sign = match bytes[cursor] {
            b'+' => 1.0,
            b'-' => -1.0,
            _ => {
                return Err(annotation_error(
                    base_offset + cursor,
                    "TAL onset must start with '+' or '-'",
                ));
            }
        };
        cursor += 1;
        let onset_start = cursor;
        while let Some(byte) = bytes.get(cursor) {
            if *byte == ANNOTATION_SEPARATOR || *byte == ANNOTATION_DURATION {
                break;
            }
            if *byte == 0 {
                return Err(annotation_error(
                    base_offset + cursor,
                    "TAL onset is missing its separator",
                ));
            }
            cursor += 1;
        }
        let marker = *bytes.get(cursor).ok_or_else(|| {
            annotation_error(
                base_offset + tal_start,
                "TAL onset reaches the end of the record",
            )
        })?;
        let onset = parse_annotation_number(
            &bytes[onset_start..cursor],
            base_offset + onset_start,
            "invalid TAL onset",
        )? * sign;

        let duration_seconds = if marker == ANNOTATION_DURATION {
            cursor += 1;
            let duration_start = cursor;
            while let Some(byte) = bytes.get(cursor) {
                if *byte == ANNOTATION_SEPARATOR {
                    break;
                }
                if *byte == 0 {
                    return Err(annotation_error(
                        base_offset + cursor,
                        "TAL duration is missing its separator",
                    ));
                }
                cursor += 1;
            }
            if bytes.get(cursor) != Some(&ANNOTATION_SEPARATOR) {
                return Err(annotation_error(
                    base_offset + duration_start,
                    "TAL duration reaches the end of the record",
                ));
            }
            Some(parse_annotation_number(
                &bytes[duration_start..cursor],
                base_offset + duration_start,
                "invalid TAL duration",
            )?)
        } else {
            None
        };

        cursor += 1; // separator after onset or duration
        let is_timekeeping_tal = tal_index == 0
            && tal_start == 0
            && onset_has_positive_sign
            && marker == ANNOTATION_SEPARATOR
            && bytes.get(cursor) == Some(&ANNOTATION_SEPARATOR);
        let mut text_start = cursor;
        let mut terminated = false;
        let mut saw_annotation_separator = false;
        while cursor < bytes.len() {
            match bytes[cursor] {
                ANNOTATION_SEPARATOR => {
                    push_annotation_text(
                        &mut annotations,
                        &bytes[text_start..cursor],
                        base_offset + text_start,
                        onset,
                        duration_seconds,
                        budget,
                        limits,
                    )?;
                    cursor += 1;
                    text_start = cursor;
                    saw_annotation_separator = true;
                }
                0 => {
                    if text_start != cursor || !saw_annotation_separator {
                        return Err(annotation_error(
                            base_offset + cursor,
                            "annotation text is missing its separator",
                        ));
                    }
                    cursor += 1;
                    terminated = true;
                    break;
                }
                _ => cursor += 1,
            }
        }
        if !terminated {
            return Err(annotation_error(
                base_offset + tal_start,
                "TAL is not NUL terminated",
            ));
        }
        if is_timekeeping_tal {
            record_onset_seconds = Some(onset);
        }
        tal_index += 1;
    }
    Ok(ParsedAnnotationRecord {
        record_onset_seconds,
        annotations,
    })
}

fn parse_annotation_number(
    bytes: &[u8],
    offset: usize,
    reason: &'static str,
) -> Result<f64, ParseError> {
    let mut has_digit = false;
    let mut has_decimal_point = false;
    let mut has_fractional_digit = false;
    for byte in bytes {
        match byte {
            b'0'..=b'9' => {
                has_digit = true;
                if has_decimal_point {
                    has_fractional_digit = true;
                }
            }
            b'.' if !has_decimal_point => has_decimal_point = true,
            _ => return Err(annotation_error(offset, reason)),
        }
    }
    if !has_digit || (has_decimal_point && !has_fractional_digit) {
        return Err(annotation_error(offset, reason));
    }
    let text = core::str::from_utf8(bytes).expect("ASCII is valid UTF-8");
    let value = text
        .parse::<f64>()
        .map_err(|_| annotation_error(offset, reason))?;
    if !value.is_finite() {
        return Err(annotation_error(offset, reason));
    }
    Ok(value)
}

fn push_annotation_text(
    annotations: &mut Vec<Annotation>,
    text: &[u8],
    text_offset: usize,
    onset_seconds: f64,
    duration_seconds: Option<f64>,
    budget: &mut AnnotationBudget,
    limits: &Limits,
) -> Result<(), ParseError> {
    if text.is_empty() {
        return Ok(());
    }

    let next_annotation_count = checked_add(
        budget.annotations,
        1,
        text_offset,
        "decoded annotation count",
    )?;
    enforce_limit(
        "annotations",
        limits.max_annotations,
        next_annotation_count,
        text_offset,
    )?;
    let decoded_text_bytes = lossy_utf8_len(text, text_offset)?;
    let next_text_bytes = checked_add(
        budget.text_bytes,
        decoded_text_bytes,
        text_offset,
        "decoded annotation text bytes",
    )?;
    enforce_limit(
        "decoded annotation text bytes",
        limits.max_annotation_text_bytes,
        next_text_bytes,
        text_offset,
    )?;
    let decoded_text = decode_lossy_utf8(text, decoded_text_bytes, text_offset)?;
    try_reserve_vec(annotations, 1, text_offset, "annotations")?;

    // QString::fromUtf8 in the pinned OSCAR parser replaces invalid annotation
    // sequences. Preserve that specific behavior while keeping headers strict.
    annotations.push(Annotation {
        onset_seconds,
        duration_seconds,
        text: decoded_text,
    });
    budget.annotations = next_annotation_count;
    budget.text_bytes = next_text_bytes;
    Ok(())
}

#[derive(Debug, Default)]
struct AnnotationBudget {
    annotations: usize,
    text_bytes: usize,
}

#[derive(Debug)]
struct ParsedAnnotationRecord {
    record_onset_seconds: Option<f64>,
    annotations: Vec<Annotation>,
}

fn lossy_utf8_len(bytes: &[u8], offset: usize) -> Result<usize, ParseError> {
    let mut remaining = bytes;
    let mut output_len = 0usize;
    loop {
        match core::str::from_utf8(remaining) {
            Ok(valid) => {
                return checked_add(output_len, valid.len(), offset, "lossy UTF-8 output length");
            }
            Err(error) => {
                output_len = checked_add(
                    output_len,
                    error.valid_up_to(),
                    offset,
                    "lossy UTF-8 output length",
                )?;
                output_len = checked_add(
                    output_len,
                    '\u{fffd}'.len_utf8(),
                    offset,
                    "lossy UTF-8 replacement",
                )?;
                let Some(error_len) = error.error_len() else {
                    return Ok(output_len);
                };
                remaining = &remaining[error.valid_up_to() + error_len..];
            }
        }
    }
}

fn decode_lossy_utf8(bytes: &[u8], output_len: usize, offset: usize) -> Result<String, ParseError> {
    let mut output = String::new();
    try_reserve_string(&mut output, output_len, offset, "annotation text bytes")?;
    let mut remaining = bytes;
    loop {
        match core::str::from_utf8(remaining) {
            Ok(valid) => {
                output.push_str(valid);
                return Ok(output);
            }
            Err(error) => {
                let valid_end = error.valid_up_to();
                let valid = core::str::from_utf8(&remaining[..valid_end])
                    .map_err(|_| annotation_error(offset, "invalid UTF-8 prefix"))?;
                output.push_str(valid);
                output.push('\u{fffd}');
                let Some(error_len) = error.error_len() else {
                    return Ok(output);
                };
                remaining = &remaining[valid_end + error_len..];
            }
        }
    }
}

const fn annotation_error(offset: usize, reason: &'static str) -> ParseError {
    ParseError::new(offset, ParseErrorKind::MalformedAnnotation { reason })
}

fn checked_add(
    left: usize,
    right: usize,
    offset: usize,
    operation: &'static str,
) -> Result<usize, ParseError> {
    left.checked_add(right)
        .ok_or_else(|| ParseError::new(offset, ParseErrorKind::ArithmeticOverflow { operation }))
}

fn checked_mul(
    left: usize,
    right: usize,
    offset: usize,
    operation: &'static str,
) -> Result<usize, ParseError> {
    left.checked_mul(right)
        .ok_or_else(|| ParseError::new(offset, ParseErrorKind::ArithmeticOverflow { operation }))
}

fn try_reserve_vec<T>(
    values: &mut Vec<T>,
    additional: usize,
    offset: usize,
    resource: &'static str,
) -> Result<(), ParseError> {
    let requested = checked_add(
        values.len(),
        additional,
        offset,
        "requested vector capacity",
    )?;
    values.try_reserve_exact(additional).map_err(|_| {
        ParseError::new(
            offset,
            ParseErrorKind::AllocationFailed {
                resource,
                requested,
            },
        )
    })
}

fn try_reserve_string(
    value: &mut String,
    additional: usize,
    offset: usize,
    resource: &'static str,
) -> Result<(), ParseError> {
    let requested = checked_add(value.len(), additional, offset, "requested string capacity")?;
    value.try_reserve_exact(additional).map_err(|_| {
        ParseError::new(
            offset,
            ParseErrorKind::AllocationFailed {
                resource,
                requested,
            },
        )
    })
}

fn enforce_limit(
    resource: &'static str,
    limit: usize,
    actual: usize,
    offset: usize,
) -> Result<(), ParseError> {
    if actual <= limit {
        Ok(())
    } else {
        Err(ParseError::new(
            offset,
            ParseErrorKind::LimitExceeded {
                resource,
                limit,
                actual,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_test_annotations(
        bytes: &[u8],
        base_offset: usize,
    ) -> Result<ParsedAnnotationRecord, ParseError> {
        parse_annotations(
            bytes,
            base_offset,
            &mut AnnotationBudget::default(),
            &Limits::default(),
        )
    }

    #[test]
    fn annotation_tal_supports_duration_multiple_texts_and_utf8() {
        let bytes = b"-1.5\x152.25\x14Obstruction\x14RER\xc3\x81\x14\0\0";
        let parsed = parse_test_annotations(bytes, 100).expect("valid annotations");
        let annotations = parsed.annotations;
        assert_eq!(annotations.len(), 2);
        assert!((annotations[0].onset_seconds - (-1.5)).abs() < f64::EPSILON);
        assert_eq!(annotations[0].duration_seconds, Some(2.25));
        assert_eq!(annotations[0].text, "Obstruction");
        assert_eq!(annotations[1].text, "RERÁ");
    }

    #[test]
    fn adjacent_tals_preserve_the_clock_and_decode_later_events() {
        let bytes = b"+0\x14\x14\0+1.5\x14Hypopnea\x14\0\0";
        let parsed = parse_test_annotations(bytes, 0).expect("adjacent TALs");

        assert_eq!(parsed.record_onset_seconds, Some(0.0));
        assert_eq!(parsed.annotations.len(), 1);
        assert!((parsed.annotations[0].onset_seconds - 1.5).abs() < f64::EPSILON);
        assert_eq!(parsed.annotations[0].text, "Hypopnea");
    }

    #[test]
    fn every_ascii_non_digit_in_a_datetime_pair_is_rejected_without_panicking() {
        const DIGIT_POSITIONS: [usize; 12] = [0, 1, 3, 4, 6, 7, 8, 9, 11, 12, 14, 15];
        let valid = *b"01.02.2403.04.05";
        assert!(parse_datetime(&valid, 0).is_ok(), "test seed must be valid");

        for position in DIGIT_POSITIONS {
            for byte in 0_u8..=127 {
                if byte.is_ascii_digit() {
                    continue;
                }
                let mut malformed = valid;
                malformed[position] = byte;
                let error = parse_datetime(&malformed, 0)
                    .expect_err("non-digit pair byte must be rejected");
                assert!(
                    matches!(error.kind, ParseErrorKind::InvalidDateTime { .. }),
                    "wrong error for byte {byte:#04x} at position {position}: {error:?}"
                );
            }
        }
    }

    #[test]
    fn annotation_tal_rejects_unterminated_data_without_panicking() {
        let error = parse_test_annotations(b"+0\x14event", 10).expect_err("must reject truncation");
        assert_eq!(error.offset, 10);
        assert!(matches!(
            error.kind,
            ParseErrorKind::MalformedAnnotation {
                reason: "TAL is not NUL terminated"
            }
        ));
    }

    #[test]
    fn empty_first_tal_preserves_record_onset_without_creating_an_event() {
        let parsed = parse_test_annotations(b"+567.25\x14\x14\0", 0).expect("timekeeping TAL");
        assert_eq!(parsed.record_onset_seconds, Some(567.25));
        assert!(parsed.annotations.is_empty());

        assert!(matches!(
            parse_test_annotations(b"+10\x14\0", 0)
                .expect_err("TAL must end with separator then NUL")
                .kind,
            ParseErrorKind::MalformedAnnotation { .. }
        ));
    }

    #[test]
    fn tal_numbers_reject_non_edf_numeric_grammar() {
        for bytes in [
            &b"+1e3\x14event\x14\0"[..],
            &b"++1\x14event\x14\0"[..],
            &b"+-1\x14event\x14\0"[..],
            &b"+1\x15-2\x14event\x14\0"[..],
            &b"+1\x15+2\x14event\x14\0"[..],
            &b"+0.\x14event\x14\0"[..],
            &b"+1\x151.\x14event\x14\0"[..],
        ] {
            assert!(
                matches!(
                    parse_test_annotations(bytes, 0)
                        .expect_err("invalid TAL number")
                        .kind,
                    ParseErrorKind::MalformedAnnotation { .. }
                ),
                "accepted {bytes:?}"
            );
        }
    }

    #[test]
    fn checked_arithmetic_reports_overflow() {
        let add = checked_add(usize::MAX, 1, 7, "test add").expect_err("overflow");
        assert!(matches!(
            add.kind,
            ParseErrorKind::ArithmeticOverflow {
                operation: "test add"
            }
        ));
        let multiply = checked_mul(usize::MAX, 2, 8, "test multiply").expect_err("overflow");
        assert!(matches!(
            multiply.kind,
            ParseErrorKind::ArithmeticOverflow {
                operation: "test multiply"
            }
        ));

        let mut values = Vec::<u8>::new();
        let reserve =
            try_reserve_vec(&mut values, usize::MAX, 9, "test bytes").expect_err("capacity");
        assert!(matches!(
            reserve.kind,
            ParseErrorKind::AllocationFailed {
                resource: "test bytes",
                requested: usize::MAX
            }
        ));
    }
}
