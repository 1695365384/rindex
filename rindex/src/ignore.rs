/// Built-in path prefixes for build artifacts and dependency directories.
/// These are a best-effort fallback — .gitignore is the primary filter.
/// Only list directories that are NEVER project source code.
const BUILTIN_DIRS: &[&str] = &[
    // === VCS ===
    ".git",
    // === JS/TS ecosystem ===
    "node_modules", "dist", ".next", ".nuxt", ".output", ".svelte-kit", ".angular",
    ".plasmo",  // Plasmo browser extension build output
    // === Rust ===
    "target",
    // === Python ===
    "__pycache__", ".venv", "venv", ".eggs", ".tox",
    ".pytest_cache", ".mypy_cache", ".ruff_cache", ".hypothesis",
    // === Go ===
    "vendor",
    // === Java/Kotlin/Gradle ===
    ".gradle",
    // === .NET ===
    // "bin", "obj" — handled as ROOT_ONLY_DIRS below
    // === General build output / caches ===
    "build", "out", "_build", ".turbo", ".cache", ".parcel-cache", ".nx",
    "bower_components",
    // === Test / Coverage reports ===
    "coverage", ".nyc_output", "htmlcov",
    // === IDE / Editor ===
    ".idea", ".vscode", ".project", ".classpath", ".settings",
    // === Infra ===
    ".terraform", ".serverless",
    // === Documentation builds ===
    ".docusaurus",
];

/// File extensions that are always ignored — compiled binaries and lock files.
/// These are NOT project source assets.
const BUILTIN_EXTS: &[&str] = &[
    "pyc", "pyo", "class",           // Compiled bytecode
    "exe", "dll", "so", "dylib",     // Native binaries
    "bin", "obj", "lib", "a",        // Compiled objects
    "wasm",                           // WebAssembly binary
    "lock",                           // Auto-generated lock files
];

/// Directory names that are only ignored at the project root level.
/// These may contain source code in subdirectories (e.g. src/bin/).
const ROOT_ONLY_DIRS: &[&str] = &[
    "bin", "obj",
];

/// Built-in filenames always ignored
const BUILTIN_FILES: &[&str] = &[
    ".DS_Store", "Thumbs.db",
];

/// Binary file extensions that can't be read as text.
/// Keep minimal — images/fonts/media/docs CAN be project assets; we just can't parse them.
const BINARY_EXTENSIONS: &[&str] = &[
    // Images
    "png", "jpg", "jpeg", "gif", "ico", "svg", "webp", "bmp",
    // Fonts
    "woff", "woff2", "ttf", "eot", "otf",
    // Documents
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    // Archives
    "zip", "tar", "gz", "bz2", "7z", "rar", "xz",
    // Media
    "mp3", "mp4", "avi", "mov", "wav", "flac", "ogg", "webm",
    // Compiled objects (also in BUILTIN_EXTS, listed here for safety)
    "o", "obj", "lib", "a", "wasm",
    // Packages (never project source)
    "whl", "jar", "war", "ear", "apk", "ipa",
];

const TEXT_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "ts", "tsx", "jsx", "go",
    "c", "h", "cpp", "hpp", "java", "kt", "kts",
    "swift", "rb", "php", "pl", "pm", "lua",
    "toml", "json", "yaml", "yml", "md", "txt",
    "xml", "html", "css", "scss", "less",
    "sh", "bash", "zsh", "fish",
    "sql", "graphql", "proto",
    "vue", "svelte", "astro",
    "dockerfile", "cmake", "makefile",
    "gradle", "properties", "cfg", "conf",
    "env", "env.example",
];

#[derive(Clone)]
pub struct IgnoreConfig {
    pub max_file_size: u64,
}

impl Default for IgnoreConfig {
    fn default() -> Self {
        Self { max_file_size: 1_048_576 }
    }
}

#[derive(Clone)]
pub struct IgnoreEngine {
    config: IgnoreConfig,
    /// Custom glob patterns from .gitignore (pre-compiled)
    custom_patterns: Vec<glob::Pattern>,
    /// Patterns ending with '/' — only match directories
    dir_only_patterns: Vec<glob::Pattern>,
    /// Negation patterns (from lines starting with '!') — these override ignores
    negation_patterns: Vec<glob::Pattern>,
}

impl IgnoreEngine {
    pub fn new(config: &IgnoreConfig) -> Self {
        Self { config: config.clone(), custom_patterns: Vec::new(), dir_only_patterns: Vec::new(), negation_patterns: Vec::new() }
    }

    pub fn add_gitignore_pattern(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return;
        }
        if line.starts_with('!') {
            let pattern = &line[1..];
            if let Ok(p) = glob::Pattern::new(pattern) {
                self.negation_patterns.push(p);
            }
            return;
        }
        if line.ends_with('/') {
            let without_slash = &line[..line.len() - 1];
            if let Ok(p) = glob::Pattern::new(without_slash) {
                self.dir_only_patterns.push(p);
            }
            return;
        }
        if let Ok(pattern) = glob::Pattern::new(line) {
            self.custom_patterns.push(pattern);
        }
    }

    /// Fast path: check if a relative path should be ignored
    /// Uses prefix/suffix matching instead of glob where possible
    #[inline]
    pub fn should_ignore(&self, relative_path: &str) -> bool {
        let norm = relative_path.replace('\\', "/");

        // Check negation patterns first — these OVERRIDE all other ignores
        for pattern in &self.negation_patterns {
            if pattern.matches(&norm) || pattern.matches(&norm.trim_end_matches('/')) {
                return false;
            }
        }

        // Check root-only directories (e.g. bin/, obj/ — but NOT src/bin/)
        for dir in ROOT_ONLY_DIRS {
            if norm.starts_with(dir) && (norm.len() == dir.len() || norm.as_bytes().get(dir.len()) == Some(&b'/')) {
                return true;
            }
        }

        // Check built-in directory prefixes: "dir/" or "dir/anything"
        for dir in BUILTIN_DIRS {
            if norm.starts_with(dir) && (norm.len() == dir.len() || norm.as_bytes().get(dir.len()) == Some(&b'/')) {
                return true;
            }
            // Also check "/dir/" for nested paths
            let search = format!("/{}/", dir);
            if norm.contains(&search) {
                return true;
            }
        }

        // Check built-in file extensions: "*.ext"
        if let Some(dot) = norm.rfind('.') {
            let ext = &norm[dot + 1..];
            if BUILTIN_EXTS.contains(&ext) {
                return true;
            }
        }

        // Check built-in filenames
        if let Some(last_slash) = norm.rfind('/') {
            let fname = &norm[last_slash + 1..];
            if BUILTIN_FILES.contains(&fname) {
                return true;
            }
        } else if BUILTIN_FILES.contains(&norm.as_str()) {
            return true;
        }

        // Check directory-only patterns (from "dir/" style gitignore rules)
        // Only match if the path IS a directory (ends with "/")
        if norm.ends_with('/') || norm.ends_with("/*") {
            let without_slash = norm.trim_end_matches('/');
            for pattern in &self.dir_only_patterns {
                if pattern.matches(without_slash) {
                    return true;
                }
            }
        }

        // Check custom gitignore patterns (pre-compiled)
        for pattern in &self.custom_patterns {
            if pattern.matches(&norm) {
                return true;
            }
        }

        false
    }

    #[inline]
    pub fn is_binary_extension(ext: &str) -> bool {
        BINARY_EXTENSIONS.contains(&ext)
    }

    #[inline]
    pub fn is_text_extension(ext: &str) -> bool {
        TEXT_EXTENSIONS.contains(&ext)
    }

    #[inline]
    pub fn is_too_large(&self, size: u64) -> bool {
        size > self.config.max_file_size
    }

    /// Combined check: should this file be included in the index?
    #[inline]
    pub fn should_index(&self, relative_path: &str, size: u64, ext: &str) -> bool {
        if self.should_ignore(relative_path) { return false; }
        if Self::is_binary_extension(ext) { return false; }
        if self.is_too_large(size) { return false; }
        if ext.is_empty() { return false; }
        Self::is_text_extension(ext)
    }
}

impl Default for IgnoreEngine {
    fn default() -> Self {
        Self::new(&IgnoreConfig::default())
    }
}
