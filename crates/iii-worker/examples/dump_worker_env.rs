//! Quick demo: dumps the env hashmap that `iii worker start <name>`
//! would inject into the sandbox VM for a given worker, after
//! `${VAR}` / `${VAR:default}` expansion. Run from a directory
//! containing `config.yaml`:
//!
//!     ANTHROPIC_API_KEY=sk-real \
//!       cargo run -p iii-worker --example dump_worker_env -- ai-worker

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: dump_worker_env <worker-name>");
        std::process::exit(2);
    });
    let env = iii_worker::cli::config_file::get_worker_config_as_env(&name);
    if env.is_empty() {
        eprintln!("(empty — worker not found, or expansion failed; see stderr above)");
        std::process::exit(1);
    }
    let mut keys: Vec<_> = env.keys().collect();
    keys.sort();
    for k in keys {
        println!("{}={}", k, env[k]);
    }
}
