mod common;
mod http_blackbox;

use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use futures_util::future::join_all;
use http_blackbox::BenchRuntime;
use tokio::runtime::Runtime;

fn http_concurrency_loopback_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("http_concurrency_loopback");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(3));

    for concurrency in common::http_concurrency_levels() {
        let runtime = rt.block_on(BenchRuntime::start(100));
        let target_path = common::http_api_path(99);

        group.throughput(Throughput::Elements(concurrency as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrency),
            &concurrency,
            |b, &concurrency| {
                b.to_async(&rt).iter_custom(|iters| {
                    let runtime = &runtime;
                    let target_path = &target_path;
                    async move {
                        let start = Instant::now();
                        for _ in 0..iters {
                            let requests = (0..concurrency).map(|_| async {
                                let _response = runtime
                                    .post_json(target_path, &common::http_request_body())
                                    .await;
                            });
                            join_all(requests).await;
                        }
                        start.elapsed()
                    }
                });
            },
        );

        rt.block_on(runtime.shutdown());
    }

    group.finish();
}

criterion_group!(benches, http_concurrency_loopback_benchmark);
criterion_main!(benches);
