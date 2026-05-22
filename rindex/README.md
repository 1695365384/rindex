# rIndex — Local File Index MCP Server

A Rust-based MCP server that indexes project files using semantic search, so Claude Code can find code without repeated file exploration.

## Features

- **Semantic search** — Find code by meaning, not just keywords
- **Symbol search** — Quickly locate functions, classes, and types by name
- **Auto-indexing** — Automatically indexes projects on first open
- **Incremental updates** — Only reindexes changed files
- **Zero API calls** — Runs 100% locally with CPU-only inference
- **Multi-language** — Supports Rust, Python, JavaScript, TypeScript, Go, and more

## Installation

### 1. Build

```bash
cd rindex
cargo build --release
```

### 2. Install the binary

Copy `target/release/rindex` to a directory in your PATH:

```bash
# On Linux/macOS
cp target/release/rindex ~/.local/bin/

# On Windows
# copy target/release/rindex.exe %USERPROFILE%\.cargo\bin\
```

### 3. Configure Claude Code

Add to your `claude.json` (usually at `~/.claude/claude.json` or project root):

```json
{
  "mcpServers": {
    "rindex": {
      "command": "rindex",
      "args": []
    }
  }
}
```

### 4. Restart Claude Code

rindex will automatically index your project on first open.

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
