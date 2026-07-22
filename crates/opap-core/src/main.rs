use opap_core::resmed::{detect_card, read_machine_info};
use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    let command = arguments.next().ok_or_else(usage)?;
    let path = arguments.next().ok_or_else(usage)?;
    if arguments.next().is_some() {
        return Err(usage());
    }

    match command.as_str() {
        "detect" => {
            println!("{}", detect_card(Path::new(&path)));
            Ok(())
        }
        "machine-info" => {
            let info = read_machine_info(Path::new(&path)).map_err(|error| error.to_string())?;
            let json = serde_json::to_string_pretty(&info).map_err(|error| error.to_string())?;
            println!("{json}");
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage: opap-core <detect|machine-info> <resmed-card-path>".to_owned()
}
