/// Built-in path prefixes that are always ignored (checked via starts_with/contains)
const BUILTIN_DIRS: &[&str] = &[
    ".git", "node_modules", "target", "dist", "build", ".next", ".venv", "__pycache__",
];

/// Built-in file extensions that are always ignored
const BUILTIN_EXTS: &[&str] = &[
    "pyc", "pyo", "bin", "exe", "dll", "so", "dylib", "class", "log",
];

/// Built-in filenames always ignored
const BUILTIN_FILES: &[&str] = &[
    ".DS_Store", "Thumbs.db",
];

const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "ico", "svg",
    "woff", "woff2", "ttf", "eot",
    "pdf", "doc", "docx", "xls", "xlsx",
    "zip", "tar", "gz", "bz2", "7z", "rar",
    "mp3", "mp4", "avi", "mov", "wav",
    "o", "obj", "lib", "a",
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
}

impl IgnoreEngine {
    pub fn new(config: &IgnoreConfig) -> Self {
        Self { config: config.clone(), custom_patterns: Vec::new() }
    }

    pub fn add_gitignore_pattern(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
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
