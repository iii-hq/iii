cargo build --release
rm packages/node/motia/iii || true
mv target/release/iii packages/node/motia/iii

cargo build --release --target x86_64-unknown-linux-gnu
rm packages/node/motia-example/iii || true
mv target/x86_64-unknown-linux-gnu/release/iii packages/node/motia-example/iii