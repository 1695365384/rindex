use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A configuration manager that loads settings from multiple sources
pub struct ConfigManager {
    settings: HashMap<String, ConfigValue>,
    sources: Vec<ConfigSource>,
    cache: Option<Arc<ConfigCache>>,
}

#[derive(Debug, Clone)]
pub enum ConfigValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Array(Vec<ConfigValue>),
    Map(HashMap<String, ConfigValue>),
}

struct ConfigSource {
    path: PathBuf,
    priority: u32,
    loaded: bool,
}

impl ConfigManager {
    pub fn new() -> Self {
        Self {
            settings: HashMap::new(),
            sources: Vec::new(),
            cache: None,
        }
    }

    pub fn load_file(&mut self, path: &Path) -> Result<(), ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Io(e.to_string()))?;
        let source = ConfigSource {
            path: path.to_path_buf(),
            priority: self.sources.len() as u32,
            loaded: true,
        };
        self.sources.push(source);
        self.parse_and_merge(&content)?;
        self.cache = None;
        Ok(())
    }

    fn parse_and_merge(&mut self, content: &str) -> Result<(), ConfigError> {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                let k = key.trim().to_string();
                let v = self.parse_value(value.trim());
                self.settings.insert(k, v);
            }
        }
        Ok(())
    }

    fn parse_value(&self, raw: &str) -> ConfigValue {
        if raw.starts_with('"') && raw.ends_with('"') {
            ConfigValue::String(raw[1..raw.len()-1].to_string())
        } else if raw == "true" {
            ConfigValue::Boolean(true)
        } else if raw == "false" {
            ConfigValue::Boolean(false)
        } else if raw.contains('.') {
            ConfigValue::Float(raw.parse().unwrap_or(0.0))
        } else {
            ConfigValue::Integer(raw.parse().unwrap_or(0))
        }
    }

    pub fn get_string(&self, key: &str) -> Option<&str> {
        match self.settings.get(key) {
            Some(ConfigValue::String(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn get_int(&self, key: &str) -> Option<i64> {
        match self.settings.get(key) {
            Some(ConfigValue::Integer(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn merge(&mut self, other: ConfigManager) {
        for (key, value) in other.settings {
            self.settings.insert(key, value);
        }
        self.cache = None;
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(String),
    Parse(String),
    NotFound(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(msg) => write!(f, "IO error: {}", msg),
            ConfigError::Parse(msg) => write!(f, "Parse error: {}", msg),
            ConfigError::NotFound(key) => write!(f, "Key not found: {}", key),
        }
    }
}

impl std::error::Error for ConfigError {}

/// A simple LRU cache with TTL support
pub struct ConfigCache {
    entries: HashMap<String, CacheEntry>,
    capacity: usize,
}

struct CacheEntry {
    value: ConfigValue,
    expires_at: std::time::Instant,
}

impl ConfigCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            capacity,
        }
    }

    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.entries.get(key).and_then(|entry| {
            if entry.expires_at > std::time::Instant::now() {
                Some(&entry.value)
            } else {
                None
            }
        })
    }

    pub fn set(&mut self, key: String, value: ConfigValue, ttl: std::time::Duration) {
        if self.entries.len() >= self.capacity {
            if let Some(oldest) = self.entries.keys().next().cloned() {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(key, CacheEntry {
            value,
            expires_at: std::time::Instant::now() + ttl,
        });
    }

    pub fn invalidate(&mut self) {
        self.entries.clear();
    }
}

/// Profile-aware configuration with environment variable override support
pub struct ProfileConfig {
    active_profile: String,
    managers: HashMap<String, ConfigManager>,
}

impl ProfileConfig {
    pub fn new(profile: &str) -> Self {
        Self {
            active_profile: profile.to_string(),
            managers: HashMap::new(),
        }
    }

    pub fn load_profile(&mut self, name: &str, path: &Path) -> Result<(), ConfigError> {
        let mut manager = ConfigManager::new();
        manager.load_file(path)?;
        self.managers.insert(name.to_string(), manager);
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<ConfigValue> {
        // Try active profile first, then fall back to "default"
        if let Some(mgr) = self.managers.get(&self.active_profile) {
            if let Some(val) = mgr.settings.get(key) {
                return Some(val.clone());
            }
        }
        if let Some(mgr) = self.managers.get("default") {
            if let Some(val) = mgr.settings.get(key) {
                return Some(val.clone());
            }
        }
        // Check environment variable override
        let env_key = format!("CONFIG_{}", key.to_uppercase().replace('.', "_"));
        if let Ok(val) = std::env::var(&env_key) {
            return Some(ConfigValue::String(val));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_manager() {
        let mut mgr = ConfigManager::new();
        mgr.parse_and_merge("host = \"localhost\"\nport = 8080").unwrap();
        assert_eq!(mgr.get_string("host"), Some("localhost"));
        assert_eq!(mgr.get_int("port"), Some(8080));
    }

    #[test]
    fn test_cache_ttl() {
        let mut cache = ConfigCache::new(10);
        cache.set("key".into(), ConfigValue::Integer(42), std::time::Duration::from_secs(60));
        assert!(cache.get("key").is_some());
    }

    #[test]
    fn test_profile_fallback() {
        let config = ProfileConfig::new("production");
        assert!(config.get("database.url").is_none());
    }
}
