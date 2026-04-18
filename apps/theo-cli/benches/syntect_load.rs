//! Criterion benchmark for syntect loading + highlighting.
//!
//! Verifies the T1.3 DoD: `SyntaxSet` + `ThemeSet` load < 50 ms,
//! and per-language highlight is fast enough for real-time use.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

#[path = "../src/render/style.rs"]
#[allow(dead_code, clippy::all)]
pub mod style;

mod render {
    pub use super::style;
}

#[path = "../src/render/code_block.rs"]
#[allow(dead_code, clippy::all)]
mod code_block_under_bench;

use code_block_under_bench::{highlight, render_block, syntax_set, theme_set};
use style::StyleCaps;

fn bench_syntax_set_cold_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("syntect_cold_load");
    // Small sample size — this is a once-per-process cost.
    group.sample_size(10);
    group.bench_function("SyntaxSet::load_defaults_newlines", |b| {
        b.iter(|| {
            let s = syntect::parsing::SyntaxSet::load_defaults_newlines();
            black_box(s);
        });
    });
    group.finish();
}

fn bench_theme_set_cold_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("syntect_theme_load");
    group.sample_size(10);
    group.bench_function("ThemeSet::load_defaults", |b| {
        b.iter(|| {
            let t = syntect::highlighting::ThemeSet::load_defaults();
            black_box(t);
        });
    });
    group.finish();
}

fn bench_lazy_access(c: &mut Criterion) {
    // Prime the OnceLock
    let _ = syntax_set();
    let _ = theme_set();
    let mut group = c.benchmark_group("syntect_lazy_access");
    group.bench_function("syntax_set_repeat", |b| {
        b.iter(|| {
            let s = syntax_set();
            black_box(s);
        });
    });
    group.bench_function("theme_set_repeat", |b| {
        b.iter(|| {
            let t = theme_set();
            black_box(t);
        });
    });
    group.finish();
}

fn bench_highlight_languages(c: &mut Criterion) {
    let samples: &[(&str, &str)] = &[
        ("rust", "fn main() { println!(\"hi\"); }"),
        ("python", "def main():\n    print('hi')"),
        ("typescript", "function main(): void { console.log('hi'); }"),
        ("go", "func main() { fmt.Println(\"hi\") }"),
        ("bash", "echo hi && ls -la"),
        ("json", r#"{"key": "value", "n": 42}"#),
        ("yaml", "key: value\nn: 42"),
        ("toml", "key = \"value\"\nn = 42"),
    ];
    let caps = StyleCaps::full();
    let mut group = c.benchmark_group("syntect_highlight_lang");
    for (lang, code) in samples {
        group.bench_with_input(BenchmarkId::from_parameter(lang), code, |b, c| {
            b.iter(|| {
                let out = highlight(black_box(c), lang, caps);
                black_box(out);
            });
        });
    }
    group.finish();
}

fn bench_render_block(c: &mut Criterion) {
    let code = "fn main() {\n    let x = 1;\n    let y = 2;\n    println!(\"{}\", x + y);\n}";
    let caps = StyleCaps::full();
    let mut group = c.benchmark_group("syntect_render_block");
    group.bench_function("rust_five_lines", |b| {
        b.iter(|| {
            let out = render_block(black_box(code), "rust", caps);
            black_box(out);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_syntax_set_cold_load,
    bench_theme_set_cold_load,
    bench_lazy_access,
    bench_highlight_languages,
    bench_render_block,
);
criterion_main!(benches);
