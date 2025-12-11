cargo build --release
rm packages/node/motia/iii
mv target/release/iii packages/node/motia/iii

cargo build --release --target x86_64-unknown-linux-gnu
rm packages/node/motia-example/iii
mv target/x86_64-unknown-linux-gnu/release/iii packages/node/motia-example/iii