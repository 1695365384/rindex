<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-000000?style=for-the-badge&logo=rust" />
  <img src="https://img.shields.io/badge/license-MIT-8B5CF6?style=for-the-badge" />
  <img src="https://img.shields.io/badge/inference-CPU--only-10B981?style=for-the-badge" />
</p>

# rindex

> 本地代码索引与语义搜索引擎，为 LLM 编程智能体打造。
> 别再 grep 了。让你的 AI 真正理解代码。

**rindex** 在本地为整个项目构建代码索引，通过 MCP（Model Context Protocol）提供语义搜索。让 Claude Code 用 **1-2 次工具调用**完成原来需要 5-7 次 grep + 逐文件阅读才能找到的代码。

---

## 核心价值

### 你的上下文窗口，是最贵的资源

Claude Code 的上下文窗口是 200K token。听起来很多，但一次 grep 返回 3,000 token 的噪音——**20 次搜索就能把 30% 的上下文烧光**，而且全是无用信息。Claude 不是在理解你的代码，是在翻废纸。

rindex 把每次搜索的输出从 ~3,000 token 压缩到 ~70 token。**省下来的空间，Claude 拿去做真正的事：理解业务逻辑、追踪 bug 根因、生成正确的代码。**

### 算一笔账

| | Grep/Glob | rindex | 节省 |
|:---|---:|---:|---:|
| **每次搜索调用次数** | 5 – 7 次 | 1 – 2 次 | **~70%** |
| **每次搜索响应 token** | ~3,000 | ~70 | **~98%** |
| **每日 20 次搜索** | ~60,000 tok / 140 次调用 | ~1,400 tok / 40 次调用 | **58,600 tokens** |
| **每日成本 (Claude Opus)** | ~$0.90 | ~$0.02 | **$0.88/天** |
| **每月成本 (个人)** | ~$19.80 | ~$0.44 | **$19/月** |
| **每月成本 (5 人团队)** | ~$99 | ~$2.20 | **$97/月** |
| **每月成本 (20 人团队)** | ~$396 | ~$8.80 | **$387/月** |
| **等待时间 (每次搜索)** | 5 – 7 轮往返 | 1 – 2 轮 | **响应快 3-5 倍** |

> **5 人团队，每月省 $97，每年省 $1,164。** 这还没算 Claude 因为上下文更干净而产出的更高质量的代码——少改一轮 bug，就值回票价。

### 性能基准

| 操作 | 延迟 |
|:---|---:|
| 符号搜索 (1,000 chunks) | **1.7 µs** |
| 解析 500 行 Rust 文件 | **1.36 ms** |
| 扫描 38 文件项目 | **836 µs** |

### 一句话总结

> rindex 不是"更好的 grep"。它改变了 LLM 编程智能体的工作方式：
> **从"在噪音里翻找"变成"直接拿到答案"**。省下的 token 和时间，都变成更好的代码。

---

## 功能

- **语义搜索** — 按含义搜索代码，不是匹配字符串。搜"数据库连接配置"能找到 `DBPool::new()`
- **符号查找** — 微秒级定位函数、结构体、trait、类。比 grep 快 1000 倍
- **find_related** — 发现项目中概念相似的代码。一次调用替代多轮 grep 拼凑
- **自动索引** — 首次打开项目自动扫描，后续增量同步，零配置
- **.llm-index-ignore** — 自定义排除规则（配合 .gitignore 使用）
- **跨会话记忆** — 保存项目决策、踩坑、模式，下次对话自动注入，告别重复解释
- **100% 本地** — 无 API 密钥、无云服务、无遥测。静态 token 嵌入纯 CPU 运行，无需 GPU

---

## 安装

### 一键安装（推荐）

下载发布包，解压，双击安装脚本：

| 平台 | 包名 | 安装方式 |
|---|---|---|
| **Windows 11** | `rindex-v1.0.0-windows-x64.zip` | 双击 `install.bat` |
| **macOS** | `rindex-v1.0.0-macos-arm64.tar.gz` | 双击 `install.command` |

二进制、嵌入模型、PATH 一步到位。装完后打开你的编码智能体进入任意项目，rindex 自动检测客户端并完成全部配置。零手动步骤。

**支持的客户端**: Claude Code、Cursor、Windsurf，以及任何兼容 MCP 协议的智能体。

### 从源码构建

```bash
cargo build --release && cp target/release/rindex ~/.local/bin/
rindex backfill  # 用静态模型为所有代码块生成向量（~166MB，发布包内含）
```

然后打开 Claude Code — rindex 自动处理其余一切。

---

## 客户端支持

任何兼容 MCP 的编码智能体均可使用。rindex 自动检测并配置：

| 客户端 | 配置文件 | 配置方式 |
|---|---|---|
| **Claude Code** | `.mcp.json` + `.claude/skills/` | 自动 |
| **Cursor** | `.cursor/mcp.json` | 自动 |
| **Windsurf** | `.windsurf/mcp.json` | 自动 |
| **其他 MCP 客户端** | 标准 MCP JSON 配置 | 指向 `rindex` |

所有客户端共享同一索引数据库 — 随时切换，无需重建索引。

## 可用工具

| 工具 | 用途 |
|:---|:---|
| `search` | 语义 + 关键词混合搜索已索引代码 |
| `search_symbol` | 按名称精确查找符号 — 函数、类、类型、trait |
| `find_related` | 发现与某个符号或文件位置语义相似的代码 |
| `project_status` | 查看索引进度、文件/块数量、模型状态 |
| `reindex` | 从头重建整个项目索引（后台运行） |
| `backfill` | 手动为缺失嵌入的代码块生成向量 |
| `verify` | 检查索引完整性 — 报告过期条目和缺失文件 |
| `session_note` | 保存持久项目记忆（设计决策、踩坑、模式） |
| `session_context` | 读取历史会话中保存的项目记忆 |

---

## 工作原理

```
索引管线
────────────────────────────────────────────────────
  源文件   →  tree-sitter AST  →  函数/类级   →  静态嵌入表  →  SQLite
              解析符号            代码块          (查表,无框架)    存储
```

1. **扫描** — 文件遍历器根据 `.gitignore` + `.llm-index-ignore` 排除无关文件
2. **分块** — tree-sitter 将代码拆分为符号级代码块（函数、结构体、类）
3. **嵌入** — 自研蒸馏的静态 token 嵌入表（纯 CPU，~0.01ms/chunk — 就是查表，没有 transformer）
4. **搜索** — 查询向量化 → 余弦相似度重排序 → 紧凑文本输出

搜索采用**两阶段混合策略**：FTS5 全文预过滤（AND 语义）缩小候选集 → 嵌入相似度重排序。避免将所有向量加载到内存，支持 10 万+ 代码块的项目。

---

## 配置

### CLI

```
rindex [OPTIONS]

  -p, --path <PATH>       项目根目录 (默认: 当前目录)
      --db <PATH>         数据库路径 (默认: ~/Library/Application Support/rindex/rindex.db)
      --max-size <BYTES>  索引文件大小上限 (默认: 1 MB)
      --model-id <ID>     静态模型目录 (默认: c2llm-static-256)
      --no-model          跳过嵌入模型 — 仅文本搜索
```

### `rindex.toml`

```toml
max_file_size = 2097152           # 2 MB
model_id = "c2llm-static-256"    # model_cache_dir 下的本地目录
default_search_limit = 20
watcher_debounce_ms = 500
```

---

## 系统要求

- **Rust 1.75+**
- **不需要 GPU** — 纯 CPU 推理，任何机器都能跑
- **~170 MB** 磁盘用于模型缓存（token_embeddings.safetensors + tokenizer.json）
- **~50 MB** 磁盘每 1 万个索引块（SQLite + 向量数据）

---

## 许可证

MIT
