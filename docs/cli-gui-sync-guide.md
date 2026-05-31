# CLI ↔ GUI 同步指南

> 本文档记录 vercel-skills CLI 与 skill-deck GUI 之间的当前同步规则、架构差异和互操作约束。
> 每次从上游 CLI 同步变更后，必须按本文档检查并更新 GUI 实现与本文档本身。

---

## 0. 维护原则

- CLI 是兼容性和安全语义基线，不是 GUI 的功能上限。GUI 可以在符合 CLI 可读、可写、可解释语义的前提下扩展能力，例如项目级更新检测、批量操作、缓存、状态展示、修复入口、更新计划和更细的错误提示。
- 当前兼容基线为 skills CLI v1.5.9。该基线包含 Zed agent、SSH private git 来源保真、有限 depth-2 skill 发现、项目 lock 过滤 CLI agent 目录中的已安装 skill，以及 `deleted-upstream` 维护状态。
- 本文档只描述当前应保持一致的状态，不记录某个 CLI 发布版本的迁移流水账。一次性升级细节应放在 plan、spec 或 review 文档中。
- 上游 CLI 每次变更后，先对照第 8 节清单检查代码，再把本文档更新为新的当前态。
- GUI 扩展字段必须保持向前兼容：使用可选字段、缺省值和 `skip_serializing_if`，不要使用 `deny_unknown_fields`。
- GUI 读写 CLI 共享 JSON 时必须保留未知字段。优先使用 `serde(flatten)` 或基于原始 JSON 的局部更新；如果某条写路径暂时无法保留未知字段，必须先把字段同步到模型再允许写回。
- 如果 GUI 为体验优化引入额外状态，必须能解释它和 CLI 基础语义的关系。例如 `skipped` 是“未安装到该 agent”，不能算作成功安装。

---

## 1. Lock 文件

### 1.1 全局 Lock

**路径**：`~/.agents/.skill-lock.json`，并支持 `$XDG_STATE_HOME/skills/.skill-lock.json`。

全局 lock 记录全局安装的 skill 元数据。GUI 读取和写入时应保持 CLI 字段语义一致，GUI 额外字段只能以可选方式扩展。

| JSON 字段 | GUI 字段 | 说明 |
| --- | --- | --- |
| `source` | `source` | 规范化来源标识，例如 `owner/repo` |
| `sourceType` | `source_type` | 来源类型，例如 `github`、`local`、`well-known` |
| `sourceUrl` | `source_url` | 原始安装 URL |
| `ref` | `ref_name` | 分支或 tag，serde 需要 rename |
| `skillPath` | `skill_path` | 仓库内 skill 子路径，更新定位依赖该字段 |
| `skillFolderHash` | `skill_folder_hash` | 来源版本追踪 hash。GitHub 来源通常是远端 tree SHA；非 GitHub 来源可能是安装来源/本地内容 hash，不能默认当作远端可检查信号 |
| `installedAt` | `installed_at` | 安装时间 |
| `updatedAt` | `updated_at` | 更新时间 |
| `pluginName` | `plugin_name` | 所属 plugin 名称 |

全局 lock 顶层还包含两个安装偏好字段：

| JSON 字段 | 归属 | 说明 |
| --- | --- | --- |
| `lastSelectedAgents` | CLI/GUI | CLI 的最后选择记录。GUI 写入默认安装目标时会同步写入 global/project 默认目标的并集，用于保持 CLI fallback 体验 |
| `defaultTargetAgents` | GUI 扩展 | GUI 的 scope-aware 默认安装目标，结构为 `{ global: string[], project: string[] }`。只保存需要额外投放的 Agent；当前 scope 下自动读取共享目录的 Agent 不进入该字段 |

互操作规则：

- CLI 使用 JSON roundtrip 时应保留未知字段。
- GUI 使用 serde 时应忽略未知字段，并在写回时保留未知字段和当前 GUI 已建模的字段。
- CLI 新增全局 lock 字段后，GUI 的 `SkillLockEntry` 仍需要同步建模，便于前端展示、校验和类型生成；未知字段保留机制是兜底，不是替代同步。
- 如果 CLI 写回 lock 时未保留 `defaultTargetAgents`，GUI 必须能从 `lastSelectedAgents` 迁移出可用默认值，而不是把配置视为损坏。

### 1.2 项目 Lock

**路径**：`<project>/skills-lock.json`。CLI 和 GUI 读写同一个文件，这里是最重要的互操作点。

项目 lock 设计目标是可提交到 git，所以字段应尽量稳定、精简、排序可预测。本地 `vercel-skills` CLI 当前已经在 project lock 中记录 `skillPath`，并使用它支持项目级定点 reinstall/update。GUI 可以在此基础上保留额外元数据，用于远端更新检测、私有来源保真、插件展示和修复流，但必须保持 CLI 可读可写。

项目 lock 的兼容模型分两层：

- **共享契约字段**：CLI 和 GUI 都必须保持同一语义，包括 `source`、`ref`、`sourceType`、`skillPath`、`computedHash`。这些字段决定 CLI 是否能恢复/更新项目 skill。
- **GUI 增强字段**：只增强 GUI 能力，包括 `sourceUrl`、`remoteHash`、`pluginName`。缺失这些字段不能破坏 CLI 更新能力，只能让 GUI 降级，例如不能提前检测远端更新、展示信息变少或需要修复来源。

| JSON 字段 | 归属 | 说明 |
| --- | --- | --- |
| `source` | CLI/GUI | 规范化来源标识 |
| `ref` | CLI/GUI | 分支或 tag，GUI 字段为 `ref_name` |
| `sourceType` | CLI/GUI | 来源类型 |
| `computedHash` | CLI/GUI | 本地文件内容 SHA-256 |
| `skillPath` | CLI/GUI | 仓库内 skill 子路径，project scope 安全 update 依赖该字段 |
| `sourceUrl` | GUI 扩展 | 原始来源 URL，用于 SSH/private repo 等场景保真 |
| `remoteHash` | GUI 扩展 | 远端版本追踪 hash。当前主要用于 GitHub 项目级更新检测；非 GitHub 来源只有在实现了对应远端检测器时才能依赖它判定更新 |
| `pluginName` | GUI 扩展 | 所属 plugin 名称 |

关键约束：

- 缺 `skillPath` 的旧 entry 不能安全定位上游目录。所有 lock 驱动的普通 update 都必须禁用，标记为 `missing-skill-path`，并通过“修复来源”流程重新选择来源并刷新 lock。
- 缺 `remoteHash` 时，GitHub 来源可以执行 reinstall 语义的 update，但 GUI 不能提前判断远端是否有更新，应标记为 `missing-remote-hash`。
- 非 GitHub 来源可以保留本地 `computedHash`、安装来源 hash 或 GUI 扩展 hash 用于审计和未来检测；只有实现了该 source type 的远端查询/比较逻辑后，才能把它作为自动更新检测信号。
- CLI 的 project `add/update` 会用新的 `LocalSkillLockEntry` 替换对应 skill entry。当前 CLI entry 只包含共享契约字段，因此这类写入可能丢失 GUI 增强字段。GUI 必须把这种情况当作可恢复降级，而不是 lock 损坏。
- GUI 写回项目 lock 时不能改变共享契约字段的 CLI 语义。`sourceUrl` 可以用于 GUI 保真和展示，但不能替代 `source` 成为 CLI 重新安装语义的唯一来源。
- GUI 新增扩展字段时必须使用 `Option<T>` 和 `skip_serializing_if`，避免污染项目 lock。
- CLI 新增项目 lock 字段后，GUI 必须同步到 `LocalSkillLockEntry`，并确认未知字段保留测试覆盖该写路径。要做到强无损兼容，GUI 需要 `serde(flatten)` 或原始 JSON 局部更新；CLI 侧则需要在替换 entry 前 merge 旧 entry 的未知字段。

### 1.3 旧版 Lock 兼容

GUI 支持读取旧版项目级 lock 路径 `<project>/.agents/.skill-lock.json` 并转换为当前项目 lock 结构。CLI 不需要实现这条兼容路径。

---

## 2. Source Parser

CLI 和 GUI 都需要解析相同的 source 输入形态，包括 GitHub shorthand、URL、fragment ref、skill filter 和 alias。

同步规则：

- CLI 新增源格式后，GUI `parse_source()` 必须同步。
- CLI 新增或修改 alias 后，GUI alias 表必须同步。
- Fragment ref 的处理顺序必须一致：先提取 `#ref`，再处理 `@skill-filter`。
- 解析结果中的 `source`、`source_type`、`source_url`、`ref`、`skill_path` 必须能回写到 lock，并能被 update 链路复用。
- `skillPath` 的规范形态是仓库内 `SKILL.md` 文件路径，例如 `skills/foo/SKILL.md`；需要目录路径时由调用方显式去掉 `SKILL.md` 后缀。
- `git@host:org/repo.git` 和 `ssh://git@host[:port]/org/repo.git` 都是 private git source。写入 lock 时必须保留原始 `git@` 或 `ssh://` 来源，不要规范化为 GitHub shorthand；fragment ref 仍应从 lock source 中剥离到 `ref` 字段。

---

## 3. Agent 注册表

CLI 的 agent registry 是 GUI `AgentType` 的配置基线，但 GUI 不再直接复用 CLI 的 `universal` 分类作为行为模型。

同步规则：

- CLI 新增 agent 后，GUI 需要新增 `AgentType` variant、`config()`、`detect()` 和相关测试。
- 必须检查 `skills_dir`、`global_skills_dir` 和检测逻辑是否与 CLI 一致。
- 当前 CLI v1.5.9 兼容集包含 Zed。Zed 的 global/project canonical 目录都是 `.agents/skills` 语义：global 指向 `~/.agents/skills`，project 指向 `<project>/.agents/skills`。
- CLI 的 `universal` 列表是静态分类：`skillsDir === ".agents/skills"`，并且 `showInUniversalList !== false` 时才会进入可选/自动安装目标。CLI 仍包含一个 `universal` agent key。
- GUI 的 `automatic` 是 scope-aware 运行时分类：某个 Agent 在当前 scope 下的目标目录等于 canonical 共享目录时，才算自动可用。全局 canonical 是 `~/.agents/skills`，项目 canonical 是 `<project>/.agents/skills`。
- 因为 GUI 按 scope 判断，同一个 Agent 可以在项目中自动可用、在全局中需要额外投放。例如 Antigravity 的 project target 是 `.agents/skills`，global target 是 `~/.gemini/antigravity/skills`。
- GUI 不保留 CLI 的 `universal` agent key，也不把它写入 lock 或当作真实安装目标。`showInUniversalList` 会影响 CLI 的 universal 列表是否纳入安装与 sync 目标，但 GUI 不能把这个静态列表直接当成当前 scope 的自动目标规则。
- GUI 默认安装设置只保存额外 Agent。自动可用 Agent 由后端按当前 scope 动态补齐，用户不需要也不应该手动保存它们。
- 项目级安装或 update 不应创建 CLI 不会自动创建的额外 Agent 根目录；这种结果应作为 `skipped` 返回给 GUI。
- GUI 中用户显式选择“为某个 Agent 添加/管理 Skill”时，可以作为体验优化创建目标目录，但这属于显式操作，不应影响 CLI 基线规则。

---

## 4. Well-Known Protocol

CLI 和 GUI 都维护 well-known endpoint 的路径、探测顺序和 index 校验逻辑。

同步规则：

- CLI 变更 well-known 路径后，GUI `WELL_KNOWN_PATHS` 必须同步。
- CLI 变更探测顺序后，GUI 构造 URL 的顺序必须同步。当前语义是优先 `agent-skills`，再 fallback 到 `skills`。
- legacy `files[]` 继续兼容，但路径校验必须和 CLI 一致。任何非法路径都应使整个 legacy index 无效，而不是只跳过单个 entry。
- legacy path 必须拒绝空路径、绝对路径、路径穿越、包含 `..` 的路径和 null byte。
- well-known v2 的 `skill-md` 与 `archive` artifact 必须校验 `sha256:` digest。
- archive 解压必须拒绝路径穿越、绝对路径、过多文件和超大内容。
- GUI 可以展示 artifact type、artifact host、legacy/v2 和 digest verified 状态，但展示优化不能放宽安装校验。

---

## 5. Discovery 搜索路径

CLI 和 GUI 都维护 skill 发现的优先搜索路径。

同步规则：

- CLI 新增、移除或重排搜索路径后，GUI 的搜索路径必须同步。
- `SKILL.md` 识别应保持大小写兼容，同时保留磁盘上的实际路径大小写写入 lock。
- 同仓库存在多个 skill 时，更新链路应优先用 lock 中的 `skillPath` 精确定位，而不是只按 skill name 匹配。
- 已知 skill 容器目录支持有限 depth-2 发现，例如 `skills/<name>/SKILL.md`、`examples/<name>/SKILL.md` 等；默认发现不应无限递归。
- 当用户直接选择项目中的 CLI agent skill 目录作为 source candidate 时，GUI 必须过滤已被 `<project>/skills-lock.json` 跟踪的 skill。过滤范围包括项目根、`.agents/skills` 以及 `.agents/skills/<name>` 这类 direct subpath，避免把已安装项误当成可安装来源。

---

## 6. 文件复制与排除规则

安装时的复制规则必须和 CLI 保持一致。

同步规则：

- CLI 变更排除文件或排除目录后，GUI 对应常量必须同步。
- dotfiles 和 dotdirs 默认应保留，例如 `.env.example`、`.rules`。
- 只排除明确的元数据、缓存或 VCS 内容，例如 `.git`、`metadata.json`、`__pycache__`、`__pypackages__`。
- Broken symlink 应 warn 并跳过，不能中断整个安装。
- copy、symlink、junction 的 fallback 和错误展示可以由 GUI 优化，但最终结果必须明确反映成功、失败或跳过。

---

## 7. 更新链路同步边界

更新语义是“重新安装当前来源的最新内容”，不是增量 patch。

同步规则：

- CLI 变更 update/add/reinstall 的核心语义后，GUI update 链路必须同步。
- GUI 可以做更进一步的优化和产品化能力，包括项目级远端更新检测、批量 clone/API 复用、更新计划、缓存、进度事件、错误分类、失败重试和修复来源。约束是：对同一个 lock entry，GUI 最终安装的来源、ref、`skillPath`、目标 agent 和 lock 写回应能解释为 CLI reinstall/update 语义的兼容扩展。
- 所有 lock 驱动的普通 update 都必须依赖 `skillPath` 定位来源。缺 `skillPath` 时不要按 skill name 猜测路径；用户显式发起“修复来源”才可以重新发现目录并刷新 lock。
- Project scope 下，CLI 的 update 语义是“有 `skillPath` 就定点 reinstall”，不是“先检测远端是否变化”。GUI 的 `remoteHash` 检测是额外体验，不能成为执行 reinstall 的必要条件。
- GUI update 在找不到已安装 Agent 时，只能回退到当前 scope 下自动读取共享目录的 Agent，不能回退到 CLI 的静态 universal 列表。
- GUI update 应保留每个 agent 原本的安装模式，除非用户显式改变。
- 每个 agent 的结果需要区分 `success`、`failed`、`skipped`。`skipped` 不能计入安装成功覆盖率。
- Skill 总状态中，`success + skipped` 的归并规则必须显式：如果 `skipped` 表示“不属于实际目标”，可以整体视为 `success`；如果 `skipped` 表示请求目标未被处理，应归为 `partial` 或 `skipped`，并且不得清除用户重试入口。
- update 失败或 partial 时不应把缓存强行写成 up-to-date。只有符合 GUI 定义的完整成功后才能清除该 skill 的 update 标记。
- 更新检测缓存和前端合并不能只以 skill name 作为身份。至少需要同时考虑 scope、project path、source/sourceUrl、ref 和 `skillPath`，避免旧缓存污染修复来源或换 ref 后的新安装项。
- `deleted-upstream` 是维护状态：来源可访问，但 lock 中的 `skillPath` 在上游已经不存在。它不会触发普通 update，也不会自动删除本地文件；GUI 只能提示用户删除本地副本、修复来源或保留当前安装。

---

## 8. 同步检查清单

每次从上游 CLI 同步时，按此清单检查：

- [ ] `types.ts`：agent 类型是否新增、移除或重命名。
- [ ] `agents.ts`：agent 配置、检测逻辑、global/project 目录、`showInUniversalList` 是否变化；同时检查 GUI 的 scope-aware target/automatic 推导是否仍符合预期。
- [ ] `source-parser.ts`：source 格式、alias、ref/filter 解析顺序是否变化。
- [ ] `skill-lock.ts`：全局 lock 字段、版本、默认值和未知字段保留策略是否变化。
- [ ] `local-lock.ts`：项目 lock 字段、排序、写回策略和未知字段保留策略是否变化。
- [ ] `installer.ts`：copy/symlink/junction、排除规则、agent 目标选择是否变化。
- [ ] `providers/wellknown.ts`：well-known 路径、index schema、artifact 校验是否变化。
- [ ] `skills.ts` 或 discovery 相关模块：搜索路径、`SKILL.md` 识别和多 skill 匹配逻辑是否变化。
- [ ] `cli.ts`：`add`、`update`、`check`、`upgrade` 命令参数和行为是否变化。
- [ ] GUI Rust models：对应 lock、install、update、agent 类型是否同步。
- [ ] GUI TypeScript bindings：Rust 类型变更后 bindings 是否同步。
- [ ] GUI UI：新增状态、reason、warning、skipped 是否有清晰展示。
- [ ] Tests：Rust 单元测试、前端 store/component 测试是否覆盖同步差异，包括未知字段保留、缓存 identity、`missing-skill-path` 和 `success/skipped` 归并。
- [ ] 本文档和 [skill-update-design.md](./skill-update-design.md) 是否已更新为新的当前态。
