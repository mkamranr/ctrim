//! Throughput of the whole pipeline on representative input.
//!
//! Run with `cargo bench`. The 100MB target from the design docs is checked
//! separately with `scripts/bench-large.sh`, which also measures RSS.

use std::io::Cursor;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ctrim::{pipeline, Config, OutputFormat};

/// A mixed log: retry spam, a vendor-heavy traceback, and ordinary output.
fn synthetic_log(repeats: usize) -> String {
    let mut out = String::new();
    for i in 0..repeats {
        out.push_str(&format!(
            "2024-05-01T10:00:{:02}Z level=info msg=\"handling request {}\"\n",
            i % 60,
            i
        ));
        for _ in 0..8 {
            out.push_str("\x1b[33mWARN\x1b[0m retrying upstream connection, backing off\n");
        }
        out.push_str("Traceback (most recent call last):\n");
        out.push_str(
            "  File \"app/main.py\", line 42, in handler\n    return service.run(payload)\n",
        );
        for frame in 0..10 {
            out.push_str(&format!(
                "  File \"/srv/.venv/lib/python3.11/site-packages/pkg{frame}/mod.py\", line {frame}, in call\n    return next_call()\n"
            ));
        }
        out.push_str("ValueError: bad payload\n");
    }
    out
}

fn bench_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline");
    for repeats in [50usize, 500] {
        let input = synthetic_log(repeats);
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KiB", input.len() / 1024)),
            &input,
            |b, input| {
                let config = Config {
                    format: OutputFormat::Raw,
                    ..Config::default()
                };
                b.iter(|| {
                    let mut sink = Vec::with_capacity(input.len() / 4);
                    pipeline::run(Cursor::new(input.as_bytes()), &mut sink, &config).unwrap()
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_pipeline);
criterion_main!(benches);
