use serde_json::Value;
use std::fs::File;
use std::process::Command;
use tempfile::NamedTempFile;

use opap_oscar_compat::{
    event_collection_sha256, session_aggregate_sha256, waveform_collection_sha256,
};

const OSCAR: &str = "tests/fixtures/synthetic-oscar.json";
const OPAP: &str = "tests/fixtures/synthetic-opap.json";

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oscar-diff"))
}

fn mutated_fixture(mutate: impl FnOnce(&mut Value)) -> NamedTempFile {
    let mut value: Value = serde_json::from_reader(File::open(OPAP).unwrap()).unwrap();
    mutate(&mut value);
    let file = NamedTempFile::new().unwrap();
    serde_json::to_writer_pretty(file.as_file(), &value).unwrap();
    file
}

fn refresh_aggregate_digests(value: &mut Value) {
    let event_channels = value["sessions"][0]["events"]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|channel| {
            (
                channel["channel_id"].as_str().unwrap().to_owned(),
                channel["sha256"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let events_sha256 = event_collection_sha256(
        event_channels
            .iter()
            .map(|(channel, digest)| (channel.as_str(), digest.as_str())),
    );
    value["sessions"][0]["events"]["sha256"] = Value::String(events_sha256.clone());

    let waveform_channels = value["sessions"][0]["waveforms"]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|channel| {
            (
                channel["channel_id"].as_str().unwrap().to_owned(),
                channel["source_sha256"].as_str().unwrap().to_owned(),
                channel["sha256"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let waveforms_sha256 =
        waveform_collection_sha256(waveform_channels.iter().map(|(channel, source, semantic)| {
            (channel.as_str(), source.as_str(), semantic.as_str())
        }));
    value["sessions"][0]["waveforms"]["sha256"] = Value::String(waveforms_sha256.clone());

    let session = &value["sessions"][0];
    let session_sha256 = session_aggregate_sha256(
        session["source_id_sha256"].as_str().unwrap(),
        session["source_sha256"].as_str().unwrap(),
        session["slices"]["sha256"].as_str().unwrap(),
        session["summary"]["source_sha256"].as_str().unwrap(),
        session["settings"]["sha256"].as_str().unwrap(),
        &events_sha256,
        &waveforms_sha256,
    );
    value["sessions"][0]["sha256"] = Value::String(session_sha256);
}

#[test]
fn cli_accepts_the_synthetic_differential_pair() {
    let output = command().args(["compare", OSCAR, OPAP]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("compatible"));
}

#[test]
fn cli_fails_on_an_exact_digest_mismatch() {
    let actual = mutated_fixture(|value| {
        value["sessions"][0]["waveforms"]["channels"][0]["sha256"] = Value::String("a".repeat(64));
        refresh_aggregate_digests(value);
    });
    let output = command()
        .args(["compare", OSCAR, actual.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("waveforms.channels[expected:0].sha256"),
        "{stderr}"
    );
}

#[test]
fn cli_rejects_a_missing_digest() {
    let actual = mutated_fixture(|value| {
        value["sessions"][0]
            .as_object_mut()
            .unwrap()
            .remove("sha256");
    });
    let output = command()
        .args(["validate", actual.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid JSON manifest"));
}

#[test]
fn cli_rejects_an_empty_session_list() {
    let actual = mutated_fixture(|value| value["sessions"] = Value::Array(Vec::new()));
    let output = command()
        .args(["validate", actual.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("empty export is never a pass"));
}

#[test]
fn cli_reports_an_expected_channel_missing_from_a_valid_empty_collection() {
    let actual = mutated_fixture(|value| {
        value["sessions"][0]["events"]["channel_count"] = Value::from(0);
        value["sessions"][0]["events"]["channels"] = Value::Array(Vec::new());
        refresh_aggregate_digests(value);
    });
    let output = command()
        .args(["compare", OSCAR, actual.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("events.channels[expected:0]"));
}

#[test]
fn cli_rejects_oracle_self_comparison() {
    let output = command().args(["compare", OSCAR, OSCAR]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires role Subject"));
}

#[test]
fn cli_redacts_sensitive_manifest_values() {
    let actual = mutated_fixture(|value| {
        value["machine"]["serial_number"] = Value::String("SYNTHETIC-SECRET-VALUE".to_owned());
    });
    let output = command()
        .args(["compare", OSCAR, actual.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("machine.serial_number"));
    assert!(!stderr.contains("SYNTHETIC-SECRET-VALUE"));
}

#[test]
fn cli_redacts_private_paths_in_read_errors() {
    let private_path = "/definitely-not-present/SYNTHETIC-PRIVATE-NAME/oscar.json";
    let output = command().args(["validate", private_path]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot read manifest"));
    assert!(!stderr.contains("SYNTHETIC-PRIVATE-NAME"));
}

#[test]
fn cli_redacts_values_from_parse_and_contract_errors() {
    let malformed = mutated_fixture(|value| {
        value["sessions"][0]["settings"]["count"] =
            Value::String("SYNTHETIC-PRIVATE-PARSE-VALUE".to_owned());
    });
    let output = command()
        .args(["validate", malformed.path().to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2));
    assert!(!stderr.contains("SYNTHETIC-PRIVATE-PARSE-VALUE"));

    let wrong_contract = mutated_fixture(|value| {
        value["producer"]["name"] = Value::String("SYNTHETIC-PRIVATE-CONTRACT-VALUE".to_owned());
    });
    let output = command()
        .args(["validate", wrong_contract.path().to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2));
    assert!(!stderr.contains("SYNTHETIC-PRIVATE-CONTRACT-VALUE"));
}
