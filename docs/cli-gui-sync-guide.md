# CLI ↔ GUI 同步指南

> 本文档记录 vercel-skills CLI 与 skill-deck GUI 之间的架构差异和互操作约束。
> **每次从上游同步变更时必须查阅此文档。**

---

## 1. Lock 文件

### 1.1 全局 Lock (`~/.agents/.skill-lock.json`)

**路径**: `~/.agents/.skill-lock.json`（支持 `$XDG_STATE_HOME/skills/.skill-lock.json`）

两边结构基本一致，但 GUI 的 `SkillLockEntry` 可能包含额外字段（以可选方式添加）。

| JSON 字段 | CLI 类型 | GUI 类型 | 说明 |
|-----------|---------|---------|------|
| `source` | `string` | `String` | 一致 |
| `sourceType` | `string` | `String` | 一致 |
| `sourceUrl` | `string` | `String` | 一致 |
| `ref` | `string?` | `Option<String>` (serde rename) | v1.4.7+ 新增，GUI 字段名 `ref_name` |
| `skillPath` | `string?` | `Option<String>` | 一致 |
| `skillFolderHash` | `string` | `String` | 一致 |
| `installedAt` | `string` | `String` | 一致 |
| `updatedAt` | `string` | `String` | 一致 |
| `pluginName` | `string?` | `Option<String>` | 一致 |

**互操作规则**:
- CLI 使用 `JSON.parse()` / `JSON.stringify()`，自动保留所有未知字段
- GUI 使用 `serde_json`，默认忽略未知字段（不需要 `deny_unknown_fields`）
- ✅ 双向兼容，互不破坏

### 1.2 本地 Lock (`<project>/skills-lock.json`)

**路径**: `<project>/skills-lock.json`（**CLI 和 GUI 读写同一文件**）

这是最关键的互操作点。两边结构有显著差异：

| JSON 字段 | CLI | GUI | 差异原因 |
|-----------|-----|-----|---------|
| `source` | ✅ 必需 | ✅ 必需 | — |
| `ref` | ✅ 可选 (v1.4.7+) | ✅ 可选 (`ref_name`) | GUI 需同步新增字段 |
| `sourceType` | ✅ 必需 | ✅ 必需 | — |
| `computedHash` | ✅ 必需 | ✅ 必需 | 本地文件 SHA-256 |
| `sourceUrl` | ❌ 不存在 | ✅ 可选 | **GUI 扩展**: 保留原始 URL |
| `remoteHash` | ❌ 不存在 | ✅ 可选 | **GUI 扩展**: GitHub tree SHA，用于更新检测 |
| `skillPath` | ❌ 不存在 | ✅ 可选 | **GUI 扩展**: 仓库内 skill 子路径 |
| `pluginName` | ❌ 不存在 | ✅ 可选 | **GUI 扩展**: 所属 plugin |

**关键设计差异**:

1. **CLI 的本地 lock 是精简设计** — 只记录 source、sourceType、computedHash，专为 git 版本控制优化（最小化 merge conflict）
2. **GUI 的本地 lock 是扩展设计** — 额外存储 remoteHash、skillPath 等字段，支持项目级 skill 更新检测（CLI 不支持本地 skill 更新）
3. **GUI 扩展字段使用 `skip_serializing_if = "Option::is_none"`** — 当值为 None 时不序列化，减少 JSON 体积

**互操作风险点**:

| 场景 | 风险 | 结果 |
|------|------|------|
| CLI 写 → GUI 读 | 低 | GUI 扩展字段为 None，更新检测可能降级 |
| GUI 写 → CLI 读 | 无 | CLI 忽略未知字段 |
| CLI 写 → GUI 写同一 skill | ⚠️ | **如果 GUI 缺少某个 CLI 字段，会丢失该字段。** 所以 CLI 新增字段时 GUI 必须同步 |
| GUI 写 → CLI 写同一 skill | 低 | CLI 的 `JSON.stringify` roundtrip 保留 GUI 扩展字段 |

**同步规则（MUST）**:
- CLI 本地 lock entry 新增字段 → GUI `LocalSkillLockEntry` 必须同步添加对应 `Option<T>` 字段 + `serde(rename)` + `skip_serializing_if`
- GUI 新增扩展字段 → 使用 `Option<T>` + `skip_serializing_if`，不影响 CLI
- 永远不要在 GUI 侧使用 `#[serde(deny_unknown_fields)]`

### 1.3 旧版 Lock 兼容

GUI 支持读取旧版项目级 lock 路径 `<project>/.agents/.skill-lock.json`（全局 lock 格式），自动转换为新格式。CLI 无此兼容逻辑。

---

## 2. Source Parser

| 格式 | CLI (source-parser.ts) | GUI (source_parser.rs) |
|------|----------------------|----------------------|
| GitHub shorthand | `parseSource()` | `parse_source()` |
| Fragment ref `#` | `parseFragmentRef()` (v1.4.7+) | 需同步实现 |
| Source aliases | `SOURCE_ALIASES` 对象 | `SOURCE_ALIASES` HashMap |

**同步规则**:
- CLI 新增源格式 → GUI `parse_source()` 必须同步
- CLI 新增 alias → GUI `SOURCE_ALIASES` 必须同步
- Fragment ref 逻辑要保持一致：先提取 `#`，再处理 `@skill-filter`

---

## 3. Agent 注册表

| 方面 | CLI (agents.ts) | GUI (agents.rs) |
|------|----------------|-----------------|
| 类型定义 | `AgentType` union type | `AgentType` enum |
| 配置 | `agents` Record | `AgentType::config()` method |
| 检测 | `detectInstalled` async function | `AgentType::detect()` async method |

**同步规则**:
- CLI 新增 agent → GUI `AgentType` enum 新增 variant + `config()` 实现 + `detect()` 实现
- 注意检查 `skills_dir`、`global_skills_dir`、`show_in_universal_list` 是否一致
- 更新 agent 计数测试断言
- GUI 的 `AgentType` enum 有 `Universal` variant（CLI 无此概念），不影响同步

---

## 4. Well-Known Protocol

| 方面 | CLI (wellknown.ts) | GUI (wellknown.rs) |
|------|-------------------|-------------------|
| 路径 | `WELL_KNOWN_PATHS` 数组 | `WELL_KNOWN_PATHS` 切片 |
| 探测顺序 | `agent-skills` → `skills` fallback | 需同步 |
| Index 结构 | `WellKnownIndex` | `WellKnownIndex` |

**同步规则**:
- CLI 变更 well-known 路径 → GUI 常量必须同步
- CLI 变更探测顺序 → GUI `build_index_urls()` 必须同步

---

## 5. Discovery 搜索路径

CLI 和 GUI 都维护一份优先搜索目录列表。变更时需双向同步。

**同步规则**:
- CLI 新增/移除搜索路径 → GUI `get_priority_search_dirs()` 必须同步

---

## 6. 文件排除规则

| 规则 | CLI (installer.ts) | GUI (installer.rs) |
|------|-------------------|--------------------|
| 排除文件 | `EXCLUDE_FILES` Set | `EXCLUDE_FILES` 切片 |
| 排除目录 | `EXCLUDE_DIRS` Set | `EXCLUDE_DIRS` 切片 |
| 前缀排除 | `.` 开头 (dotfiles) | `.` 开头 (dotfiles) |
| Broken symlink | warn + skip | warn + skip |

**同步规则**:
- CLI 变更排除列表 → GUI 必须同步

---

## 7. 同步检查清单

每次从 vercel-skills 上游同步时，按此清单检查：

- [ ] **types.ts** → `AgentType` 枚举是否有新增/移除
- [ ] **agents.ts** → agent 配置（skills_dir, global_skills_dir, detect）是否有变更
- [ ] **source-parser.ts** → 是否有新的源格式或别名
- [ ] **skill-lock.ts** → `SkillLockEntry` 是否有新字段
- [ ] **local-lock.ts** → `LocalSkillLockEntry` 是否有新字段（**最重要，同一文件路径**）
- [ ] **installer.ts** → 文件排除规则是否变更
- [ ] **providers/wellknown.ts** → well-known 路径或协议是否变更
- [ ] **skills.ts / discovery** → 搜索路径或发现算法是否变更
- [ ] **cli.ts** → 新增 CLI 命令或参数是否需要 GUI 对应功能
