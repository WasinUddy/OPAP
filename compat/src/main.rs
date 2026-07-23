use opap_oscar_compat::{DEFAULT_FLOAT_TOLERANCES, HarnessError, compare, load_and_validate};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Difference) => ExitCode::from(1),
        Err(CliError::Usage(message)) => {
            eprintln!("{message}\n\n{}", usage());
            ExitCode::from(2)
        }
        Err(CliError::Harness(error)) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

enum CliError {
    Difference,
    Usage(String),
    Harness(HarnessError),
}

impl From<HarnessError> for CliError {
    fn from(value: HarnessError) -> Self {
        Self::Harness(value)
    }
}

fn run(args: Vec<String>) -> Result<(), CliError> {
    match args.as_slice() {
        [command, path] if command == "validate" => {
            let manifest = load_and_validate(path, "input")?;
            println!(
                "valid: schema {}, producer {} at {}, oracle {}",
                manifest.schema_version,
                manifest.producer.name,
                manifest.producer.source_revision,
                manifest.oracle.revision,
            );
            Ok(())
        }
        [command, expected_path, actual_path] if command == "compare" => {
            let expected = load_and_validate(expected_path, "expected")?;
            let actual = load_and_validate(actual_path, "actual")?;
            let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES)?;
            if report.compatible {
                println!(
                    "compatible within manifest scope: no differences; this is not a full OSCAR parity claim"
                );
                Ok(())
            } else {
                eprintln!("incompatible: {} difference(s)", report.differences.len());
                for difference in report.differences {
                    eprintln!(
                        "  {} [{:?}]: {}",
                        difference.path, difference.kind, difference.message
                    );
                }
                Err(CliError::Difference)
            }
        }
        [command] if command == "tolerances" => {
            for (name, value) in DEFAULT_FLOAT_TOLERANCES.named() {
                println!("{name}={value}");
            }
            Ok(())
        }
        [] => Err(CliError::Usage("missing command".to_owned())),
        _ => Err(CliError::Usage("invalid arguments".to_owned())),
    }
}

fn usage() -> &'static str {
    "Usage:\n  oscar-diff validate MANIFEST.json\n  oscar-diff compare EXPECTED.json ACTUAL.json\n  oscar-diff tolerances"
}
