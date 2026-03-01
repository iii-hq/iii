
watch-debug:
	cargo watch -w Cargo.toml -w config.yaml -w src -s "RUST_LOG=debug cargo run "

bench-all:
	cargo bench --benches
