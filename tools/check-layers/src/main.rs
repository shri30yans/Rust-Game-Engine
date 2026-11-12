//! Entry point for the layer gate. Run from the repository root with
//! `cargo run -p check-layers`. Exits 0 when every workspace crate obeys
//! `layers.json`, 1 when there are violations, and 2 when the check itself
//! could not run.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("layers.json");
    match check_layers::run(&config) {
        Ok(violations) if violations.is_empty() => {
            println!("check-layers: all workspace crates obey layers.json");
            ExitCode::SUCCESS
        }
        Ok(violations) => {
            for violation in &violations {
                eprintln!("error: {violation}");
            }
            eprintln!("check-layers: {} violation(s)", violations.len());
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("check-layers: {err}");
            ExitCode::from(2)
        }
    }
}
