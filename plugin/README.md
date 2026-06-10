# rindex Claude Code Plugin

Local semantic code index with cross-session memory. Runs entirely on your machine.

## What you get

- **semantic search** — find code by meaning, not just text
- **symbol lookup** — locate functions, classes, types by name
- **related code discovery** — find conceptually similar code
- **cross-session memory** — project knowledge persists across Claude sessions

## 3-step install

### 1. Install the rindex binary

```bash
cargo install --git https://github.com/bundy-work/llm-file-index.git rindex
```

Or download from [GitHub Releases](https://github.com/bundy-work/llm-file-index/releases).

### 2. Add the marketplace & install plugin

```bash
claude plugins marketplace add rindex-marketplace \
  --github bundy-work/llm-file-index

claude plugins install rindex@rindex-marketplace
```

### 3. Register the MCP server

```bash
claude mcp add --scope user rindex -- rindex
```

Or use the one-line installer (Windows):

```powershell
irm https://raw.githubusercontent.com/bundy-work/llm-file-index/main/plugin/install.ps1 | iex
```

## Verify

Start Claude Code and run a search — rindex will auto-index your project on first use.

```bash
# Check status
claude mcp get rindex
```
