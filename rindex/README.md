# rIndex — Local File Index MCP Server

A Rust-based MCP server that indexes project files using semantic search, so Claude Code can find code without repeated file exploration.

## Features

- **Semantic search** — Find code by meaning, not just keywords
- **Symbol search** — Quickly locate functions, classes, and types by name
- **Auto-indexing** — Automatically indexes projects on first open
- **Incremental updates** — Only reindexes changed files
- **Zero API calls** — Runs 100% locally with CPU-only inference
- **Multi-language** — Supports Rust, Python, JavaScript, TypeScript, Go, and more

## Quick Start

```bash
# Build
cargo build --release

# Install
cp target/release/rindex ~/.local/bin/

# Claude Code will auto-index your project on first open
```

## CLI Options

```
rindex [OPTIONS]

  -p, --path <PATH>        Project root (default: current dir)
      --db <PATH>          Database location (default: ~/.local/share/rindex/rindex.db)
      --config <FILE>      Config file (rindex.toml)
      --no-model           Skip embedding model (text-only search)
      --max-size <BYTES>   Max file size to index (default: 1MB)
      --model-id <ID>      Embedding model (default: BAAI/bge-small-en-v1.5)
```

## Config File (`rindex.toml`)

```toml
max_file_size = 2097152
model_id = "BAAI/bge-small-en-v1.5"
default_search_limit = 20
watcher_debounce_ms = 500
```

## Logging

- Default: human-readable stderr
- `RINDEX_LOG_FORMAT=json` — JSON structured logs (production)
- `RUST_LOG=rindex=debug` — verbose logging

## Available Tools

| Tool | Description |
|------|-------------|
| `search` | Semantic code search. Returns relevant files, functions, and line numbers |
| `search_symbol` | Find by exact symbol name (function, class, etc.) |
| `project_status` | Check how many files and chunks are indexed |
| `reindex` | Trigger a full reindex of the project |

## How It Works

1. **Indexing**: Files are scanned, parsed with tree-sitter AST, split into function/class-level chunks
2. **Embedding**: Each chunk is vectorized using BGE-small-en-v1.5 (runs locally via candle, CPU only)
3. **Storage**: Vectors stored in SQLite — no external database needed
4. **Search**: Queries are embedded and matched via cosine similarity against all chunks

## Requirements

- Rust 1.75+
- No GPU required (CPU-only inference)
- ~200MB disk space for the embedding model cache
