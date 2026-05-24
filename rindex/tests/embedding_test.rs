use rindex::embedding::Embedder;
use std::path::PathBuf;

fn model_dir() -> PathBuf {
    // Matches Config::load() default: dirs::data_dir()/rindex/models
    dirs::data_dir()
        .expect("data dir")
        .join("rindex")
        .join("models")
}

#[test]
fn test_load_and_embed() {
    let dir = model_dir();
    let embedder = Embedder::load(&dir, "c2llm-static-256", None)
        .expect("Failed to load static model");

    // Embed a simple Rust function
    let vec = embedder.embed("fn main() { println!(\"hello\"); }")
        .expect("embed failed");

    assert_eq!(vec.len(), 256, "expected 256-dim vector");
    assert!(vec.iter().any(|&v| v != 0.0), "should not be all zeros");

    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 0.001, "should be L2 normalized, got norm={}", norm);

    println!("embedding: first 8 values = {:?}", &vec[..8]);
}

#[test]
fn test_different_texts_different_vectors() {
    let dir = model_dir();
    let embedder = Embedder::load(&dir, "c2llm-static-256", None).unwrap();

    let v1 = embedder.embed("fn authenticate(user: &User) -> bool { user.verify() }").unwrap();
    let v2 = embedder.embed("struct Database { conn: Pool, cache: LruCache }").unwrap();
    let v3 = embedder.embed("def render_html(template: str) -> str: pass").unwrap();

    let cos_sim = |a: &[f32], b: &[f32]| {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        dot // already L2 normalized
    };

    let sim_12 = cos_sim(&v1, &v2);
    let sim_13 = cos_sim(&v1, &v3);

    println!("cos_sim(rust_auth, rust_struct) = {:.4}", sim_12);
    println!("cos_sim(rust_auth, python_html) = {:.4}", sim_13);

    // Same-language should have higher similarity
    assert!(sim_12 > sim_13,
        "rust→rust ({}) should be closer than rust→python ({})",
        sim_12, sim_13
    );
}

#[test]
fn test_empty_text() {
    let dir = model_dir();
    let embedder = Embedder::load(&dir, "c2llm-static-256", None).unwrap();
    let vec = embedder.embed("").unwrap();
    assert_eq!(vec.len(), 256);
}

#[test]
fn test_reproducible() {
    let dir = model_dir();
    let embedder = Embedder::load(&dir, "c2llm-static-256", None).unwrap();
    let v1 = embedder.embed("test reproducibility").unwrap();
    let v2 = embedder.embed("test reproducibility").unwrap();
    assert_eq!(v1, v2, "same text should produce identical vector");
}
