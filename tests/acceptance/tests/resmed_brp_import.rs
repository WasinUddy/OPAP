// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (c) 2026 OPAP contributors
//
// These acceptance fixtures are independently authored from the EDF field
// layout and contain no patient or manufacturer test data.

use cucumber::{World as _, given, then, when};
use opap_core::resmed::ResmedImporter;
use opap_core::{
    DeviceLocalDateTime, DirectorySource, ImportClockContext, ImportOptions, ImportReport,
    Importer, SessionDataKind,
};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::{TempDir, tempdir};

const CARD_SERIAL: &str = "SYNTHETIC-BRP-CARD-0001";
const OTHER_SERIAL: &str = "SYNTHETIC-BRP-OTHER-0002";
const BRP_PATH: &str = "DATALOG/20260102_220000_BRP.edf";
const FLOW_CHANNEL: &str = "pap.series.flow_rate";
const EXPECTED_START_UTC_MS: i64 = 1_767_366_000_250;
const EXPECTED_END_UTC_MS: i64 = 1_767_366_002_250;

#[derive(Debug, Clone, PartialEq, Eq)]
enum SnapshotEntry {
    Directory,
    File(Vec<u8>),
}

#[derive(Debug, Default, cucumber::World)]
struct BrpWorld {
    fixture: Option<TempDir>,
    fixture_path: Option<PathBuf>,
    original_contents: Option<BTreeMap<PathBuf, SnapshotEntry>>,
    report: Option<ImportReport>,
}

impl BrpWorld {
    fn card_path(&self) -> &Path {
        self.fixture_path
            .as_deref()
            .expect("a temporary ResMed card must exist")
    }

    fn report(&self) -> &ImportReport {
        self.report.as_ref().expect("the import must have run")
    }
}

#[given("a temporary ResMed card with a matching synthetic BRP recording")]
fn matching_card(world: &mut BrpWorld) {
    install_card(world, CARD_SERIAL);
}

#[given("a temporary ResMed card with a mismatched synthetic BRP recording")]
fn mismatched_card(world: &mut BrpWorld) {
    install_card(world, OTHER_SERIAL);
}

#[given("the generated card contents are recorded")]
fn record_card_contents(world: &mut BrpWorld) {
    world.original_contents = Some(snapshot(world.card_path()));
}

#[when("the BRP card is imported with an explicit fixed-offset clock")]
fn import_card(world: &mut BrpWorld) {
    let source = DirectorySource::open(world.card_path()).expect("open temporary card capability");
    let options = ImportOptions {
        clock_context: Some(ImportClockContext {
            current_device_local_time: DeviceLocalDateTime {
                year: 2030,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                millisecond: 0,
            },
            applied_utc_offset_seconds: 7 * 60 * 60,
            device_clock_correction_ms: 250,
            timezone_basis: Some("acceptance:fixed:+07:00".to_owned()),
        }),
        ..ImportOptions::default()
    };

    world.report = Some(
        Importer::import(&ResmedImporter, &source, &options)
            .expect("synthetic BRP import should complete"),
    );
}

#[then("exactly one partial BRP session is returned")]
fn one_partial_session(world: &mut BrpWorld) {
    let report = world.report();
    assert_eq!(report.sessions.len(), 1);
    assert_eq!(report.statistics.sessions_imported, 1);
    let session = &report.sessions[0];
    assert_eq!(session.data_kind, SessionDataKind::Partial);
    assert_eq!(session.therapy_day, "2026-01-02");
    assert_eq!(session.summary.usage_ms, 2_000);
    assert!(session.slices.is_empty());
    assert!(session.event_series.is_empty());
    assert!(session.settings.is_empty());
}

#[then("its device clock is normalized with the exact offset and correction")]
fn exact_clock_normalization(world: &mut BrpWorld) {
    let session = &world.report().sessions[0];
    assert_eq!(
        session.start_time.device_local_wall_time,
        "2026-01-02T22:00:00.000"
    );
    assert_eq!(
        session.end_time.device_local_wall_time,
        "2026-01-02T22:00:02.000"
    );
    assert_eq!(
        session.start_time.normalized_utc_unix_ms,
        EXPECTED_START_UTC_MS
    );
    assert_eq!(session.end_time.normalized_utc_unix_ms, EXPECTED_END_UTC_MS);
    for boundary in [&session.start_time, &session.end_time] {
        assert_eq!(boundary.applied_utc_offset_seconds, Some(7 * 60 * 60));
        assert_eq!(boundary.device_clock_correction_ms, 250);
        assert_eq!(
            boundary.timezone_basis.as_deref(),
            Some("acceptance:fixed:+07:00")
        );
    }
}

#[then("its flow samples use the full EDF affine calibration in litres per minute")]
fn affine_flow_samples(world: &mut BrpWorld) {
    let session = &world.report().sessions[0];
    let flow = session
        .waveforms
        .iter()
        .find(|series| series.channel_id == FLOW_CHANNEL)
        .expect("normalized flow waveform");
    assert_eq!(flow.start_time_unix_ms, EXPECTED_START_UTC_MS);
    assert_eq!(flow.sample_interval_ms, 500.0);
    assert_eq!(flow.samples, vec![-120.0, 0.0, 60.0, 120.0]);

    let encoding = flow.source_encoding.expect("EDF source calibration");
    assert_eq!(encoding.digital_minimum, -100);
    assert_eq!(encoding.digital_maximum, 100);
    assert_eq!(encoding.physical_minimum, -2.0);
    assert_eq!(encoding.physical_maximum, 2.0);
    assert_eq!(encoding.samples_per_record, 2);
    assert_eq!(encoding.record_duration_seconds, 1.0);
}

#[then("its imported identifiers are opaque and private")]
fn opaque_private_ids(world: &mut BrpWorld) {
    let session = &world.report().sessions[0];
    let fixture_path = world.card_path().to_string_lossy();
    for value in [
        session.id.as_str(),
        session.source_key.as_str(),
        session.waveforms[0].source_key.as_str(),
    ] {
        assert_opaque_sha256(value);
        for forbidden in [CARD_SERIAL, BRP_PATH, "DATALOG", fixture_path.as_ref()] {
            assert!(
                !value.contains(forbidden),
                "opaque identifier disclosed `{forbidden}`"
            );
        }
    }
}

#[then("the partial-session limitation is reported")]
fn partial_warning(world: &mut BrpWorld) {
    let report = world.report();
    let session = &report.sessions[0];
    let warning = report
        .warnings
        .iter()
        .find(|warning| warning.code == "resmed_partial_brp_session")
        .expect("partial BRP warning");
    assert_eq!(warning.session_id.as_deref(), Some(session.id.as_str()));
}

#[then("no phantom session is returned")]
fn no_phantom_session(world: &mut BrpWorld) {
    let report = world.report();
    assert!(report.sessions.is_empty());
    assert_eq!(report.statistics.sessions_imported, 0);
}

#[then("a privacy-safe BRP serial-mismatch warning is reported")]
fn privacy_safe_mismatch(world: &mut BrpWorld) {
    let report = world.report();
    let warning = report
        .warnings
        .iter()
        .find(|warning| warning.code == "resmed_brp_serial_mismatch")
        .expect("stable serial mismatch warning");
    assert!(warning.message.contains("file skipped"));

    let fixture_path = world.card_path().to_string_lossy();
    for warning in &report.warnings {
        for forbidden in [CARD_SERIAL, OTHER_SERIAL, fixture_path.as_ref()] {
            assert!(
                !warning.message.contains(forbidden),
                "warning message disclosed `{forbidden}`"
            );
        }
    }
}

#[then("the generated card is unchanged and disposable")]
fn unchanged_and_disposable(world: &mut BrpWorld) {
    let expected = world
        .original_contents
        .as_ref()
        .expect("original card snapshot");
    assert_eq!(&snapshot(world.card_path()), expected);

    let fixture = world.fixture.take().expect("temporary fixture");
    let path = fixture.path().to_owned();
    fixture.close().expect("remove temporary fixture");
    assert!(!path.exists(), "temporary fixture was not removed");
}

fn install_card(world: &mut BrpWorld, recording_serial: &str) {
    let fixture = tempdir().expect("create temporary card");
    fs::create_dir(fixture.path().join("DATALOG")).expect("create DATALOG directory");
    fs::write(fixture.path().join("STR.edf"), []).expect("write ResMed card marker");
    fs::write(
        fixture.path().join("Identification.tgt"),
        format!("#SRN {CARD_SERIAL}\n#PNA AirSense_10_AutoSet\n#PCD SYN-BRP-10\n"),
    )
    .expect("write synthetic machine identity");
    fs::write(
        fixture.path().join(BRP_PATH),
        build_brp_edf(recording_serial),
    )
    .expect("write synthetic BRP EDF");

    world.fixture_path = Some(fixture.path().to_owned());
    world.fixture = Some(fixture);
}

fn build_brp_edf(recording_serial: &str) -> Vec<u8> {
    const SIGNAL_COUNT: usize = 1;
    const HEADER_BYTES: usize = 256 + SIGNAL_COUNT * 256;
    const RECORD_COUNT: usize = 2;
    const SAMPLES_PER_RECORD: usize = 2;

    let mut bytes = Vec::with_capacity(HEADER_BYTES + RECORD_COUNT * SAMPLES_PER_RECORD * 2);
    append_field(&mut bytes, "0", 8);
    append_field(&mut bytes, "synthetic subject", 80);
    append_field(
        &mut bytes,
        &format!("ResMed acceptance SRN={recording_serial}"),
        80,
    );
    append_field(&mut bytes, "02.01.26", 8);
    append_field(&mut bytes, "22.00.00", 8);
    append_field(&mut bytes, &HEADER_BYTES.to_string(), 8);
    append_field(&mut bytes, "", 44);
    append_field(&mut bytes, &RECORD_COUNT.to_string(), 8);
    append_field(&mut bytes, "1", 8);
    append_field(&mut bytes, &SIGNAL_COUNT.to_string(), 4);

    append_field(&mut bytes, "Flow", 16);
    append_field(&mut bytes, "", 80);
    append_field(&mut bytes, "L/s", 8);
    append_field(&mut bytes, "-2", 8);
    append_field(&mut bytes, "2", 8);
    append_field(&mut bytes, "-100", 8);
    append_field(&mut bytes, "100", 8);
    append_field(&mut bytes, "", 80);
    append_field(&mut bytes, &SAMPLES_PER_RECORD.to_string(), 8);
    append_field(&mut bytes, "", 32);
    assert_eq!(bytes.len(), HEADER_BYTES);

    for sample in [-100_i16, 0, 50, 100] {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn append_field(bytes: &mut Vec<u8>, value: &str, width: usize) {
    assert!(value.is_ascii(), "EDF fixture fields must be ASCII");
    assert!(value.len() <= width, "EDF field exceeds fixed width");
    bytes.extend_from_slice(value.as_bytes());
    bytes.resize(bytes.len() + width - value.len(), b' ');
}

fn assert_opaque_sha256(value: &str) {
    let digest = value
        .strip_prefix("sha256:")
        .expect("opaque key must use the SHA-256 scheme");
    assert_eq!(digest.len(), 64);
    assert!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "opaque key must be lowercase hexadecimal"
    );
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, SnapshotEntry> {
    fn visit(root: &Path, current: &Path, entries: &mut BTreeMap<PathBuf, SnapshotEntry>) {
        let mut children = fs::read_dir(current)
            .unwrap_or_else(|error| panic!("read {}: {error}", current.display()))
            .map(|entry| entry.expect("read fixture entry").path())
            .collect::<Vec<_>>();
        children.sort_by(|left, right| {
            left.file_name()
                .unwrap_or_else(|| OsStr::new(""))
                .cmp(right.file_name().unwrap_or_else(|| OsStr::new("")))
        });

        for path in children {
            let relative = path.strip_prefix(root).expect("fixture-relative path");
            if path.is_dir() {
                entries.insert(relative.to_owned(), SnapshotEntry::Directory);
                visit(root, &path, entries);
            } else {
                entries.insert(
                    relative.to_owned(),
                    SnapshotEntry::File(
                        fs::read(&path)
                            .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
                    ),
                );
            }
        }
    }

    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

#[tokio::main]
async fn main() {
    BrpWorld::run(format!(
        "{}/features/resmed_brp_import.feature",
        env!("CARGO_MANIFEST_DIR")
    ))
    .await;
}
