use cucumber::{World as _, given, then, when};
use opap_core::resmed::{self, Error, MachineInfo};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::{TempDir, tempdir};

const LEGACY_SERIAL: &str = "SYNTHETIC-TGT-0001";
const LEGACY_MODEL_NUMBER: &str = "SYN-TGT-10";
const JSON_SERIAL: &str = "SYNTHETIC-JSON-0011";
const JSON_MODEL_NUMBER: &str = "SYN-JSON-11";

#[derive(Debug, Clone, PartialEq, Eq)]
enum SnapshotEntry {
    Directory,
    File(Vec<u8>),
}

#[derive(Debug)]
struct CliResult {
    detected: bool,
    machine_info: Option<MachineInfo>,
    machine_info_error: Option<String>,
}

#[derive(Debug, Default, cucumber::World)]
struct ResmedWorld {
    fixture: Option<TempDir>,
    fixture_path: Option<PathBuf>,
    original_contents: Option<BTreeMap<PathBuf, SnapshotEntry>>,
    library_detected: Option<bool>,
    library_machine_info: Option<MachineInfo>,
    library_error: Option<Error>,
    cli_result: Option<CliResult>,
}

impl ResmedWorld {
    fn card_path(&self) -> &Path {
        self.fixture_path
            .as_deref()
            .expect("a synthetic card fixture must exist")
    }
}

#[given("an empty synthetic card directory")]
fn empty_synthetic_card(world: &mut ResmedWorld) {
    install_fixture(world, false);
}

#[given("a synthetic directory with a valid ResMed card structure")]
fn valid_synthetic_card(world: &mut ResmedWorld) {
    install_fixture(world, true);
}

#[given("legacy TGT identification is present")]
fn legacy_identification(world: &mut ResmedWorld) {
    fs::write(
        world.card_path().join("Identification.tgt"),
        format!("#SRN {LEGACY_SERIAL}\n#PNA AirSense_10_AutoSet\n#PCD {LEGACY_MODEL_NUMBER}\n"),
    )
    .expect("write synthetic Identification.tgt");
}

#[given("JSON identification is present")]
fn json_identification(world: &mut ResmedWorld) {
    let json = serde_json::json!({
        "FlowGenerator": {
            "IdentificationProfiles": {
                "Product": {
                    "SerialNumber": JSON_SERIAL,
                    "ProductCode": JSON_MODEL_NUMBER,
                    "ProductName": "AirSense11 AutoSet"
                }
            }
        }
    });
    fs::write(
        world.card_path().join("Identification.json"),
        serde_json::to_vec_pretty(&json).expect("serialize synthetic identification"),
    )
    .expect("write synthetic Identification.json");
}

#[given("the original synthetic card contents are recorded")]
fn record_original_contents(world: &mut ResmedWorld) {
    world.original_contents = Some(snapshot(world.card_path()));
}

#[when("OPAP detects and identifies the card")]
fn inspect_card(world: &mut ResmedWorld) {
    let card_path = world.card_path().to_owned();

    world.library_detected = Some(resmed::detect_card(&card_path));
    match resmed::read_machine_info(&card_path) {
        Ok(info) => world.library_machine_info = Some(info),
        Err(error) => world.library_error = Some(error),
    }
    world.cli_result = Some(inspect_with_cli(&card_path));
}

#[then("the card is detected as ResMed")]
fn detected_as_resmed(world: &mut ResmedWorld) {
    assert_eq!(world.library_detected, Some(true));
}

#[then("the card is not detected as ResMed")]
fn not_detected_as_resmed(world: &mut ResmedWorld) {
    assert_eq!(world.library_detected, Some(false));
}

#[then("the legacy machine identity is returned")]
fn legacy_identity_returned(world: &mut ResmedWorld) {
    assert_eq!(
        world.library_machine_info.as_ref(),
        Some(&MachineInfo {
            brand: "ResMed".to_owned(),
            model: "AirSense 10 AutoSet".to_owned(),
            model_number: LEGACY_MODEL_NUMBER.to_owned(),
            serial: LEGACY_SERIAL.to_owned(),
            series: "AirSense 10".to_owned(),
        })
    );
}

#[then("the JSON machine identity is returned")]
fn json_identity_returned(world: &mut ResmedWorld) {
    assert_eq!(
        world.library_machine_info.as_ref(),
        Some(&MachineInfo {
            brand: "ResMed".to_owned(),
            model: "AirSense11 AutoSet".to_owned(),
            model_number: JSON_MODEL_NUMBER.to_owned(),
            serial: JSON_SERIAL.to_owned(),
            series: "AirSense 11".to_owned(),
        })
    );
}

#[then("identification fails because the card is invalid")]
fn invalid_card_error(world: &mut ResmedWorld) {
    assert!(matches!(world.library_error, Some(Error::NotResmedCard(_))));
}

#[then("identification fails because machine identity is missing")]
fn missing_identity_error(world: &mut ResmedWorld) {
    assert!(matches!(
        world.library_error,
        Some(Error::MissingIdentification(_))
    ));
}

#[then("the CLI and library results agree")]
fn cli_and_library_agree(world: &mut ResmedWorld) {
    let cli = world.cli_result.as_ref().expect("CLI result");
    assert_eq!(
        cli.detected,
        world.library_detected.expect("library result")
    );

    match (&world.library_machine_info, &world.library_error) {
        (Some(info), None) => {
            assert_eq!(cli.machine_info.as_ref(), Some(info));
            assert_eq!(cli.machine_info_error, None);
        }
        (None, Some(error)) => {
            let stderr = cli
                .machine_info_error
                .as_deref()
                .expect("the CLI should report the library error");
            assert!(
                stderr.contains(&error.to_string()),
                "CLI error `{stderr}` did not contain `{error}`"
            );
            assert_eq!(cli.machine_info, None);
        }
        state => panic!("inconsistent library result: {state:?}"),
    }
}

#[then("the fixture is outside the source repository")]
fn fixture_is_outside_repository(world: &mut ResmedWorld) {
    let fixture = world
        .card_path()
        .canonicalize()
        .expect("canonical fixture path");
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical repository path");

    assert!(
        !fixture.starts_with(&repository),
        "synthetic fixture unexpectedly exists inside {}",
        repository.display()
    );
}

#[then("inspecting the card leaves its contents unchanged")]
fn card_contents_unchanged(world: &mut ResmedWorld) {
    let expected = world
        .original_contents
        .as_ref()
        .expect("original card snapshot");
    assert_eq!(&snapshot(world.card_path()), expected);
}

#[then("destroying the fixture removes its synthetic data")]
fn fixture_is_destroyed(world: &mut ResmedWorld) {
    let fixture = world.fixture.take().expect("temporary fixture");
    let path = fixture.path().to_owned();
    fixture.close().expect("remove temporary fixture");
    assert!(!path.exists(), "temporary fixture was not removed");
}

fn install_fixture(world: &mut ResmedWorld, valid_card_structure: bool) {
    let fixture = tempdir().expect("create synthetic temporary card");
    if valid_card_structure {
        fs::create_dir(fixture.path().join("DATALOG")).expect("create DATALOG directory");
        fs::write(fixture.path().join("STR.edf"), []).expect("write synthetic STR.edf");
    }
    world.fixture_path = Some(fixture.path().to_owned());
    world.fixture = Some(fixture);
}

fn inspect_with_cli(card_path: &Path) -> CliResult {
    let detect = run_cli("detect", card_path);
    assert!(
        detect.status.success(),
        "detect command failed: {}",
        String::from_utf8_lossy(&detect.stderr)
    );
    let detected = String::from_utf8(detect.stdout)
        .expect("detect output is UTF-8")
        .trim()
        .parse::<bool>()
        .expect("detect output is boolean");

    let machine_info = run_cli("machine-info", card_path);
    if machine_info.status.success() {
        CliResult {
            detected,
            machine_info: Some(
                serde_json::from_slice(&machine_info.stdout)
                    .expect("machine-info output is MachineInfo JSON"),
            ),
            machine_info_error: None,
        }
    } else {
        CliResult {
            detected,
            machine_info: None,
            machine_info_error: Some(
                String::from_utf8(machine_info.stderr).expect("machine-info error is UTF-8"),
            ),
        }
    }
}

fn run_cli(command: &str, card_path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_opap-core-acceptance-cli"))
        .arg(command)
        .arg(card_path)
        .output()
        .unwrap_or_else(|error| panic!("run opap-core {command}: {error}"))
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
    ResmedWorld::run(format!("{}/features", env!("CARGO_MANIFEST_DIR"))).await;
}
