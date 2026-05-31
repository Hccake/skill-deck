# Skill 更新机制设计文档

本文档描述 vercel-skills CLI 与 Skill Deck 桌面端在“skill 更新”链路上的当前设计、差异边界和维护注意事项。

配套阅读：[CLI ↔ GUI 同步指南](./cli-gui-sync-guide.md)。

---

## 1. 当前设计摘要

| 维度 | vercel-skills CLI | Skill Deck GUI |
| --- | --- | --- |
| 更新语义 | 重新执行安装流程 | 同样是重新安装语义，由 Rust 内联执行 |
| 全局更新检测 | GitHub tree SHA | GitHub tree SHA |
| 项目更新检测 | 有 `skillPath` 的 entry 可定点 reinstall/update；legacy entry 仅提示刷新来源 | 在 CLI 语义上增加远端检测能力，当前主要使用 `skillPath` + GUI 扩展的 `remoteHash` 判断 GitHub 项目 skill 是否有更新 |
| 不可检查状态 | CLI 输出 skip/reason | 类型化为 capability 和 check status |
| 缓存 | 无 | 进程内 TTL 缓存 |
| 进度反馈 | 终端输出 | Tauri event + 前端局部状态 |
| 批量优化 | 按 CLI 流程逐项处理 | 同源同 ref 的 skill 可共享 clone/API 调用 |

核心语义：更新 = 重新安装。不存在增量更新，也不应该只 patch 某几个文件。

GUI 的职责是让这个语义更可见、更可恢复、更少重复操作，并可以提供 CLI 没有的检测、计划和修复体验；不是发明与 CLI 不兼容的安装/lock 规则。

---

## 2. 数据模型

### 2.1 Lock 文件

全局 lock 和项目 lock 的字段同步规则见 [cli-gui-sync-guide.md](./cli-gui-sync-guide.md)。

项目 lock 需要区分 CLI 共享契约字段和 GUI 增强字段：

- CLI 共享契约字段：`source`、`ref`、`sourceType`、`skillPath`、`computedHash`。
- GUI 增强字段：`sourceUrl`、`remoteHash`、`pluginName`。

GUI 增强字段可以让项目级更新体验更强，例如提前检查 GitHub 远端是否变化、保留 SSH/private repo 原始 URL、展示 plugin 来源。但这些字段不是 CLI project update 的必要条件。CLI 重新执行 project update 后可能只写回共享契约字段，导致 GUI 增强字段丢失；GUI 必须降级处理，而不是把 lock 判坏。

更新链路只依赖标准化后的元数据：

```rust
pub struct NormalizedUpdateMetadata {
    pub source: String,
    pub source_type: String,
    pub source_url: Option<String>,
    pub ref_name: Option<String>,
    pub skill_path: Option<String>,
    pub remote_hash: Option<String>,
}
```

来源：

- Global lock：`skillFolderHash` 标准化为 `remote_hash`。GitHub 来源通常是远端 tree SHA；非 GitHub 来源可能是安装来源/本地内容 hash，只有存在对应远端检测器时才可用于自动比较。
- Project lock：GUI 扩展的 `remoteHash` 标准化为 `remote_hash`。当前主要用于 GitHub 远端检测；`computedHash` 仅表示本地内容 hash。
- `skillPath` 是 update 定位上游目录的关键字段，规范形态是仓库内 `SKILL.md` 文件路径。缺失时不要按 skill name 猜测路径。

### 2.2 Capability vs Status

GUI 把“理论上能不能检查/执行更新”和“一次检查的运行结果”拆开。

| 概念 | 来源 | 典型字段 | 意义 |
| --- | --- | --- | --- |
| Capability | lock 元数据静态派生 | `canRunUpdate`、`canCheckForUpdates`、`updateReason` | 这个 skill 原理上是否支持 update/check |
| Check status | `check_updates` 运行结果 | `update-available`、`up-to-date`、`cannot-check`、`deleted-upstream` | 本次检查得到的结果 |

当前 capability 派生规则：

| 条件 | `can_run_update` | `can_check_for_updates` | `reason` |
| --- | --- | --- | --- |
| `source` 为空或 `source_type == "local"` | false | false | `local-source` |
| 可重装来源缺 `skill_path` | false | false | `missing-skill-path` |
| `source_type != "github"` 且有 `skill_path`、但没有对应远端检测器 | true | false | `unsupported-source-type` |
| 来源支持远端检测但缺 `remote_hash` | true | false | `missing-remote-hash` |
| 元数据完整且来源有远端检测器 | true | true | `None` |

这个派生过程不发网络请求。`listSkills()` 返回的 skill 已经带有 capability，首屏可以直接展示 cannot-check 状态或修复入口。

可重装来源包括 `github`、`gitlab`、`git`、`well-known`、`wellknown` 和 `direct-url`。这些来源的普通 update 都依赖 lock 中的 `skillPath` 精确定位目录；缺失时只能走“修复来源/重新安装”流程。远端检测器是 GUI 可扩展点：新增 source type 检测能力时，应先定义 hash 语义、错误 reason 和缓存 identity，再把 capability 从 `unsupported-source-type` 提升为可检查。

---

## 3. CLI Project Update 基线

CLI project update 是 GUI 必须兼容的基线语义。它不依赖 `sourceUrl` 或 `remoteHash`，也不会先判断远端是否真的变化。

流程：

1. 读取 `<project>/skills-lock.json`。
2. 过滤出非 `node_modules`、非 `local` 的 project skill。
3. 按 `skillPath` 分成两类：
   - 有 `skillPath`：可定点 reinstall/update。
   - 缺 `skillPath`：legacy entry，不能安全自动 update，只打印重新安装提示。
4. 对每个可更新 entry，调用 `buildLocalUpdateSource(entry)` 构造安装源。
5. `buildLocalUpdateSource` 从 `skillPath` 去掉 `SKILL.md` 后缀，把目录拼回 `source`，并追加 `#ref`。例如 `source=owner/repo`、`skillPath=skills/foo/SKILL.md`、`ref=main` 会得到 `owner/repo/skills/foo#main`。
6. CLI 执行 `skills add <installUrl> --skill <name> -y`。没有 `-g`，所以仍是 project scope；`--skill` 确保只安装该 skill。
7. `add` 重新 clone/fetch、discover、安装到项目目录，并重新写 `skills-lock.json`。写回内容包含共享契约字段和新的 `computedHash`。

因此，CLI project update 的能力是“定点刷新”，不是“远端更新检测”。GUI 的 `remoteHash` 检测是在这个基线上增加的可视化判断；缺失 `remoteHash` 时仍可执行 reinstall，只是不能提前告诉用户远端是否变化。

---

## 4. 后端检测流程

入口：`src-tauri/src/commands/update.rs` 中的 `check_updates` / `check_updates_inner`。

流程：

1. 按 scope 读取 global lock 或 project lock。
2. 将 lock entry 标准化为 `NormalizedUpdateMetadata`。
3. 派生 `UpdateCapability`。
4. `can_check_for_updates == false` 的 entry 直接返回 `cannot-check`，不发网络请求。
5. 可检查的 GitHub skill 按 `(source, ref)` 分组。
6. 每组调用 GitHub Trees API 获取远端 tree 信息，再按各自的 `skillPath` 切出 skill 目录 hash。
7. 远端 hash 与本地记录 hash 不同则 `update-available`，相同则 `up-to-date`。
8. 如果 GitHub Trees API 可访问、但某个 `skillPath` 在远端 tree 中不存在，该 skill 返回 `deleted-upstream`，reason 同样是 `deleted-upstream`。
9. 限流、认证失败、网络错误或 API 级不可用时返回 `cannot-check`，并保留机器可读 reason。

设计约束：

- 同仓库多个 skill 应优先共享远端 tree 请求。
- 检查失败不能改 lock。
- `deleted-upstream` 只表示“上游路径缺失”，不能自动删除本地文件，也不能当作普通 update 执行。
- `mergeUpdateInfo` 合并缓存时必须保留后端已有 `updateReason`，避免没有命中本次检查结果时把 cannot-check 原因清空。
- `check_updates` 结果必须携带足够身份信息供前端合并，不能长期只依赖 skill name。推荐 identity 为 `scope + projectPath + name + source/sourceUrl + ref + skillPath`。

---

## 5. 后端执行流程

入口：`update_skill` / `update_skills_batch`。

单个 skill 更新流程：

1. 从 lock 读取来源、ref、skillPath、pluginName 等元数据。
2. 调用 `ensure_can_run_update`；不可执行时返回 `skipped` 结果，而不是进入 clone/install。
3. 构造 update target，使用 `skillPath` 推导 discover 子路径。
4. 获取来源内容：GitHub/GitLab/git 走 clone，well-known/direct-url 走 well-known fetch，本地来源不走普通 update。
5. discover skill，并优先按 lock 中的 `skillPath` 精确匹配。
6. 检测当前已经安装到哪些 agent。
   - 如果文件系统中找不到任何已安装 agent，只回退到当前 scope 下自动读取共享目录的 agent。
   - 这里不能使用 CLI 的静态 universal 列表；GUI 的 automatic target 是按当前 scope 和实际目标路径计算的。
7. 检测每个 agent 现有安装模式，尽量保留 symlink、junction 或 copy。
8. 覆盖安装 canonical 目录，再为各 agent 重建链接或复制。
9. 写回 lock：全局写 `skillFolderHash`，项目写本地 `computedHash`，并在 GUI 能取得可比较的远端版本 hash 时写扩展 `remoteHash`。如果该 skill 是由 CLI project update 刚刚写回，GUI 增强字段可能不存在，后续 list/check 应降级为 `missing-remote-hash` 或缺少展示元数据。

批量更新流程：

- 按 `(source_type, source_url, ref)` 分组。
- 每组只 clone/fetch 一次。
- 组内每个 skill 仍使用自己的 `skillPath` 定位和写回，不能用同名 skill 猜测。

Agent 结果语义：

| Agent 状态 | 说明 |
| --- | --- |
| `success` | 该 agent 安装或更新成功 |
| `failed` | 该 agent 安装失败，错误应暴露给前端 |
| `skipped` | 按规则未安装到该 agent，例如项目级额外 Agent 的根目录不存在 |

Skill 总状态归并：

| Skill 状态 | 触发条件 |
| --- | --- |
| `success` | 所有实际目标 agent 成功；只允许包含“不属于实际目标”的 skipped |
| `partial` | 至少一个成功且至少一个失败，或请求目标中存在未处理的 skipped |
| `failed` | 有目标 agent，但全部失败 |
| `skipped` | 没有可执行目标，或所有 agent 都是 skipped |

`skipped` 是独立状态，不能计入成功覆盖率，也不能写成 failed。实现必须区分两类 skipped：一种是“规则上不属于实际目标”，可以不阻止整体 success；另一种是“用户请求的目标未被处理”，必须保留重试入口，不能清除 update 标记。

---

## 6. 前端缓存与 UI 规则

### 6.1 更新检测缓存

`src/stores/skills-utils.ts` 维护进程内缓存：

```ts
updateInfoCache: Map<string, { results: SkillUpdateInfo[]; checkedAt: number }>
```

规则：

- global scope 的 cache key 是 `global`。
- project scope 的 cache key 是项目绝对路径。
- cache value 中的单条更新结果必须用稳定 identity 合并，至少包含 `name`、`source/sourceUrl`、`gitRef` 和 `skillPath`。只按 skill name 合并会污染修复来源、换 ref 或同名 skill 场景。
- TTL 是短期进程内缓存，应用重启后丢失。
- 手动刷新应绕过 TTL。
- update 完整成功后才能清除该 skill 的 update 标记。
- partial、failed、skipped 不应把缓存强写为 `up-to-date`，否则用户会失去重试入口。
- `deleted-upstream` 不能被缓存清理路径改写为 `up-to-date`。只有用户完成删除、修复来源或重新安装后，列表刷新才应自然移除这个维护状态。

### 6.2 列表状态合并

`mergeUpdateInfo` 将 `check_updates` 结果合并到 `listSkills` 返回的基础列表。

合并规则：

- 命中本次检查结果时，以检查结果的 `hasUpdate`、`status`、`reason` 为准。命中必须基于稳定 identity，而不是只基于 skill name。
- 未命中时，保留已有 `updateStatus` 和 `updateReason`。
- `hasUpdate` 未命中时默认为 false，避免旧缓存误标新列表项。

### 6.3 UI 状态展示

展示 cannot-check 的统一规则：

```ts
skill.updateStatus === 'cannot-check' || skill.canCheckForUpdates === false
```

含义：

- 本次检查失败或不可检查时展示 cannot-check。
- 静态 capability 已知不可检查时，即使尚未发起网络检查，也展示 cannot-check。
- `canCheckForUpdates == null` 表示未知，不应直接展示 cannot-check。

更新按钮和批量按钮应优先读取 capability：

- `canRunUpdate == false`：不要执行普通 update，展示修复或禁用状态。
- `canCheckForUpdates == false`：不参与“检查更新”批量入口。
- reason 应映射为可本地化文案，不要直接把机器字符串作为主要 UI 文案。
- `deleted-upstream` 使用独立维护展示：它不属于普通 update 分组，不触发单项或批量 update；UI 应提供删除本地副本、修复来源或继续保留安装的入口。

### 6.4 安装结果处理

安装流程的完成页，以及修复来源流程的安装结果处理，都需要区分 successful、failed、skipped：

- successful 计入成功覆盖率。
- failed 展示错误。
- skipped 展示“已跳过”的 agent 列表。
- 覆盖率分母可以包含 skipped，用来说明目标集合；分子不能包含 skipped。

---

## 7. CLI 与 GUI 的稳定差异

这些差异是 GUI 的体验优化，但必须以 CLI 语义为基础。

| 能力 | CLI | GUI |
| --- | --- | --- |
| Project scope 更新检测 | 有 `skillPath` 时可定点 reinstall/update，但不一定提前判断远端是否变化 | 用 `remoteHash` 和检测器补充可视化检测 |
| 批量更新 | 按 CLI 命令流程处理 | 同源同 ref 的 skill 可共享 clone/fetch |
| 状态表达 | 终端输出文本 | 结构化 capability、status、reason、agent results |
| 进度反馈 | 终端进度 | `update-progress` event |
| 错误提示 | 文本输出 | reason → i18n 文案、toast、修复入口；`deleted-upstream` 作为维护状态单独展示 |
| Agent 目标 | CLI 使用静态 universal/non-universal 分类 | GUI 使用 scope-aware automatic/additional 分类，同时展示 skipped/partial |

不允许的差异：

- 缺 `skillPath` 时猜测远端目录并普通 update。
- 只按 skill name 合并更新检测缓存。
- 把 `deleted-upstream` 当成普通可更新项执行 update，或自动删除本地文件。
- 把 partial 或 failed 的更新标记清掉。
- 把请求目标中的 skipped agent 当成 success。
- GUI 写回 lock 时丢失 CLI 字段。
- GUI 扩展字段改变 CLI 已定义字段的含义，例如把 `computedHash` 当成远端 hash，或让 CLI 读取后产生不同安装目标。
- GUI 把 `remoteHash` 当作执行 project reinstall 的必要条件。它只能控制“能否提前检测”，不能控制“能否按 CLI 语义刷新”。
- GUI 把 CLI 的静态 universal 列表当成当前 scope 的自动目标。自动目标必须按实际目标目录是否等于 canonical 共享目录计算。
- 放宽 CLI 的 source、well-known 或文件路径安全校验。

---

## 8. 当前限制与维护注意事项

这些是当前设计需要持续关注的点，不是历史修复记录：

- `source` 为空和 local source 当前都会得到 `local-source` reason。如果要细分文案，需要先扩展后端 reason，再同步前端 i18n。
- Project lock 的 `computedHash` 是本地内容 hash；GUI 的 `remoteHash` 是额外的远端/来源版本追踪字段。新增非 GitHub 远端检测时，必须先明确该字段与 source type 的 hash 语义。
- CLI project `add/update` 可能覆盖 GUI 增强字段。GUI 遇到缺 `sourceUrl`、`remoteHash` 或 `pluginName` 时应降级展示和检测能力，而不是阻断 reinstall。
- well-known/direct-url update 路径需要持续保持测试覆盖，避免协议或校验调整时只覆盖 install 不覆盖 update。
- Windows symlink、junction、copy 的 fallback 行为需要明确暴露给 UI，不能让用户误以为所有 agent 都成功更新。
- GitHub API 错误应尽量保留机器可读 reason，前端按 reason 渲染可理解文案。
- 批量更新的 group 失败处理要保持和单个 update 一致，避免同一错误在不同入口下表现不同。

---

## 9. 修改更新链路前的检查清单

- [ ] 先阅读 [cli-gui-sync-guide.md](./cli-gui-sync-guide.md)，确认这次变更是否涉及 CLI 同步点。
- [ ] 检查 `src-tauri/src/core/update_metadata.rs`，确认 capability 派生是否需要调整。
- [ ] 检查 `src-tauri/src/commands/update.rs`，确认检测和执行流程是否都要改。
- [ ] 检查 lock 模型：`skill_lock.rs`、`local_lock.rs`、bindings、前端类型是否一致。
- [ ] 检查 lock 读写是否保留未知字段，避免 GUI 版本落后于 CLI 时丢字段。
- [ ] 检查前端缓存：`src/stores/skills-utils.ts`、`src/stores/skills-data.ts`。
- [ ] 检查 UI：SkillCard、SkillDetailPanel、SkillsSection、安装完成页是否需要同步状态展示。
- [ ] 修改后补 Rust 单元测试和前端 store/component 测试。
- [ ] 验证 `skipped`、`partial`、`cannot-check` 和 reason 不会被误归并成 success 或 up-to-date。
- [ ] 验证更新检测缓存不会只按 skill name 命中新来源、新 ref 或新 `skillPath` 的安装项。
- [ ] 最后更新本文档，让它继续描述新的当前状态。
