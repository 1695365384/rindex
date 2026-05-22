"""Configuration management system with profile support."""

from __future__ import annotations
import os
import json
import time
from pathlib import Path
from typing import Any, Dict, List, Optional, Union
from dataclasses import dataclass, field
from enum import Enum
from threading import Lock


class ConfigType(Enum):
    """Supported configuration value types."""
    STRING = "string"
    INTEGER = "integer"
    FLOAT = "float"
    BOOLEAN = "boolean"
    ARRAY = "array"
    OBJECT = "object"


@dataclass
class ConfigValue:
    """A single configuration value with metadata."""
    value: Any
    value_type: ConfigType
    source: str = "default"
    description: str = ""
    validated: bool = False

    def as_int(self) -> int:
        if self.value_type == ConfigType.INTEGER:
            return self.value
        raise TypeError(f"Cannot convert {self.value_type} to int")


class ConfigSource:
    """Represents a configuration source (file, env var, etc.)."""

    def __init__(self, name: str, priority: int = 0):
        self.name = name
        self.priority = priority
        self.values: Dict[str, ConfigValue] = {}
        self._lock = Lock()

    def load_from_dict(self, data: Dict[str, Any], prefix: str = "") -> None:
        with self._lock:
            for key, value in data.items():
                full_key = f"{prefix}.{key}" if prefix else key
                if isinstance(value, dict):
                    self.load_from_dict(value, full_key)
                else:
                    self.values[full_key] = ConfigValue(
                        value=value,
                        value_type=self._infer_type(value),
                        source=self.name,
                    )

    def load_from_file(self, path: Path) -> None:
        with open(path) as f:
            if path.suffix == ".json":
                data = json.load(f)
            else:
                data = self._parse_ini(f)
        self.load_from_dict(data)

    @staticmethod
    def _infer_type(value: Any) -> ConfigType:
        if isinstance(value, bool):
            return ConfigType.BOOLEAN
        elif isinstance(value, int):
            return ConfigType.INTEGER
        elif isinstance(value, float):
            return ConfigType.FLOAT
        elif isinstance(value, (list, tuple)):
            return ConfigType.ARRAY
        elif isinstance(value, dict):
            return ConfigType.OBJECT
        else:
            return ConfigType.STRING

    @staticmethod
    def _parse_ini(f) -> Dict[str, str]:
        result = {}
        for line in f:
            line = line.strip()
            if not line or line.startswith("#") or line.startswith(";"):
                continue
            if "=" in line:
                key, value = line.split("=", 1)
                result[key.strip()] = value.strip()
        return result


class ConfigManager:
    """Hierarchical configuration manager with override support."""

    def __init__(self):
        self.sources: List[ConfigSource] = []
        self._cache: Dict[str, ConfigValue] = {}
        self._cache_time: float = 0
        self._cache_ttl: float = 5.0
        self._lock = Lock()

    def add_source(self, source: ConfigSource) -> None:
        self.sources.append(source)
        self.sources.sort(key=lambda s: s.priority, reverse=True)
        self._invalidate_cache()

    def load_file(self, path: Union[str, Path]) -> None:
        source = ConfigSource(name=str(path), priority=len(self.sources))
        source.load_from_file(Path(path))
        self.add_source(source)

    def add_env_source(self, prefix: str = "APP_") -> None:
        env_source = ConfigSource("environment", priority=1000)
        for key, value in os.environ.items():
            if key.startswith(prefix):
                config_key = key[len(prefix):].lower().replace("__", ".")
                env_source.values[config_key] = ConfigValue(
                    value=value,
                    value_type=ConfigType.STRING,
                    source="environment",
                )
        self.add_source(env_source)

    def get(self, key: str, default: Any = None) -> Any:
        self._ensure_cache_fresh()
        if key in self._cache:
            return self._cache[key].value
        return default

    def get_int(self, key: str, default: int = 0) -> int:
        value = self.get(key)
        if isinstance(value, int):
            return value
        if isinstance(value, str) and value.isdigit():
            return int(value)
        return default

    def get_bool(self, key: str, default: bool = False) -> bool:
        value = self.get(key)
        if isinstance(value, bool):
            return value
        if isinstance(value, str):
            return value.lower() in ("true", "yes", "1")
        return default

    def all(self) -> Dict[str, Any]:
        self._ensure_cache_fresh()
        return {k: v.value for k, v in self._cache.items()}

    def _ensure_cache_fresh(self) -> None:
        now = time.time()
        if now - self._cache_time > self._cache_ttl:
            self._rebuild_cache()

    def _rebuild_cache(self) -> None:
        with self._lock:
            merged: Dict[str, ConfigValue] = {}
            for source in reversed(self.sources):
                merged.update(source.values)
            self._cache = merged
            self._cache_time = time.time()

    def _invalidate_cache(self) -> None:
        self._cache_time = 0


class ProfileAwareConfig:
    """Configuration that switches based on active profile."""

    def __init__(self, profile: str = "default"):
        self.active_profile = profile
        self.profiles: Dict[str, ConfigManager] = {}
        self._global = ConfigManager()

    def load_profile(self, name: str, path: Path) -> None:
        mgr = ConfigManager()
        mgr.load_file(path)
        mgr.add_env_source()
        self.profiles[name] = mgr

    def for_profile(self, name: str) -> Optional[ConfigManager]:
        return self.profiles.get(name)

    def get(self, key: str, default: Any = None) -> Any:
        # Try active profile first
        if self.active_profile in self.profiles:
            val = self.profiles[self.active_profile].get(key)
            if val is not None:
                return val
        # Fall back to default
        if "default" in self.profiles:
            val = self.profiles["default"].get(key)
            if val is not None:
                return val
        # Global fallback
        return self._global.get(key, default)
