<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-000000?style=for-the-badge&logo=rust" />
  <img src="https://img.shields.io/badge/license-MIT-8B5CF6?style=for-the-badge" />
  <img src="https://img.shields.io/badge/inference-CPU--only-10B981?style=for-the-badge" />
</p>

# rindex

> Local code index & semantic search engine for LLM coding agents.
> Stop grepping. Start understanding.

**rindex** indexes your entire project on disk and serves code search via MCP (Model Context Protocol). It lets Claude Code find functions, types, and related code in **1-2 tool calls** instead of 5-7 rounds of grep + file reading.

---

## Core Value

### Your context window is your most expensive resource

Claude Code's context window is 200K tokens. Sounds like a lot, but a single grep returns ~3,000 tokens of noise — **20 searches burn through 30% of your context with useless information**. Claude isn't understanding your code; it's sifting through garbage.

rindex compresses each search response from ~3,000 tokens to ~70 tokens. **The freed space goes where it matters: Claude understanding your business logic, tracking bug root causes, generating correct code.**

### Do the math

| | Grep/Glob | rindex | Savings |
|:---|---:|---:|---:|
| **Calls per search** | 5 – 7 | 1 – 2 | **~70%** |
| **Response tokens per search** | ~3,000 | ~70 | **~98%** |
| **Daily 20 searches** | ~60,000 tok / 140 calls | ~1,400 tok / 40 calls | **58,600 tokens** |
| **Daily cost (Claude Opus)** | ~$0.90 | ~$0.02 | **$0.88/day** |
| **Monthly cost (individual)** | ~$19.80 | ~$0.44 | **$19/month** |
| **Monthly cost (5-person team)** | ~$99 | ~$2.20 | **$97/month** |
| **Monthly cost (20-person team)** | ~$396 | ~$8.80 | **$387/month** |
| **Wait time per search** | 5 – 7 round trips | 1 – 2 round trips | **3-5× faster** |

> **A 5-person team saves $97/month, $1,164/year.** And that's before counting the higher-quality code Claude produces with a cleaner context — one fewer bug-fix round pays for itself.

### Performance Benchmarks

| Operation | Latency |
|:---|---:|
| Symbol search (1,000 chunks) | **1.7 µs** |
| Parse 500-line Rust file | **1.36 ms** |
| Walk 38-file project | **836 µs** |

### The bottom line

> rindex isn't "a better grep." It changes how LLM coding agents work:
> **from "digging through noise" to "getting the answer directly."** Every token and every second saved becomes better code.

---

## Features

- **Semantic search** — Find code by meaning, not exact text. "database config" finds `DBPool::new()`
- **Symbol lookup** — Locate functions, structs, traits by name in microseconds. 1000× faster than grep
- **find_related** — Discover conceptually similar code across the project in one call
- **Auto-indexing** — Scans your project on first open, syncs incrementally. Zero config
- **.llm-index-ignore** — Fine-grained exclusion rules (alongside .gitignore)
- **Cross-session memory** — Save project notes that persist across conversations. Stop re-explaining your codebase
- **100% local** — No API keys, no cloud, no telemetry. Static token embeddings run on CPU (no ML framework, no GPU)

---

## Install

### One-click (recommended)

Download the release package, extract, double-click the install script:

| Platform | Package | Installer |
|---|---|---|
| **Windows 11** | `rindex-v1.0.0-windows-x64.zip` | Double-click `install.bat` |
| **macOS** | `rindex-v1.0.0-macos-arm64.tar.gz` | Double-click `install.command` |

Binary, embedding model, PATH — everything installed in one click. After that, open your coding agent in any project. rindex auto-detects your client and configures everything on first start. Zero manual steps.

**Supported clients**: Claude Code, Cursor, Windsurf, and any MCP-compatible agent.

### Build from source

```bash
cargo build --release && cp target/release/rindex ~/.local/bin/
rindex backfill  # embeds all chunks with the static model (~166MB, bundled in release)
```

Then open Claude Code — rindex handles the rest.

---

## Client Support

Any MCP-compatible coding agent works. rindex auto-detects and configures:

| Client | Config | Setup |
|---|---|---|
| **Claude Code** | `.mcp.json` + `.claude/skills/` | Automatic |
| **Cursor** | `.cursor/mcp.json` | Automatic |
| **Windsurf** | `.windsurf/mcp.json` | Automatic |
| **Other MCP clients** | Standard MCP JSON config | Point to `rindex` |

All share the same index database — switch clients anytime, no reindex needed.

## Available Tools

| Tool | Description |
|:---|:---|
| `search` | Semantic / keyword search across indexed code |
| `search_symbol` | Exact symbol lookup — functions, classes, types, traits |
| `find_related` | Discover code semantically similar to a symbol or file location |
| `project_status` | Check indexing progress, file/chunk counts, model state |
| `reindex` | Rebuild the entire project index from scratch (runs in background) |
| `backfill` | Manually trigger embedding generation for chunks that lack them |
| `verify` | Check index integrity — reports stale entries and missing files |
| `session_note` | Save persistent project memory (decisions, gotchas, patterns) |
| `session_context` | Read project memories from past sessions |

---

## How It Works

```
Index Pipeline
────────────────────────────────────────────────────
  Source   →  tree-sitter AST  →  function/class  →  Static     →  SQLite
  files        parse symbols      chunks              Embedding      storage
                                                       Table
```

1. **Parse** — file walker scans project respecting `.gitignore` + `.llm-index-ignore`
2. **Chunk** — tree-sitter splits code into symbol-level chunks (functions, structs, classes)
3. **Embed** — Custom-distilled static token embedding table (CPU, ~0.01ms/chunk — just a lookup, no transformer)
4. **Search** — query embedded → cosine similarity re-ranking → compact text output

Search uses a **two-stage hybrid** pipeline: FTS5 full-text pre-filter (AND semantics) narrows candidates, then embedding similarity re-ranks. This avoids loading all vectors into memory, scaling to 100k+ chunks.

---

## Configuration

### CLI

```
rindex [OPTIONS]

  -p, --path <PATH>       Project root (default: current dir)
      --db <PATH>         Database path (default: platform data dir/rindex/rindex.db)
      --max-size <BYTES>  Max file size to index (default: 1 MB)
      --model-id <ID>     Static model dir under model_cache (default: c2llm-static-256)
      --no-model          Skip embedding model — text-only search
```

### `rindex.toml`

```toml
max_file_size = 2097152           # 2 MB
model_id = "c2llm-static-256"    # local dir under model_cache_dir
default_search_limit = 20
watcher_debounce_ms = 500
```

---

## Requirements

- **Rust 1.75+**
- **No GPU needed** — CPU-only inference runs on any machine
- **~170 MB** disk for model cache (token_embeddings.safetensors + tokenizer.json)
- **~50 MB** disk per 10k indexed chunks (SQLite + vectors)

---

## License

MIT
