

watch:
	bacon run

watch-debug:
	cargo watch -w src -s "RUST_LOG=debug cargo run "

