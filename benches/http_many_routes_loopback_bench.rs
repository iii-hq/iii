mod common;
mod http_blackbox;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use http_blackbox::BenchRuntime;
use tokio::runtime::Runtime;

fn http_many_routes_loopback_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("http_many_routes_loopback");

    for route_count in common::route_counts() {
        let runtime = rt.block_on(BenchRuntime::start(route_count));
        let target_path = common::http_api_path(route_count - 1);

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(route_count),
            &route_count,
            |b, _| {
                b.to_async(&rt).iter(|| async {
                    let response = runtime
                        .post_json(&target_path, &common::http_request_body())
                        .await;
                    assert!(response.status().is_success());
                });
            },
        );

        rt.block_on(runtime.shutdown());
    }

    group.finish();
}

criterion_group!(benches, http_many_routes_loopback_benchmark);
criterion_main!(benches);
