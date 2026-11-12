//! `ui-cook <page.html> <out.ui.ron>`: compiles a page into a UI document.

use std::process::ExitCode;

fn run(input: &str, output: &str) -> Result<(), String> {
    let html = std::fs::read_to_string(input).map_err(|e| format!("{input}: {e}"))?;
    let ron = ui_cook::cook(&html).map_err(|e| format!("{input}: {e}"))?;
    std::fs::write(output, ron).map_err(|e| format!("{output}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: ui-cook <page.html> <out.ui.ron>");
        return ExitCode::from(2);
    };
    match run(input, output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
