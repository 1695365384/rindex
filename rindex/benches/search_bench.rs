use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rindex::db::Database;
use rindex::db::queries;
use rindex::search::Searcher;
use std::sync::Mutex;

fn setup_db_with_chunks(count: usize) -> Database {
    let db = Database::open_temp().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;

    for i in 0..count {
        let path = format!("src/module_{}.rs", i / 10);
        queries::upsert_file(&db, &path, "abc", 100, now, "rust", now).unwrap();
        queries::insert_chunk(
            &db, &path, "function",
            Some(&format!("func_{}", i)), None,
            (i * 10 + 1) as i64, (i * 10 + 20) as i64,
            &format!("pub fn func_{}() -> i32 {{\n    // documentation for function {}\n    {} + 1\n}}", i, i, i),
        ).unwrap();
    }

    db
}

fn bench_search_symbol_100(c: &mut Criterion) {
    let db = setup_db_with_chunks(100);
    let searcher = Searcher::new(&db, None);

    c.bench_function("search_symbol_100chunks", |b| {
        b.iter(|| {
            let results = searcher.search_symbol(black_box("func_5"), None).unwrap();
            black_box(results)
        });
    });
}

fn bench_search_symbol_1000(c: &mut Criterion) {
    let db = setup_db_with_chunks(1000);
    let searcher = Searcher::new(&db, None);

    c.bench_function("search_symbol_1000chunks", |b| {
        b.iter(|| {
            let results = searcher.search_symbol(black_box("func_50"), None).unwrap();
            black_box(results)
        });
    });
}

fn bench_search_symbol_not_found(c: &mut Criterion) {
    let db = setup_db_with_chunks(500);
    let searcher = Searcher::new(&db, None);

    c.bench_function("search_symbol_not_found", |b| {
        b.iter(|| {
            let results = searcher.search_symbol(black_box("nonexistent_symbol_xyz"), None).unwrap();
            black_box(results)
        });
    });
}

criterion_group!(benches, bench_search_symbol_100, bench_search_symbol_1000, bench_search_symbol_not_found);
criterion_main!(benches);
