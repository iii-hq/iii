cargo build --release
rm .bin/iii || true
mv target/release/iii .bin/iii

cargo build --release --target x86_64-unknown-linux-gnu
rm packages/docker/bun/iii || true
mv target/x86_64-unknown-linux-gnu/release/iii packages/docker/bun/iii