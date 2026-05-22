const BUILTIN_PATTERNS: &[&str] = &[
    ".git/", ".git/**", "node_modules/", "node_modules/**",
    "target/", "target/**", "dist/", "dist/**", "build/", "build/**",
    ".next/", ".next/**", ".venv/", ".venv/**", "__pycache__/",
    "*.pyc", "*.pyo", "*.bin", "*.exe", "*.dll", "*.so", "*.dylib", "*.class",
    ".DS_Store", "Thumbs.db", "*.log",
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

pub struct IgnoreEngine {
    custom_patterns: Vec<glob::Pattern>,
}

impl IgnoreEngine {
    pub fn new(_config: &IgnoreConfig) -> Self {
        Self { custom_patterns: Vec::new() }
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

    pub fn should_ignore(&self, relative_path: &str) -> bool {
        let normalized = relative_path.replace('\\', "/");
        for pattern_str in BUILTIN_PATTERNS {
            if let Ok(p) = glob::Pattern::new(pattern_str) {
                if p.matches(&normalized) || p.matches(&format!("**/{}", normalized)) {
                    return true;
                }
            }
        }
        for pattern in &self.custom_patterns {
            if pattern.matches(&normalized) {
                return true;
            }
        }
        false
    }

    pub fn is_binary_extension(ext: &str) -> bool {
        BINARY_EXTENSIONS.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_text_extension(ext: &str) -> bool {
        TEXT_EXTENSIONS.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_too_large(&self, size: u64) -> bool {
        size > self.max_file_size
    }

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
