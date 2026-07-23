use jsonschema::{Draft, JSONSchema};
use opap_oscar_compat::load_and_validate;
use serde_json::{Value, json};

const OSCAR: &str = include_str!("fixtures/synthetic-oscar.json");
const OPAP: &str = include_str!("fixtures/synthetic-opap.json");
const SCHEMA: &str = include_str!("../schema/opap-compat-manifest.schema.json");

fn validator() -> JSONSchema {
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .unwrap()
}

#[test]
fn draft_2020_12_schema_accepts_both_public_fixtures() {
    let validator = validator();
    for raw in [OSCAR, OPAP] {
        let manifest: Value = serde_json::from_str(raw).unwrap();
        assert!(validator.is_valid(&manifest));
    }
}

#[test]
fn schema_rejects_contract_drift_and_unattested_real_cards() {
    let validator = validator();
    let original: Value = serde_json::from_str(OSCAR).unwrap();

    for mutation in [
        ("unknown field", json!(true)),
        ("bad identifier", json!("Bad/ID")),
        ("fractional count", json!(1.25)),
        ("unattested real card", json!(false)),
    ] {
        let mut manifest = original.clone();
        match mutation.0 {
            "unknown field" => manifest["unexpected"] = mutation.1,
            "bad identifier" => manifest["sessions"][0]["session_id"] = mutation.1,
            "fractional count" => manifest["sessions"][0]["settings"]["count"] = mutation.1,
            "unattested real card" => manifest["fixture"]["synthetic"] = mutation.1,
            _ => unreachable!(),
        }
        assert!(!validator.is_valid(&manifest), "{}", mutation.0);
    }
}

#[test]
fn schema_and_loader_accept_exact_integral_spellings() {
    let validator = validator();
    for spelling in ["3.0", "3e0"] {
        let raw = OSCAR.replacen("\"count\": 3", &format!("\"count\": {spelling}"), 1);
        let value: Value = serde_json::from_str(&raw).unwrap();
        assert!(validator.is_valid(&value), "schema rejected {spelling}");

        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), raw).unwrap();
        assert!(
            load_and_validate(file.path(), "fixture").is_ok(),
            "loader rejected {spelling}"
        );
    }
}

#[test]
fn schema_allows_null_event_payloads_and_explicit_empty_collections() {
    let validator = validator();
    let mut manifest: Value = serde_json::from_str(OSCAR).unwrap();
    manifest["sessions"][0]["events"]["channels"][0]["items"][0]["duration_milliseconds"] =
        Value::Null;
    manifest["sessions"][0]["events"]["channels"][0]["items"][0]["value"] = Value::Null;
    manifest["sessions"][0]["events"]["channels"][0]["count"] = json!(0);
    manifest["sessions"][0]["events"]["channels"][0]["items"] = json!([]);
    manifest["sessions"][0]["summary"]["metric_count"] = json!(0);
    manifest["sessions"][0]["summary"]["metrics"] = json!([]);
    manifest["sessions"][0]["settings"]["count"] = json!(0);
    manifest["sessions"][0]["settings"]["items"] = json!([]);
    assert!(validator.is_valid(&manifest));
}
