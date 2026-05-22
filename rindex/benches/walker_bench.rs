use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rindex::ignore::IgnoreEngine;
use rindex::indexer::walker::FileWalker;
use std::path::Path;

fn bench_walk_rindex_source(c: &mut Criterion) {
    let engine = IgnoreEngine::default();
    let walker = FileWalker::new(&engine);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    c.bench_function("walk_rindex_project", |b| {
        b.iter(|| {
            let files = walker.walk(black_box(root)).unwrap();
            black_box(files)
        });
    });
}

fn bench_ignore_check(c: &mut Criterion) {
    let engine = IgnoreEngine::default();
    let paths = ["src/main.rs", ".git/HEAD", "node_modules/pkg/index.js",
                 "target/debug/rindex.exe", "src/lib.rs", "Cargo.toml",
                 "benches/search_bench.rs", ".DS_Store"];

    c.bench_function("ignore_check_8_paths", |b| {
        b.iter(|| {
            for path in &paths {
                let result = engine.should_ignore(black_box(path));
                black_box(result);
            }
        });
    });
}

criterion_group!(benches, bench_walk_rindex_source, bench_ignore_check);
criterion_main!(benches);
