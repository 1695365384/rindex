---
name: rindex
description: 本地代码索引与语义搜索引擎。提供语义搜索、符号查找、关联代码发现、以及跨会话项目记忆。当需要搜索代码、查找函数/类定义、发现相关联代码、或记录项目知识时使用。
---

# rindex — 本地代码索引使用指南

rindex 是一个本地代码索引 MCP 工具，对当前项目提供语义代码搜索、符号查找、关联代码发现和跨会话记忆。

## 核心工作流

### 1. 项目启动

进入项目目录后，rindex 会自动索引当前项目。索引在后台运行，搜索工具可以立即使用（首次搜索时模型可能还在加载，会退化为纯文本搜索）。

### 2. 代码搜索决策树

```
要找什么？
├─ 知道函数/类/类型名称 → search_symbol(name="FooBar")
├─ 知道大概意图/功能 → search(query="authentication middleware")
├─ 知道一个符号，想找相似代码 → find_related(name="FooBar")
├─ 知道具体文件和行号，想找相关代码 → find_related(file_path="...", line=42)
└─ 只是看项目概况 → project_status()
```

### 3. 搜索查询技巧

- **用自然语言描述意图**，不要用关键词拼凑
  - 好：`"user authentication handler with JWT"`
  - 差：`"auth jwt user"`
- **善用过滤器缩小范围**：
  - `search(query="config", type="rs")` — 只搜 Rust 文件
  - `search(query="test", path="src/")` — 只搜 src 目录
  - `search_symbol(name="handle", chunk_type="function")` — 只搜函数

### 4. 项目记忆

使用 `session_note` 和 `session_context` 可以在不同会话之间保存和读取项目知识：

```
→ session_note(content="使用 sqlx 而不是 diesel 做数据库查询",
                kind="decision")
→ session_note(content="email 字段有唯一约束，批量插入会报错",
                kind="gotcha")
→ session_note(content="cargo test 之前必须先设置 DATABASE_URL",
                kind="pattern")
```

`kind` 可选：`decision`, `pattern`, `bugfix`, `discovery`, `gotcha`

### 5. 索引维护

| 情况 | 操作 |
|------|------|
| 不确定索引进度 | `project_status()` |
| 怀疑索引不同步 | `verify()` |
| 索引损坏/异常 | `reindex()`（后台运行，用 `project_status` 查进度）|

## 何时不用 rindex

- **纯文本匹配** → 用 Grep（rindex 是语义搜索，不适合搜 exact string）
- **找文件路径** → 用 Glob（rindex 搜的是代码内容，不是文件名）
- **刚改完代码立即搜索** → 文件监控有延迟，等一秒再搜

## 工具速查

| 工具 | 用途 | 必需参数 |
|------|------|----------|
| `search` | 语义/关键词搜索代码 | `query` |
| `search_symbol` | 按名称找函数/类/类型 | `name` |
| `find_related` | 找语义相关的代码 | `name` 或 `file_path`+`line` |
| `session_context` | 读取跨会话记忆 | 无 |
| `session_note` | 保存跨会话记忆 | `content` |
| `project_status` | 查看索引状态 | 无 |
| `verify` | 检查索引完整性 | 无 |
| `reindex` | 重建索引（后台） | 无 |
| `backfill` | 补全缺失的 embedding | 无 |
