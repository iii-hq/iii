mod common;

use criterion::{Criterion, criterion_group, criterion_main};
use iii::{EngineBuilder, modules::config::EngineConfig};

fn startup_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("create tokio runtime");

    let config = common::write_minimal_config_file();
    c.bench_function(
        "startup/build_and_destroy_engine_from_minimal_config",
        |b| {
            b.to_async(&rt).iter(|| async {
                let path = config.path().to_string_lossy().to_string();
                let config = EngineConfig::config_file(&path).unwrap();

                let builder = EngineBuilder::new()
                    .with_config(config)
                    .build()
                    .await
                    .expect("build engine during benchmark");

                builder
                    .destroy()
                    .await
                    .expect("destroy engine after benchmark");
            });
        },
    );
}

criterion_group!(benches, startup_benchmark);
criterion_main!(benches);
