use std::process::Command;

#[test]
fn test_binary_builds_successfully() {
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("Failed to run cargo build");
    assert!(status.success(), "cargo build --release should succeed");
}

#[test]
fn test_binary_exists() {
    let binary_name = if cfg!(target_os = "windows") { "rindex.exe" } else { "rindex" };
    let binary_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/release")
        .join(binary_name);

    // Build first if needed
    if !binary_path.exists() {
        let status = Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .status()
            .expect("Failed to run cargo build");
        assert!(status.success());
    }

    assert!(binary_path.exists(), "Binary should exist at {:?}", binary_path);
    assert!(binary_path.metadata().unwrap().len() > 1024, "Binary should be larger than 1KB");
}

#[test]
fn test_unit_tests_pass() {
    let status = Command::new("cargo")
        .args(["test", "--lib"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("Failed to run cargo test");
    assert!(status.success(), "Unit tests should pass");
}
