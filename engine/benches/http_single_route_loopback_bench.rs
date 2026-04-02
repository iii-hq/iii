mod common;
mod http_blackbox;

use criterion::{Criterion, criterion_group, criterion_main};
use http_blackbox::BenchRuntime;
use tokio::runtime::Runtime;

fn http_single_route_loopback_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let runtime = rt.block_on(BenchRuntime::start(1));

    c.bench_function("http_single_route_loopback/post_json", |b| {
        b.to_async(&rt).iter(|| async {
            let response = runtime
                .post_json(&common::http_api_path(0), &common::http_request_body())
                .await;
            assert!(response.status().is_success());
        });
    });

    rt.block_on(runtime.shutdown());
}

criterion_group!(benches, http_single_route_loopback_benchmark);
criterion_main!(benches);
