use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rindex::indexer::chunker::chunk_file;
use rindex::indexer::parser::parse_code;

/// Large Rust source to benchmark parsing (simulates a real-world ~500 line file)
const RUST_CODE: &str = include_str!("./fixtures/large_rust.rs");

/// Python source
const PY_CODE: &str = include_str!("./fixtures/large_python.py");

fn bench_parse_rust(c: &mut Criterion) {
    c.bench_function("parse_rust_500lines", |b| {
        b.iter(|| {
            let symbols = parse_code(black_box(RUST_CODE), "rust").unwrap();
            black_box(symbols)
        });
    });
}

fn bench_parse_python(c: &mut Criterion) {
    c.bench_function("parse_python_500lines", |b| {
        b.iter(|| {
            let symbols = parse_code(black_box(PY_CODE), "python").unwrap();
            black_box(symbols)
        });
    });
}

fn bench_chunk_rust(c: &mut Criterion) {
    c.bench_function("chunk_rust_500lines", |b| {
        b.iter(|| {
            let chunks = chunk_file(black_box(RUST_CODE), "rust").unwrap();
            black_box(chunks)
        });
    });
}

fn bench_parse_empty(c: &mut Criterion) {
    c.bench_function("parse_empty_string", |b| {
        b.iter(|| {
            let symbols = parse_code(black_box(""), "rust").unwrap();
            black_box(symbols)
        });
    });
}

criterion_group!(benches, bench_parse_rust, bench_parse_python, bench_chunk_rust, bench_parse_empty);
criterion_main!(benches);
