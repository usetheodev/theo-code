//! Criterion benchmark for `StreamingMarkdownRenderer`.
//!
//! Verifies the T1.4b DoD: per-chunk latency < 1 ms, feeding
//! 100K characters in reasonable time, and that chunk-size
//! variation does not cause latency blow-up.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

#[path = "../src/render/style.rs"]
#[allow(dead_code, clippy::all)]
pub mod style;

#[path = "../src/render/code_block.rs"]
#[allow(dead_code, clippy::all)]
pub mod code_block;

// Shim: `streaming.rs` references `crate::render::{code_block, style}`,
// so expose them under a `render` module at the crate root.
mod render {
    pub use super::code_block;
    pub use super::style;
}

#[path = "../src/render/streaming.rs"]
#[allow(dead_code, clippy::all)]
mod streaming_under_bench;

use style::StyleCaps;
use streaming_under_bench::{StreamingMarkdownRenderer, render_complete};

fn plain() -> StyleCaps {
    StyleCaps::plain()
}

fn bench_single_chunk(c: &mut Criterion) {
    let input = "hello **world** this is `code` and more text".repeat(20);
    let mut group = c.benchmark_group("streaming_single_chunk");
    group.throughput(Throughput::Bytes(input.len() as u64));
    group.bench_function("push_all_at_once", |b| {
        b.iter(|| {
            let out = render_complete(black_box(&input), plain());
            black_box(out);
        });
    });
    group.finish();
}

fn bench_chunk_sizes(c: &mut Criterion) {
    let input = "hello **world** `code` more text ".repeat(10);
    let mut group = c.benchmark_group("streaming_chunk_size");
    group.throughput(Throughput::Bytes(input.len() as u64));
    for &size in &[1usize, 4, 16, 64, 256] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &chunk_size| {
            b.iter(|| {
                let mut r = StreamingMarkdownRenderer::new(plain());
                let bytes = input.as_bytes();
                let mut i = 0;
                while i < bytes.len() {
                    let mut end = (i + chunk_size).min(bytes.len());
                    while end > i && !input.is_char_boundary(end) {
                        end -= 1;
                    }
                    if end == i {
                        end = i + 1;
                    }
                    r.push(&input[i..end]);
                    i = end;
                }
                r.flush();
                black_box(r.take_output());
            });
        });
    }
    group.finish();
}

fn bench_100k_chars(c: &mut Criterion) {
    let input = "a".repeat(100_000);
    let mut group = c.benchmark_group("streaming_100k_plain");
    group.throughput(Throughput::Bytes(input.len() as u64));
    group.sample_size(20);
    group.bench_function("push_all", |b| {
        b.iter(|| {
            let _ = render_complete(black_box(&input), plain());
        });
    });
    group.finish();
}

fn bench_per_chunk_latency(c: &mut Criterion) {
    // Measures the latency of a single push() call with a realistic
    // chunk size (32 chars). The T1.4b DoD requires < 1 ms / chunk.
    let chunk = "some streaming text, **bold** and `code`".to_string();
    let mut group = c.benchmark_group("streaming_per_chunk");
    group.throughput(Throughput::Bytes(chunk.len() as u64));
    group.bench_function("single_push", |b| {
        let mut r = StreamingMarkdownRenderer::new(plain());
        b.iter(|| {
            r.push(black_box(&chunk));
            let _ = r.take_output();
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_single_chunk,
    bench_chunk_sizes,
    bench_100k_chars,
    bench_per_chunk_latency,
);
criterion_main!(benches);
