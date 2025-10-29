fn main() {
    tonic_build::configure()
        .compile_protos(&["proto/engine.proto"], &["proto"])
        .unwrap();
}
