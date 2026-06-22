//! One-time generator for the embedded golden upper image.
//!
//! Reads a raw (sparse) empty ext4 image and emits the compressed
//! sparse-extent format consumed by `cli::upper` at runtime. Driven by
//! `crates/iii-worker/vendor/regen-golden.sh`; not built into the shipped
//! binary.
//!
//! Usage: gen-golden <raw.ext4> <out.iiu.gz>

use iii_worker::cli::upper;
use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let (raw, out) = match (args.next(), args.next()) {
        (Some(raw), Some(out)) => (raw, out),
        _ => {
            eprintln!("usage: gen-golden <raw.ext4> <out.iiu.gz>");
            std::process::exit(2);
        }
    };

    match upper::build_golden(Path::new(&raw), Path::new(&out)) {
        Ok(stats) => {
            let on_disk = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
            eprintln!(
                "gen-golden: {} -> {}\n  logical {:.1} GiB, {} non-zero {}-byte blocks kept, artifact {:.2} MiB",
                raw,
                out,
                stats.total_len as f64 / (1u64 << 30) as f64,
                stats.blocks_kept,
                stats.block_size,
                on_disk as f64 / (1u64 << 20) as f64,
            );
        }
        Err(e) => {
            eprintln!("gen-golden: failed: {e}");
            std::process::exit(1);
        }
    }
}
