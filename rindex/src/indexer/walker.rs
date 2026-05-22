use crate::ignore::IgnoreEngine;
use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub relative_path: String,
    pub size: u64,
    pub mtime: u64,
    pub language: String,
}

fn ext_to_language(ext: &str) -> String {
    match ext {
        "rs" => "rust".to_string(), "py" => "python".to_string(),
        "js" | "jsx" => "javascript".to_string(), "ts" | "tsx" => "typescript".to_string(),
        "go" => "go".to_string(), "c" | "h" => "c".to_string(), "cpp" | "hpp" | "cc" => "cpp".to_string(),
        "java" => "java".to_string(), "kt" | "kts" => "kotlin".to_string(), "swift" => "swift".to_string(),
        "rb" => "ruby".to_string(), "php" => "php".to_string(), "pl" | "pm" => "perl".to_string(),
        "lua" => "lua".to_string(), "toml" => "toml".to_string(), "json" => "json".to_string(),
        "yaml" | "yml" => "yaml".to_string(), "md" => "markdown".to_string(), "html" => "html".to_string(),
        "css" => "css".to_string(), "sh" | "bash" => "shell".to_string(), "sql" => "sql".to_string(),
        "vue" => "vue".to_string(), "svelte" => "svelte".to_string(), "dockerfile" => "dockerfile".to_string(),
        "gradle" => "gradle".to_string(),
        _ => ext.to_string(),
    }
}

pub struct FileWalker<'a> {
    ignore: &'a IgnoreEngine,
}

impl<'a> FileWalker<'a> {
    pub fn new(ignore: &'a IgnoreEngine) -> Self {
        Self { ignore }
    }

    pub fn walk(&self, root: &Path) -> Result<Vec<FileEntry>> {
        let mut files = Vec::new();
        let root_path = root.canonicalize()?;

        for entry in WalkDir::new(&root_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                if e.file_type().is_dir() {
                    let rel = e.path().strip_prefix(&root_path).ok()
                        .and_then(|p| p.to_str())
                        .map(|s| format!("{}/", s.replace('\\', "/")));
                    if e.depth() > 0 {
                        if let Some(rel) = &rel {
                            if self.ignore.should_ignore(rel) {
                                return false;
                            }
                        }
                    }
                }
                true
            })
        {
            let entry = entry?;
            if !entry.file_type().is_file() { continue; }

            let path = entry.path();
            let relative_path = match path.strip_prefix(&root_path).ok()
                .and_then(|p| p.to_str()) {
                Some(r) => r.replace('\\', "/"),
                None => continue,
            };

            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            let size = entry.metadata()?.len();
            let mtime = entry.metadata()?.modified()
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
                .unwrap_or(0);

            if !self.ignore.should_index(&relative_path, size, &ext) { continue; }

            let language = ext_to_language(&ext);
            files.push(FileEntry { path: path.to_path_buf(), relative_path, size, mtime, language });
        }

        Ok(files)
    }
}
