//! Command-line entry point for deterministic cooked asset generation.
#![forbid(unsafe_code)]

fn main() {
    if let Err(error) = cook::run_from_args(std::env::args_os().skip(1)) {
        eprintln!("cook: {error}");
        std::process::exit(1);
    }
}
