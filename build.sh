if [ "$1" == "linux" ]; then
  echo "Building for Linux"
  cargo build --release --target x86_64-unknown-linux-gnu
  mkdir -p .bin
  rm .bin/iii || true
  mv target/x86_64-unknown-linux-gnu/release/iii .bin/iii
else
  echo "Building for current platform"
  cargo build --release
  rm .bin/iii || true
  mv target/release/iii .bin/iii
fi
