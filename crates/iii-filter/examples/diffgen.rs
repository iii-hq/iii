//! Differential test corpus: random valid filters and documents, with the
//! Rust evaluator's verdict. Deterministic per seed.
//!
//! ```text
//! cargo run -p iii-filter --example diffgen -- --seed 1 --count 1000
//! ```
//!
//! Prints `{"seed":N,"cases":[{"filters":…,"doc":…,"expect":bool}]}`. The
//! filters are given as generated (not normalized), so the other
//! implementation validates and evaluates them from scratch. Each case first
//! draws the outcome (match or not, 50/50), then a filter and a document the
//! Rust evaluator gives that outcome for.

#[path = "../tests/support/gen.rs"]
mod generator;

use std::process::ExitCode;

fn main() -> ExitCode {
    let (seed, count) = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\nusage: diffgen [--seed N] [--count M]");
            return ExitCode::from(2);
        }
    };
    println!("{}", generator::diff_run(seed, count));
    ExitCode::SUCCESS
}

fn parse_args() -> Result<(u64, usize), String> {
    let (mut seed, mut count) = (1u64, 100usize);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().ok_or_else(|| format!("{arg} needs a value"))?;
        match arg.as_str() {
            "--seed" => seed = value.parse().map_err(|e| format!("--seed: {e}"))?,
            "--count" => count = value.parse().map_err(|e| format!("--count: {e}"))?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok((seed, count))
}
