# rIndex — 本地文件索引 MCP 工具设计

## 概述

rIndex 是一个 Claude Code MCP 插件，使用 Rust 实现，旨在解决 Claude Code 在项目中搜索文件时浪费 token 和时间的问题。通过本地 embedding 模型 + SQLite 向量索引，让项目被索引后可以零重复消耗地快速定位代码。

## 核心约束

- 纯本地运行，不调用任何远程 API
- 仅使用 CPU，无需 GPU/CUDA
- 最小外部依赖
- 高 IO 操作（索引、搜索）由 Rust 实现
- 作为 Claude Code MCP server 运行

## 架构

```
┌──────────────────────────────────────────────┐
│               rIndex (Rust 二进制)             │
│                                                │
│  ┌──────────────┐  ┌──────────┐  ┌──────────┐ │
│  │  MCP Server   │  │ Indexer  │  │ Searcher │ │
│  │  (stdin/std-  │  │          │  │          │ │
│  │   out JSON)   │  │ • tree- │  │ • candle │ │
│  │               │  │   sitter │  │ • 向量化 │ │
│  │               │  │ • chunk  │  │ • 语义搜 │ │
│  │               │  │ • hash   │  │   索     │ │
│  └──────┬───────┘  └────┬──────┘  └─────┬────┘ │
│         └───────────────┼────────────────┘      │
│                         │                       │
│              ┌──────────▼──────────┐            │
│              │  SQLite (sqlite-vec)  │          │
│              │  + notify fs watch     │         │
│              └─────────────────────┘            │
└──────────────────────────────────────────────┘
```

### 组件职责

| 组件 | 职责 |
|------|------|
| MCP Server | 处理 stdin/stdout 的 JSON-RPC 协议，暴露工具接口 |
| Indexer | 遍历项目文件，tree-sitter 解析代码，分块，生成 embedding，写入 SQLite |
| Searcher | 接收搜索请求，转成 embedding，做向量搜索，返回结果 |
| SQLite | 存储文件清单、代码块、向量索引 |
| notify | 监听文件系统变更，触发增量索引 |

## 数据库设计

```sql
-- 项目元数据
CREATE TABLE project (
    id INTEGER PRIMARY KEY,
    root_path TEXT NOT NULL UNIQUE,
    indexed_at INTEGER,
    file_count INTEGER,
    chunk_count INTEGER
);

-- 文件清单
CREATE TABLE files (
    path TEXT PRIMARY KEY,
    hash TEXT NOT NULL,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    language TEXT,
    indexed_at INTEGER
);

-- 代码块索引
CREATE TABLE chunks (
    id INTEGER PRIMARY KEY,
    file_path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
    chunk_type TEXT NOT NULL,   -- function / class / method / interface / module / paragraph
    name TEXT,                  -- 符号名（函数名、类名等）
    signature TEXT,             -- 函数签名（如适用）
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    content TEXT NOT NULL,
    embedding BLOB              -- F32 向量，由 sqlite-vec 管理索引
);

-- 向量索引（sqlite-vec 虚拟表）
CREATE VIRTUAL TABLE vec_chunks USING vec0(
    chunk_id INTEGER PRIMARY KEY,
    embedding FLOAT[384]        -- BGE-small-en-v1.5 使用 384 维
);

-- 文件变更追踪（用于增量索引）
CREATE TABLE file_events (
    path TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,   -- created / modified / deleted
    detected_at INTEGER NOT NULL
);

-- 忽略规则缓存
CREATE TABLE ignore_patterns (
    pattern TEXT PRIMARY KEY,
    source TEXT NOT NULL         -- gitignore / builtin / user
);
```

## 排除策略

优先级：内建默认 > .gitignore > .llm-index-ignore

1. **内建默认排除**
   - `.git/`, `node_modules/`, `target/`, `dist/`, `build/`, `.next/`
   - `.venv/`, `__pycache__/`, `*.pyc`, `*.bin`, `*.exe`, `*.dll`
   - 所有二进制文件（按扩展名 + 文件头检测）
   - 大于 1MB 的文件（可配置）

2. **`.gitignore` 自动读取**
   - 递归读取各级 `.gitignore`

3. **`.llm-index-ignore` 可选文件**
   - 用户自定义忽略，格式同 `.gitignore`
   - 优先级最高

## 索引流程

### 首次索引
1. 读取排除规则，构建忽略集
2. 遍历项目目录，过滤出待索引文件
3. 对每个文件：
   a. 计算内容 hash
   b. tree-sitter 解析 AST
   c. 提取函数/类/接口等符号节点
   d. 对每个符号生成 chunk
   e. 非代码文件按段落分块
4. 批量生成 embedding（candle + BGE-small-en-v1.5）
5. 写入 SQLite + sqlite-vec

### 增量索引（文件变更时）
1. `notify` crate 监听文件系统事件
2. 合并高频事件（防抖 500ms）
3. hash 比对 → 仅变更文件重新索引
4. 新增/变更 → 更新对应 chunk
5. 删除 → 级联删除关联 chunk

### 首次索引的触发时机
项目打开后自动后台启动。如果索引未完成时收到搜索请求，立即返回已索引部分的结果。

## MCP 协议接口

### 工具定义

| 工具名 | 描述 | 参数 |
|--------|------|------|
| `search` | 语义搜索代码，结果精确到函数/类级别 | `query: string`, `limit?: number` |
| `search_symbol` | 按符号名精确搜索 | `name: string`, `type?: string` |
| `project_status` | 查看索引状态 | 无 |
| `reindex` | 手动触发全量重新索引 | 无 |

### `search` 返回值

```json
[
  {
    "file_path": "src/indexer/mod.rs",
    "chunk_type": "function",
    "name": "build_file_tree",
    "signature": "pub fn build_file_tree(root: &Path) -> Result<FileTree>",
    "start_line": 42,
    "end_line": 68,
    "snippet": "pub fn build_file_tree(root: &Path) -> Result<FileTree> {\n    let mut files = Vec::new();\n    ...",
    "score": 0.92
  }
]
```

## embedding 模型选择

**BGE-small-en-v1.5**

| 属性 | 值 |
|------|-----|
| 维度 | 384 |
| 模型大小 | ~33MB |
| 语言 | 英文为主，支持多语言 |
| CPU 性能 | 单次 embedding ~5-10ms |
| 许可证 | MIT |

模型文件通过 `candle` 在首次启动时自动下载到本地缓存目录 (`~/.cache/rindex/`)。

## CLAUDE.md 集成策略

项目索引完成后，在项目根目录生成/更新 `CLAUDE.md`（创建 if not exists），内容：

```markdown
## 文件搜索
使用 `search` 工具进行语义搜索来查找项目中的代码和文件，
它能精确到函数/类级别，比 Glob/Grep 更高效。

可用命令：
- `search` - 语义搜索代码
- `search_symbol` - 按符号名搜索
- `project_status` - 查看索引状态
```

工具的描述文本本身（`description` 字段）会引导 Claude 优先使用它。

## 项目结构

```
rindex/
├── Cargo.toml
├── src/
│   ├── main.rs              # 入口 + MCP JSON-RPC loop
│   ├── mcp/
│   │   └── mod.rs           # MCP 协议处理、工具注册
│   ├── indexer/
│   │   ├── mod.rs           # Indexer 编排
│   │   ├── walker.rs        # 文件遍历 + 排除规则
│   │   ├── parser.rs        # tree-sitter AST 解析
│   │   └── chunker.rs       # 代码分块策略
│   ├── search/
│   │   ├── mod.rs           # Searcher 编排
│   │   └── vector.rs        # 向量搜索逻辑
│   ├── embedding/
│   │   └── mod.rs           # candle embedding 推理
│   ├── db/
│   │   ├── mod.rs           # SQLite 初始化 + migrations
│   │   ├── files.rs         # 文件清单 CRUD
│   │   └── chunks.rs        # chunks CRUD + 向量搜索
│   ├── watcher.rs           # notify 文件系统监听
│   ├── ignore.rs            # 忽略规则解析（gitignore 等）
│   └── config.rs            # 配置管理
├── models/                   # embedding 模型文件缓存
└── fixtures/                 # 测试用项目模板
```

## Rust 依赖

```toml
[dependencies]
# ML
candle-core = "0.8"
candle-nn = "0.8"
candle-transformers = "0.8"
hf-hub = "0.4"              # HuggingFace 模型下载

# 文件解析
tree-sitter = "0.24"
tree-sitter-rust = "0.22"
tree-sitter-python = "0.21"
tree-sitter-javascript = "0.22"
tree-sitter-typescript = "0.22"
tree-sitter-go = "0.21"

# 存储
rusqlite = { version = "0.32", features = ["bundled"] }
sqlite-vec = "0.1"

# 文件系统
notify = "7.0"

# 工具
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokenizers = "0.20"         # BGE 模型 tokenizer
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
```

## 启动与生命周期

1. Claude Code 启动，读取 `claude.json` 中配置的 MCP server 命令
2. rIndex 二进制启动，初始化 SQLite 数据库
3. 检查 `project` 表是否存在当前项目
   - 存在且 hash 未变 → 零操作
   - 存在但有变更 → 增量索引
   - 不存在 → 后台全量索引
4. 启动 `notify` 文件监听器
5. 进入 MCP JSON-RPC 事件循环，等待工具调用
6. Claude Code 关闭时，rIndex 进程自动退出

## 性能目标

| 指标 | 目标 |
|------|------|
| 首次索引 (1000 文件项目) | < 30 秒 |
| 增量索引 (单文件变更) | < 100ms |
| 语义搜索 | < 50ms |
| 二进制体积 | < 20MB |
| 内存占用 | < 200MB |
| 模型加载时间 | < 2 秒 |
