// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (c) 2026 OPAP contributors
// Synthetic conformance coverage for the OSCAR-informed EDF implementation.

use opap_edf::{Limits, ParseErrorKind, Parser, SignalData, parse, parse_header};

#[derive(Clone)]
struct SignalSpec<'a> {
    label: &'a str,
    dimension: &'a str,
    physical_min: &'a str,
    physical_max: &'a str,
    digital_min: &'a str,
    digital_max: &'a str,
    samples_per_record: &'a str,
}

fn signal<'a>(label: &'a str, samples_per_record: &'a str) -> SignalSpec<'a> {
    SignalSpec {
        label,
        dimension: "unit",
        physical_min: "-100",
        physical_max: "300",
        digital_min: "-1000",
        digital_max: "1000",
        samples_per_record,
    }
}

fn field(value: &str, width: usize) -> Vec<u8> {
    assert!(value.len() <= width);
    let mut bytes = vec![b' '; width];
    bytes[..value.len()].copy_from_slice(value.as_bytes());
    bytes
}

fn synthetic_edf(
    signals: &[SignalSpec<'_>],
    record_count: &str,
    records: &[Vec<Vec<u8>>],
) -> Vec<u8> {
    let header_bytes = 256 + signals.len() * 256;
    let mut bytes = Vec::new();
    bytes.extend(field("0", 8));
    bytes.extend(field("patient", 80));
    bytes.extend(field("ResMed SRN=123456", 80));
    bytes.extend_from_slice(b"29.02.2401.02.03");
    bytes.extend(field(&header_bytes.to_string(), 8));
    bytes.extend(field("EDF+C", 44));
    bytes.extend(field(record_count, 8));
    bytes.extend(field("1", 8));
    bytes.extend(field(&signals.len().to_string(), 4));

    for spec in signals {
        bytes.extend(field(spec.label, 16));
    }
    for _ in signals {
        bytes.extend(field("", 80));
    }
    for spec in signals {
        bytes.extend(field(spec.dimension, 8));
    }
    for spec in signals {
        bytes.extend(field(spec.physical_min, 8));
    }
    for spec in signals {
        bytes.extend(field(spec.physical_max, 8));
    }
    for spec in signals {
        bytes.extend(field(spec.digital_min, 8));
    }
    for spec in signals {
        bytes.extend(field(spec.digital_max, 8));
    }
    for _ in signals {
        bytes.extend(field("", 80));
    }
    for spec in signals {
        bytes.extend(field(spec.samples_per_record, 8));
    }
    for _ in signals {
        bytes.extend(field("", 32));
    }
    assert_eq!(bytes.len(), header_bytes);

    for record in records {
        assert_eq!(record.len(), signals.len());
        for signal_bytes in record {
            bytes.extend_from_slice(signal_bytes);
        }
    }
    bytes
}

fn samples(values: &[i16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[test]
fn parses_fixed_header_and_affine_scaling() {
    let specs = [signal("Flow", "3")];
    let bytes = synthetic_edf(&specs, "1", &[vec![samples(&[-1000, 0, 1000])]]);
    let header = parse_header(&bytes).expect("valid header");
    assert_eq!(header.patient_id, "patient");
    assert_eq!(header.recording_id, "ResMed SRN=123456");
    assert_eq!(header.start.year, 2024);
    assert_eq!(header.start.month, 2);
    assert_eq!(header.start.day, 29);
    assert_eq!(header.signals[0].label, "Flow");

    let file = parse(&bytes).expect("valid EDF");
    assert_eq!(file.record_count(), 1);
    assert_eq!(
        file.signals()[0].digital_samples(),
        Some(&[-1000, 0, 1000][..])
    );
    let physical: Vec<_> = file.signals()[0]
        .physical_samples()
        .expect("calibrated")
        .collect();
    for (actual, expected) in physical.iter().zip([-100.0, 100.0, 300.0]) {
        assert!((actual - expected).abs() < 1e-10);
    }
}

#[test]
fn applies_the_edf_year_pivot_and_preserves_the_wire_year() {
    let specs = [signal("Flow", "1")];
    let mut bytes = synthetic_edf(&specs, "0", &[]);
    bytes[168..184].copy_from_slice(b"01.01.8400.00.00");
    let header = parse_header(&bytes).expect("2084 header");
    assert_eq!(header.start.year, 2084);
    assert_eq!(header.start.year_two_digits, 84);

    bytes[168..184].copy_from_slice(b"01.01.8500.00.00");
    let header = parse_header(&bytes).expect("1985 header");
    assert_eq!(header.start.year, 1985);
    assert_eq!(header.start.year_two_digits, 85);
}

#[test]
fn physical_iterator_rejects_calibrations_that_can_overflow() {
    let specs = [SignalSpec {
        label: "Flow",
        dimension: "unit",
        physical_min: "0",
        physical_max: "9e307",
        digital_min: "0",
        digital_max: "1",
        samples_per_record: "1",
    }];
    let bytes = synthetic_edf(&specs, "1", &[vec![samples(&[i16::MAX])]]);
    let file = parse(&bytes).expect("header values are finite");
    assert!(file.signals()[0].physical_samples().is_err());
    assert!(file.signals()[0].header.physical_value(i16::MAX).is_err());
}

#[test]
fn deinterleaves_multiple_signals_across_records_and_iterates_records() {
    let specs = [signal("Flow", "2"), signal("Pressure", "1")];
    let bytes = synthetic_edf(
        &specs,
        "2",
        &[
            vec![samples(&[1, -2]), samples(&[10])],
            vec![samples(&[3, i16::MIN]), samples(&[i16::MAX])],
        ],
    );
    let file = parse(&bytes).expect("valid EDF");
    assert_eq!(
        file.signals()[0].digital_samples(),
        Some(&[1, -2, 3, i16::MIN][..])
    );
    assert_eq!(
        file.signals()[1].digital_samples(),
        Some(&[10, i16::MAX][..])
    );

    let records: Vec<_> = file.records().collect();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].digital_samples(0), Some(&[1, -2][..]));
    assert_eq!(records[1].digital_samples(0), Some(&[3, i16::MIN][..]));
    assert_eq!(records[1].digital_samples(1), Some(&[i16::MAX][..]));
}

#[test]
fn exposes_annotations_per_signal_and_record() {
    let mut annotation = b"+1.5\x152\x14Obstructive apnea\x14Hypopnea\x14\0".to_vec();
    annotation.resize(64, 0);
    let specs = [signal("EDF Annotations", "32"), signal("Flow", "1")];
    let bytes = synthetic_edf(&specs, "1", &[vec![annotation, samples(&[7])]]);
    let file = parse(&bytes).expect("valid EDF+");
    let SignalData::Annotations(records) = &file.signals()[0].data else {
        panic!("expected annotation data");
    };
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].annotations.len(), 2);
    assert!((records[0].annotations[0].onset_seconds - 1.5).abs() < f64::EPSILON);
    assert!(
        (records[0].annotations[0]
            .duration_seconds
            .expect("duration")
            - 2.0)
            .abs()
            < f64::EPSILON
    );
    assert_eq!(records[0].annotations[1].text, "Hypopnea");
    assert_eq!(
        file.record(0).expect("record").annotations(0),
        Some(records[0].annotations.as_slice())
    );
}

#[test]
fn infers_unknown_record_count_and_preserves_duplicate_labels() {
    let specs = [signal("Flow", "1"), signal("Flow", "1")];
    let bytes = synthetic_edf(
        &specs,
        "-1",
        &[
            vec![samples(&[1]), samples(&[2])],
            vec![samples(&[3]), samples(&[4])],
        ],
    );
    let file = parse(&bytes).expect("valid unknown record count");
    assert_eq!(file.record_count(), 2);
    assert_eq!(file.signals_named("Flow").count(), 2);
}

#[test]
fn known_record_count_tolerates_and_reports_trailing_data_like_oscar() {
    let specs = [signal("Flow", "1")];
    let mut bytes = synthetic_edf(&specs, "1", &[vec![samples(&[9])]]);
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    let file = parse(&bytes).expect("trailing data is tolerated");
    assert_eq!(file.trailing_data_bytes(), 3);
    assert_eq!(file.signals()[0].digital_samples(), Some(&[9][..]));
}

#[test]
fn rejects_fixed_header_signal_header_and_payload_truncation() {
    let fixed = parse(&vec![b' '; 255]).expect_err("fixed header is truncated");
    assert!(matches!(fixed.kind, ParseErrorKind::UnexpectedEof { .. }));

    let specs = [signal("Flow", "1")];
    let full = synthetic_edf(&specs, "1", &[vec![samples(&[1])]]);
    let descriptors = parse(&full[..511]).expect_err("signal header is truncated");
    assert!(matches!(
        descriptors.kind,
        ParseErrorKind::UnexpectedEof { .. }
    ));

    let payload = parse(&full[..full.len() - 1]).expect_err("sample is truncated");
    assert!(matches!(
        payload.kind,
        ParseErrorKind::DataLengthMismatch { .. }
    ));
}

#[test]
fn every_truncated_prefix_is_rejected_without_panicking() {
    let specs = [signal("Flow", "2")];
    let full = synthetic_edf(&specs, "1", &[vec![samples(&[1, 2])]]);
    for end in 0..full.len() {
        assert!(parse(&full[..end]).is_err(), "prefix ending at {end}");
    }
    assert!(parse(&full).is_ok());
}

#[test]
fn enforces_the_oscar_signal_count_boundary() {
    let none = synthetic_edf(&[], "0", &[]);
    assert!(matches!(
        parse(&none).expect_err("zero signals").kind,
        ParseErrorKind::LimitExceeded {
            resource: "signals",
            actual: 0,
            ..
        }
    ));

    let too_many = vec![signal("Flow", "0"); 257];
    let bytes = synthetic_edf(&too_many, "0", &[]);
    assert!(matches!(
        parse(&bytes).expect_err("257 signals").kind,
        ParseErrorKind::LimitExceeded {
            resource: "signals",
            limit: 256,
            actual: 257
        }
    ));
}

#[test]
fn rejects_malformed_fields_and_header_length() {
    let specs = [signal("Flow", "1")];
    let mut invalid_date = synthetic_edf(&specs, "0", &[]);
    invalid_date[168..184].copy_from_slice(b"31.02.2401.02.03");
    assert!(matches!(
        parse(&invalid_date).expect_err("invalid date").kind,
        ParseErrorKind::InvalidDateTime { .. }
    ));

    let mut non_ascii = synthetic_edf(&specs, "0", &[]);
    non_ascii[8] = 0xff;
    assert!(matches!(
        parse(&non_ascii).expect_err("non-ASCII header").kind,
        ParseErrorKind::InvalidAscii { .. }
    ));

    let mut wrong_length = synthetic_edf(&specs, "0", &[]);
    wrong_length[184..192].copy_from_slice(&field("999", 8));
    assert!(matches!(
        parse(&wrong_length).expect_err("bad declared size").kind,
        ParseErrorKind::HeaderLengthMismatch { .. }
    ));
}

#[test]
fn rejects_bad_unknown_lengths_malformed_annotations_and_limits() {
    let specs = [signal("Flow", "1")];
    let mut unknown = synthetic_edf(&specs, "-1", &[vec![samples(&[1])]]);
    unknown.push(0);
    assert!(matches!(
        parse(&unknown).expect_err("partial inferred record").kind,
        ParseErrorKind::DataLengthMismatch { .. }
    ));

    let mut annotation = b"+0\x14not terminated".to_vec();
    annotation.resize(32, b'x');
    let annotation_spec = [signal("EDF Annotations", "16")];
    let malformed = synthetic_edf(&annotation_spec, "1", &[vec![annotation]]);
    assert!(matches!(
        parse(&malformed).expect_err("unterminated TAL").kind,
        ParseErrorKind::MalformedAnnotation { .. }
    ));

    let limited = Parser::new(Limits {
        max_records: 1,
        ..Limits::default()
    });
    let two_records = synthetic_edf(&specs, "2", &[vec![samples(&[1])], vec![samples(&[2])]]);
    assert!(matches!(
        limited.parse(&two_records).expect_err("record limit").kind,
        ParseErrorKind::LimitExceeded {
            resource: "records",
            ..
        }
    ));
}

#[test]
fn rejects_nonempty_zero_byte_records_before_allocation_or_decode_work() {
    let specs = [signal("EDF Annotations", "0")];
    let bytes = synthetic_edf(&specs, "1000000", &[]);
    let error = parse(&bytes).expect_err("zero-byte records must be rejected");
    assert!(matches!(
        error.kind,
        ParseErrorKind::ZeroByteRecords {
            record_count: 1_000_000
        }
    ));
}

#[test]
fn bounds_signal_record_work_for_regular_and_zero_sample_signals() {
    let specs = [signal("Flow", "1"), signal("Pressure", "1")];
    let bytes = synthetic_edf(
        &specs,
        "3",
        &[
            vec![samples(&[1]), samples(&[2])],
            vec![samples(&[3]), samples(&[4])],
            vec![samples(&[5]), samples(&[6])],
        ],
    );
    let parser = Parser::new(Limits {
        max_signal_records: 5,
        ..Limits::default()
    });
    assert!(matches!(
        parser.parse(&bytes).expect_err("six decode blocks").kind,
        ParseErrorKind::LimitExceeded {
            resource: "signal-record blocks",
            limit: 5,
            actual: 6
        }
    ));

    let mixed_specs = [
        signal("Flow", "1"),
        signal("EDF Annotations", "0"),
        signal("EDF Annotations", "0"),
        signal("EDF Annotations", "0"),
    ];
    let mixed = synthetic_edf(
        &mixed_specs,
        "2",
        &[
            vec![samples(&[1]), vec![], vec![], vec![]],
            vec![samples(&[2]), vec![], vec![], vec![]],
        ],
    );
    let parser = Parser::new(Limits {
        max_signal_records: 7,
        ..Limits::default()
    });
    assert!(matches!(
        parser.parse(&mixed).expect_err("eight mixed blocks").kind,
        ParseErrorKind::LimitExceeded {
            resource: "signal-record blocks",
            limit: 7,
            actual: 8
        }
    ));
}

#[test]
fn bounds_dense_annotation_record_metadata_before_allocation() {
    let mut tal = b"+0\x14\x14\0".to_vec();
    tal.resize(8, 0);
    let specs = [signal("EDF Annotations", "4")];
    let bytes = synthetic_edf(
        &specs,
        "3",
        &[vec![tal.clone()], vec![tal.clone()], vec![tal]],
    );
    let parser = Parser::new(Limits {
        max_annotation_records: 2,
        ..Limits::default()
    });
    assert!(matches!(
        parser
            .parse(&bytes)
            .expect_err("three annotation records")
            .kind,
        ParseErrorKind::LimitExceeded {
            resource: "annotation records",
            limit: 2,
            actual: 3
        }
    ));
}

#[test]
fn bounds_decoded_annotation_objects_and_lossy_text_bytes() {
    let mut two_events = b"+0\x14\x14a\x14b\x14\0".to_vec();
    two_events.resize(24, 0);
    let specs = [signal("EDF Annotations", "12")];
    let bytes = synthetic_edf(&specs, "1", &[vec![two_events]]);
    let limited = Parser::new(Limits {
        max_annotations: 1,
        ..Limits::default()
    });
    assert!(matches!(
        limited.parse(&bytes).expect_err("two decoded events").kind,
        ParseErrorKind::LimitExceeded {
            resource: "annotations",
            limit: 1,
            actual: 2
        }
    ));
    let exact = Parser::new(Limits {
        max_annotations: 2,
        ..Limits::default()
    });
    assert_eq!(
        exact.parse(&bytes).expect("exact event limit").signals()[0]
            .annotation_records()
            .expect("annotations")[0]
            .annotations
            .len(),
        2
    );
    assert_eq!(
        exact
            .parse(&bytes)
            .expect("timekeeping onset")
            .record(0)
            .expect("record")
            .onset_seconds(),
        Some(0.0)
    );

    let mut invalid_utf8 = b"+0\x14\x14\xff\x14\0".to_vec();
    invalid_utf8.resize(16, 0);
    let utf8_specs = [signal("EDF Annotations", "8")];
    let utf8_bytes = synthetic_edf(&utf8_specs, "1", &[vec![invalid_utf8]]);
    let limited = Parser::new(Limits {
        max_annotation_text_bytes: 2,
        ..Limits::default()
    });
    assert!(matches!(
        limited
            .parse(&utf8_bytes)
            .expect_err("replacement character is three bytes")
            .kind,
        ParseErrorKind::LimitExceeded {
            resource: "decoded annotation text bytes",
            limit: 2,
            actual: 3
        }
    ));
    let exact = Parser::new(Limits {
        max_annotation_text_bytes: 3,
        ..Limits::default()
    });
    assert_eq!(
        exact
            .parse(&utf8_bytes)
            .expect("exact text limit")
            .signals()[0]
            .annotation_records()
            .expect("annotations")[0]
            .annotations[0]
            .text,
        "\u{fffd}"
    );
}

#[test]
fn annotation_object_budget_accumulates_across_records() {
    let mut first = b"+0\x14\x14a\x14\0".to_vec();
    first.resize(16, 0);
    let mut second = b"+1\x14\x14b\x14\0".to_vec();
    second.resize(16, 0);
    let specs = [signal("EDF Annotations", "8")];
    let bytes = synthetic_edf(&specs, "2", &[vec![first], vec![second]]);
    let parser = Parser::new(Limits {
        max_annotations: 1,
        ..Limits::default()
    });
    let error = parser
        .parse(&bytes)
        .expect_err("global event budget must include both records");
    assert!(matches!(
        error.kind,
        ParseErrorKind::LimitExceeded {
            resource: "annotations",
            limit: 1,
            actual: 2
        }
    ));
    assert_eq!(error.signal_index, Some(0));
    assert_eq!(error.record_index, Some(1));
}

#[test]
fn preserves_discontinuous_record_timekeeping_onsets() {
    let mut first = b"+0\x14\x14\0".to_vec();
    first.resize(8, 0);
    let mut second = b"+10\x14\x14\0".to_vec();
    second.resize(8, 0);
    let specs = [signal("EDF Annotations", "4")];
    let mut bytes = synthetic_edf(&specs, "2", &[vec![first], vec![second]]);
    bytes[192..236].copy_from_slice(&field("EDF+D", 44));
    bytes[244..252].copy_from_slice(&field("0", 8));

    let file = parse(&bytes).expect("valid discontinuous EDF+");
    let records = file.signals()[0]
        .annotation_records()
        .expect("annotation records");
    assert_eq!(records[0].record_onset_seconds, Some(0.0));
    assert_eq!(records[1].record_onset_seconds, Some(10.0));
    assert!(records.iter().all(|record| record.annotations.is_empty()));
    assert_eq!(file.record(0).expect("record 0").onset_seconds(), Some(0.0));
    assert_eq!(
        file.record(1).expect("record 1").onset_seconds(),
        Some(10.0)
    );
}

#[test]
fn primary_annotation_signal_is_the_only_canonical_record_clock() {
    let mut primary = b"+1\x14event\x14\0".to_vec();
    primary.resize(16, 0);
    let mut secondary = b"+99\x14\x14\0".to_vec();
    secondary.resize(16, 0);
    let specs = [
        signal("EDF Annotations", "8"),
        signal("EDF Annotations", "8"),
    ];
    let bytes = synthetic_edf(&specs, "1", &[vec![primary, secondary]]);
    let file = parse(&bytes).expect("valid multiple annotation signals");
    assert_eq!(
        file.signals()[1]
            .annotation_records()
            .expect("secondary records")[0]
            .record_onset_seconds,
        Some(99.0)
    );
    assert_eq!(file.record(0).expect("record").onset_seconds(), None);
}

#[test]
fn zero_duration_requires_a_structurally_valid_discontinuous_file() {
    let specs = [signal("Flow", "1")];
    let mut continuous = synthetic_edf(&specs, "1", &[vec![samples(&[1])]]);
    continuous[244..252].copy_from_slice(&field("0", 8));
    assert!(matches!(
        parse(&continuous)
            .expect_err("continuous zero-duration record")
            .kind,
        ParseErrorKind::ValueOutOfRange {
            field: "record duration",
            ..
        }
    ));

    let mut discontinuous = continuous;
    discontinuous[192..236].copy_from_slice(&field("EDF+D", 44));
    assert!(matches!(
        parse(&discontinuous)
            .expect_err("EDF+D requires its primary annotation clock")
            .kind,
        ParseErrorKind::MissingTimekeepingSignal
    ));
}

#[test]
fn discontinuous_files_require_a_timekeeping_onset_in_every_record() {
    let mut first_clock = b"+0\x14\x14\0".to_vec();
    first_clock.resize(8, 0);
    let mut second_without_clock = b"+1\x14event\x14\0".to_vec();
    second_without_clock.resize(16, 0);
    let specs = [signal("Flow", "1"), signal("EDF Annotations", "8")];
    let mut first_clock_wide = first_clock;
    first_clock_wide.resize(16, 0);
    let mut bytes = synthetic_edf(
        &specs,
        "2",
        &[
            vec![samples(&[1]), first_clock_wide],
            vec![samples(&[2]), second_without_clock],
        ],
    );
    bytes[192..236].copy_from_slice(&field("EDF+D", 44));
    bytes[244..252].copy_from_slice(&field("0", 8));

    let error = parse(&bytes).expect_err("second record has no authoritative clock");
    assert!(matches!(
        error.kind,
        ParseErrorKind::MissingRecordTimekeepingOnset
    ));
    assert_eq!(error.signal_index, Some(1));
    assert_eq!(error.record_index, Some(1));
}

#[test]
fn discontinuous_files_require_the_exact_standard_annotation_label() {
    let mut clock = b"+0\x14\x14\0".to_vec();
    clock.resize(8, 0);
    let specs = [signal("X Annotations", "4")];
    let mut bytes = synthetic_edf(&specs, "1", &[vec![clock]]);
    bytes[192..236].copy_from_slice(&field("EDF+D", 44));

    assert!(matches!(
        parse(&bytes).expect_err("nonstandard primary label").kind,
        ParseErrorKind::MissingTimekeepingSignal
    ));
}
