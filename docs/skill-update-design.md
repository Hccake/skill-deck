# Skill 更新机制设计文档

本文档梳理 **vercel-labs/skills CLI** 与 **Skill Deck 桌面端** 在"skill 更新"这条链路上的实现，以及二者的差异和已知问题，帮助后续开发者快速 onboard。

> 配套阅读：根目录 [CLAUDE.md](../CLAUDE.md) 中"Skill Update Capability vs Status"段落 / [docs/cli-gui-sync-guide.md](./cli-gui-sync-guide.md)。

---

## 1. 一句话总结

| 维度 | vercel-skills CLI | Skill Deck (Tauri) |
| --- | --- | --- |
| **更新检测依据** | Global lock 中的 GitHub tree SHA（`skillFolderHash`） | 同上 + 项目 lock 扩展字段 `remoteHash` |
| **检测方式** | GitHub Trees API（`/repos/{repo}/git/trees/{ref}?recursive=1`） | 同上，按 `(source_url, ref)` 分组**批量**调用 |
| **更新执行** | `spawnSync('npx skills add ...')` 重新走一次 install 流程 | 同样是"重新安装"语义，但是用 Rust 直接 clone → discover → install，不再 fork 子进程 |
| **可检查/可执行二分** | 不区分（不可检查时直接打印 skip 行） | 显式建模为 `canCheckForUpdates` / `canRunUpdate` 两位 + `updateReason` |
| **缓存** | 无 | 5 分钟 TTL 的进程内缓存（`updateInfoCache`） |
| **进度反馈** | stderr 进度条 | Tauri event：`update-progress`（`cloning` / `installing` / `writing_lock`） |

**核心设计哲学**：更新 = 重新安装。不存在"增量更新"，只有"用最新仓库内容覆盖现有 skill 目录"。

---

## 2. 数据模型

### 2.1 Lock 文件

| 文件 | 路径 | 用途 | 关键字段 |
| --- | --- | --- | --- |
| **Global lock** | `~/.agents/.skill-lock.json`（v3） | 全局已安装 skill 元数据 | `skillFolderHash`（必填 string） |
| **Project lock** | `<project>/skills-lock.json`（v1） | 项目级，进 git | `computedHash`（SHA-256 本地内容） + `remoteHash?`（GUI 扩展） + `skillPath?`（GUI 扩展） |

**Global lock 条目** ([src-tauri/src/core/skill_lock.rs:18-42](../src-tauri/src/core/skill_lock.rs#L18-L42))：

```rust
pub struct SkillLockEntry {
    pub source: String,            // "owner/repo"
    pub source_type: String,       // "github" | "gitlab" | "git" | "local" | "well-known"
    pub source_url: String,        // 原始 URL
    pub ref_name: Option<String>,  // 分支/tag
    pub skill_path: Option<String>,// 仓库内子路径（含 SKILL.md）
    pub skill_folder_hash: String, // GitHub tree SHA
    pub installed_at: String,
    pub updated_at: String,
    pub plugin_name: Option<String>,
}
```

**Project lock 条目** ([src-tauri/src/core/local_lock.rs:32-62](../src-tauri/src/core/local_lock.rs#L32-L62))：

```rust
pub struct LocalSkillLockEntry {
    pub source: String,
    pub ref_name: Option<String>,
    pub source_type: String,
    pub source_url: Option<String>,
    pub computed_hash: String,           // SHA-256 of files (CLI 兼容)
    pub remote_hash: Option<String>,     // ★ GUI 扩展：tree SHA，CLI 会忽略
    pub skill_path: Option<String>,      // ★ GUI 扩展：repo 内子路径
    pub plugin_name: Option<String>,
}
```

> **注意**：CLI 的 `LocalSkillLockEntry` 只有前 4 个字段；GUI 新增 `remote_hash` / `skill_path` / `source_url` 是**向前兼容**地写入（CLI 读时用 `unknown field` 策略忽略）。这是 GUI 能在 project scope 下检测更新、而 CLI 不能的关键。

### 2.2 Capability vs Status（核心抽象）

GUI 把"能否更新"拆成两个正交概念：

| 概念 | 来源 | 类型 | 意义 |
| --- | --- | --- | --- |
| **Capability** | 静态派生自 lock 元数据 | `bool / bool / Option<String>` | 这个 skill 在原理上能否检查 / 能否执行更新 |
| **Status** | 一次 `check_updates` 调用的运行结果 | `update-available` / `up-to-date` / `cannot-check` | 当前这次检查得出的结论 |

二者的关系：`Capability.can_check_for_updates == false` 一定得到 `Status::CannotCheck`；反之 capability 为 true 时仍可能因为网络失败得到 `cannot-check`。

`UpdateCapability` 的派生规则 ([src-tauri/src/core/update_metadata.rs:53-93](../src-tauri/src/core/update_metadata.rs#L53-L93))：

| 条件 | `can_run_update` | `can_check_for_updates` | `reason` |
| --- | --- | --- | --- |
| `source_type == "local"` 或 `source` 为空 | false | false | `"local-source"` |
| `source_type != "github"` | true | false | `"unsupported-source-type"` |
| 缺 `skill_path` | true | false | `"missing-skill-path"` |
| 缺 `remote_hash` | true | false | `"missing-remote-hash"` |
| 满足全部条件 | true | true | `None` |

> 派生只看元数据，不发网络。这意味着 `listSkills()` 返回的每个 `InstalledSkill` 都已经携带 capability 字段，**首屏就能渲染 cannot-check 徽章而无需等待网络**。

---

## 3. 后端：检测 → 执行

### 3.1 检测：`check_updates`

入口 [src-tauri/src/commands/update.rs:90-202](../src-tauri/src/commands/update.rs#L90-L202)。流程：

1. **读 lock**：按 `Scope` 分别读 global / project lock。
2. **能力过滤**：对每个 entry 调用 `derive_update_capability`；不可检查的直接生成一条 `CannotCheck` 结果，**不发网络请求**。
3. **按 `(source, ref_name)` 分组**：同仓库同分支的多个 skill 合并为一组（[update.rs:126-133, 155-162](../src-tauri/src/commands/update.rs#L126-L162)）。
4. **批量拉远端 hash**：每组调用 `fetch_skill_folder_hashes_batch(owner_repo, paths, ref)`，**单次 GitHub Trees API** 拿到该仓库的整棵 tree，再按 skill 子路径切出每个 skill 的 tree SHA。
5. **比对**：远端 hash != 本地 hash → `UpdateAvailable`；相等 → `UpToDate`；远端缺失 → `CannotCheck { reason: "upstream-unavailable" }`。
6. **整组 API 失败**：组内每个 skill 都标记 `cannot-check / upstream-unavailable`（[update.rs:185-197](../src-tauri/src/commands/update.rs#L185-L197)）。

GitHub token 优先级（[src-tauri/src/core/github_api.rs](../src-tauri/src/core/github_api.rs)）：`GITHUB_TOKEN` 环境变量 → `GH_TOKEN` 环境变量 → `gh auth token` CLI → 公网 60 req/h 限额。

**`SkillUpdateInfo` 类型** ([update.rs:69-81](../src-tauri/src/commands/update.rs#L69-L81))：

```rust
pub struct SkillUpdateInfo {
    pub name: String,
    pub source: String,
    pub has_update: bool,
    pub status: SkillUpdateCheckStatus,  // update-available | up-to-date | cannot-check
    pub reason: Option<String>,
    pub git_ref: Option<String>,
}
```

### 3.2 执行：`update_skill` / `update_skills_batch`

入口 [src-tauri/src/commands/update.rs:258-295（单个）/ 577-962（批量）](../src-tauri/src/commands/update.rs#L258-L962)。

单个 skill 流程（9 步）：

1. **读 lock 拿来源** —— 提取 `source / source_type / source_url / skill_path / ref_name / plugin_name`。
2. **入口校验** —— `ensure_can_run_update(metadata)`；`local-source` 直接返回 `Err`。
3. **构造更新目标** —— `build_update_target(UpdateSourceParts {...})`，从 `skill_path` 推导 `discover_subpath`。
4. **获取源**：
   - `local`：直接用本地路径
   - `github` / `gitlab` / `git`：`clone_repo_with_progress`（git2 浅 clone）
   - `well-known` / `direct-url`：`fetch_wellknown_skills` 拉 manifest 并落到临时目录
   - 期间 emit `update-progress { phase: "cloning" }`
5. **discover** —— 在 clone 出来的目录里找 SKILL.md 并解析。
6. **检测已安装的 agents**：
   - 优先 `detect_installed_agents_for_skill`（扫描每个 agent 目录看现状）
   - 为空则 fallback 到 `AgentType::detect_installed()` + universal agents
7. **检测每个 agent 的安装模式**（symlink / junction / copy）—— `detect_install_mode`，**保留用户原本的安装模式**。
8. **覆盖安装** —— `install_skill_to_agents_with_modes`，先写 canonical 目录（`~/.agents/{name}` 或 `<project>/.agents/{name}`），再为每个 agent 重建 symlink 或 copy。emit `update-progress { phase: "installing" }`。
9. **写 lock**：
   - emit `update-progress { phase: "writing_lock" }`
   - 重新拉一次 `fetch_skill_folder_hash` 拿到最新 tree SHA
   - Global → `add_skill_to_lock`；Project → 用本地 `compute_skill_folder_hash` 算 SHA-256 + `remote_hash` 一起写

**批量更新优化**（[update.rs:577-962](../src-tauri/src/commands/update.rs#L577-L962)）：按 `UpdateGroupKey(source_type, source_url, ref)` 分组，**每组只 clone 一次**仓库，然后从同一 clone 中安装组内所有 skill。N 个同源 skill 的 clone 次数从 N 降为 1。

### 3.3 状态归并

每个 agent 的安装结果汇成 `UpdateSkillItemResult.status`（[update.rs:973-997](../src-tauri/src/commands/update.rs#L973-L997)）：

| 后端状态 | 触发条件 |
| --- | --- |
| `Success` | 全部 agent 成功 |
| `Partial` | 至少一个成功 + 至少一个失败 |
| `Failed` | 全部失败 |
| `Skipped` | agent 列表为空 / 全 skipped |

---

## 4. 前端：缓存 + UI 规则

### 4.1 缓存层

[src/stores/skills-utils.ts:31-47](../src/stores/skills-utils.ts#L31-L47)

```ts
export const updateInfoCache = new Map<string, { results: SkillUpdateInfo[]; checkedAt: number }>();
export const UPDATE_CHECK_TTL = 5 * 60 * 1000;

export function clearUpdateCacheForSkill(skillName, scope, projectPath?) {
  const cacheKey = scope === 'project' ? projectPath : 'global';
  const cached = updateInfoCache.get(cacheKey);
  if (cached) {
    cached.results = cached.results.map(r =>
      r.name === skillName
        ? { ...r, hasUpdate: false, status: 'up-to-date', reason: null }
        : r,
    );
  }
}
```

**Cache key**：global 用字符串 `'global'`，project 用项目绝对路径。  
**生命周期**：进程内 Map，无持久化；Tauri 重启即丢。  
**写入时机**：`syncUpdates` 拿到结果后写整组（[skills-data.ts:264-265](../src/stores/skills-data.ts#L264-L265)）。  
**读取时机**：`fetchSkills` / `syncSkills` 把 listSkills 的原始结果通过 `mergeUpdateInfo(skills, cache.results)` 套用缓存里的 `hasUpdate` / `status` / `reason`，避免列表刷新一次就回到 unknown 状态（[skills-data.ts:80-99](../src/stores/skills-data.ts#L80-L99)）。  
**过期策略**：TTL 5 分钟，硬过期；用户手动刷新走 `forceCheckUpdates` 直接绕开 TTL。  
**清缓存的关键规则**（[skills-data.ts:362-364](../src/stores/skills-data.ts#L362-L364)）：

```ts
if (target?.canCheckForUpdates !== false) {
  clearUpdateCacheForSkill(skillName, scope, projectPath);
}
```

> 仅当 capability 不是 `false` 时（即 `true` 或 `null`）才清缓存。能力本身为 `false` 的 skill（local 等）若清掉就会被 `clearUpdateCacheForSkill` 强写为 `up-to-date`，掩盖"它本来就不能检查"的真实状态。

### 4.2 UI 规则

UI 中"是否展示『无法检查更新』徽章"用一条统一规则，写在两个组件里：

[src/components/skills/SkillCard.tsx:129](../src/components/skills/SkillCard.tsx#L129) 和 [src/components/skills/SkillDetailPanel.tsx:123](../src/components/skills/SkillDetailPanel.tsx#L123)：

```ts
const showCannotCheckStatus =
  skill.updateStatus === 'cannot-check' || skill.canCheckForUpdates === false;
```

含义：
- **本次检查**结果是 `cannot-check`（capability 可能是 true，但网络失败）→ 显示
- **能力**是 `false`（永远没法检查）→ 显示
- 两者都不是（包括 `null`，未初始化）→ 不显示

**批量"检查更新"按钮**只在 scope 内有 `canCheckForUpdates === true` 的 skill 时才出现（[SkillsSection.tsx:90](../src/components/skills/SkillsSection.tsx#L90)）。

### 4.3 状态机：`updateStatus`（前端临时态）

`SkillCard` / `SkillDetailPanel` 用一个 *前端瞬时* 的 `updateStatus`（`'queued' | 'updating' | 'done' | 'failed'`）管理点击更新到结果出来的过渡（[SkillCard.tsx:59](../src/components/skills/SkillCard.tsx#L59)），与 `SkillUpdateCheckStatus`（持久化在 cache）是两个不同的字段，靠类型签名区分。Update 进度条直接监听 Tauri 的 `update-progress` event 并改 DOM ref，不触发 React re-render（[SkillCard.tsx:97-121](../src/components/skills/SkillCard.tsx#L97-L121)）。

---

## 5. 调用链总览

### 5.1 检测

```
Page mount
  └─ SkillsPage useEffect
       ├─ fetchSkills()                      [skills-data.ts]
       │   └─ listSkills(scope) → bindings → list_skills (Rust)
       │       └─ 每个 InstalledSkill 携带 capability 字段
       └─ syncUpdates()                      [skills-data.ts]
           ├─ TTL 检查 → 命中则跳过
           └─ checkUpdates(scope, projectPath) → bindings
               └─ check_updates_inner        [update.rs:99]
                   ├─ read_scoped_lock / read_local_lock
                   ├─ derive_update_capability  → 不可检查直接 cannot-check
                   ├─ 按 (source, ref) 分组
                   └─ fetch_skill_folder_hashes_batch  [github_api.rs]
                       └─ GET /repos/{repo}/git/trees/{ref}?recursive=1
```

### 5.2 执行

```
User clicks "Update"
  └─ skillsStore.updateSkill(name, scope)    [skills-data.ts:319]
      └─ apiUpdateSkill → bindings → update_skill (Rust)
          └─ update_skill_single             [update.rs:297]
              ├─ read lock entry
              ├─ ensure_can_run_update          ← 失败直接 reject
              ├─ build_update_target
              ├─ clone_repo_with_progress       [emit "cloning"]
              ├─ discover_skills
              ├─ detect_installed_agents_for_skill (+ fallback)
              ├─ detect_install_mode (per agent)
              ├─ install_skill_to_agents_with_modes  [emit "installing"]
              ├─ fetch_skill_folder_hash        ← 拿新 hash
              └─ add_skill_to_lock / add_skill_to_local_lock  [emit "writing_lock"]

完成后 (前端) ：
  ├─ toast(success/partial/failed)
  ├─ if (canCheckForUpdates !== false) clearUpdateCacheForSkill(name)
  ├─ clearLocalUpdateFlags(state, scope, {name})  ← 把列表里的 hasUpdate 标记清掉
  └─ syncSkills() (fire-and-forget) ← 后台刷新一次列表与详情
```

---

## 6. 测试覆盖

### Rust ([src-tauri/src/commands/update.rs:1020-1260](../src-tauri/src/commands/update.rs#L1020-L1260))

12 个单元测试，覆盖：
- `normalize_global_lock_entry` / `normalize_local_lock_entry` 字段映射
- `derive_update_capability` 各分支（local / unsupported / missing-skill-path / 完整）
- `build_update_target` 解析 `skill_path` 切出 `discover_subpath`
- `check_updates_inner` 在缺元数据时打 `cannot-check`
- `ensure_can_run_update` 拒绝 local / 接受 github
- `derive_skill_status` / `summarize_results` 多 agent 场景的状态归并

### TypeScript

- [src/stores/__tests__/skills.test.ts](../src/stores/__tests__/skills.test.ts) — `updateSkill` 状态转移、partial/warning、批量、并发隔离
- [src/components/skills/\_\_tests\_\_/SkillCard.test.tsx](../src/components/skills/__tests__/SkillCard.test.tsx) / [SkillDetailPanel.test.tsx](../src/components/skills/__tests__/SkillDetailPanel.test.tsx) — `cannot-check` 徽章、`canCheckForUpdates` 各值
- [src/components/skills/\_\_tests\_\_/SkillsSection.test.tsx](../src/components/skills/__tests__/SkillsSection.test.tsx) — 批量按钮的可见性条件

---

## 7. 与 vercel-skills CLI 的差异速查

`update / upgrade / check` 是 CLI 同一个命令的三个别名（[vercel-skills/src/cli.ts:900-903](../vercel-skills/src/cli.ts#L900-L903)），核心实现在 [`updateGlobalSkills`](../vercel-skills/src/cli.ts#L589) / [`updateProjectSkills`](../vercel-skills/src/cli.ts#L712)。

| 能力 | CLI | GUI |
| --- | --- | --- |
| 不可检查的 skill 标记 | `getSkipReason()` 返回字符串，`printSkippedSkills` 列出来（[cli.ts:494-561](../vercel-skills/src/cli.ts#L494-L561)） | 类型化 `UpdateCapability { can_check, can_run, reason }`，UI 直接读 |
| Project lock 检测远端更新 | ❌（CLI 项目级 update 直接重新跑 `add`，**不查远端 hash**） | ✅（用 GUI 扩展的 `remoteHash` + `skillPath` 字段） |
| 同仓库多 skill 的 API 调用 | N skills × N 次（[cli.ts:630-649](../vercel-skills/src/cli.ts#L630-L649)，循环里逐个 `fetchSkillFolderHash`） | 1 次 / 组（`fetch_skill_folder_hashes_batch`） |
| 进度反馈 | stderr `\r` 行刷新（"Checking 1/N..."） | Tauri event：`clone-progress` + `update-progress { phase }` |
| 更新执行的进程模型 | `spawnSync(node, [cli.mjs, 'add', url, '-g', '-y'])` —— 重新走 install 子进程 | Rust 内联调用 install 模块，无子进程 |
| 多 agent 安装模式保留 | 由 install 流程决定（每次都重新选 agent） | 显式 `detect_install_mode` 保留每个 agent 的 symlink/copy 选择 |
| 缓存 | 无 | 5 分钟 TTL，按 scope 分桶 |

> CLI 项目级"更新"实际只是 `npx skills add <source> -y` 重跑一次，因此 CLI 用户在 project scope 下**永远不会被告知"有新版本可用"**——只能盲目 reinstall。GUI 通过扩展 lock 字段填补了这个能力空缺。

---

## 8. 已知限制与改进建议

> 下列条目按"建议优先级"排序，前面的更值得优先处理。
>
> **修复状态总览（2026-04-27）** ——详见 [docs/plans/2026-04-27-skill-update-fixes-impl.md](./plans/2026-04-27-skill-update-fixes-impl.md)：
>
> | # | 描述 | 状态 |
> | --- | --- | --- |
> | 8.1 | partial 时 cache 强写 up-to-date | ✅ 已修复（仅 `success` 才清缓存 / 写 lock） |
> | 8.2 | hash 抓失败 → lock 写空 hash | ✅ 已修复（本地 git 优先 + 旧 hash 兜底） |
> | 8.3 | local-source reason 与触发条件不一致 | ⏳ 未修（cosmetic） |
> | 8.4 | GitHub token 缺失时静默限流 | ✅ 已修复（识别 `rate-limited` / `auth` / `network-error` 并 UI 区分文案） |
> | 8.5 | global vs project 的 hash 字段口径不一致 | ⏳ 维持现状（CLI 兼容设计，已在文档中说明） |
> | 8.6 | well-known update 路径缺测试 | ⏳ 未修 |
> | 8.7 | Windows symlink 失败回退 | ⏳ 未修 |
> | 8.8 | `AppError::to_string()` 信息损失 | 🟡 部分修（新增 `GitHubApiError { reason }`,但其他 variant 没动） |
> | 8.9 | batch group 失败样板代码重复 | ⏳ 未修 |
> | — | update 时 agent fallback 包含 `detect_installed()`,会装到从未链接过的 agent | ✅ 已修复（fallback 仅保留 universal agents） |

### 8.1 ⚠️ `clearUpdateCacheForSkill` 的 partial 失败处理过于激进

[skills-utils.ts:41-43](../src/stores/skills-utils.ts#L41-L43) 把缓存里该 skill 的 status 直接强写为 `up-to-date`：

```ts
? { ...r, hasUpdate: false, status: 'up-to-date', reason: null }
```

但调用方在 [skills-data.ts:362](../src/stores/skills-data.ts#L362) 是**只要 `canCheckForUpdates !== false` 就调**——即使 update 本身是 `Partial`（部分 agent 失败），也会把 cache 写成 up-to-date。这在 UI 上可能误导：用户看到"部分 agent 安装失败"的 toast，列表里的更新徽章却消失了。

**建议**：在 `Partial / Failed` 路径下不要清缓存；或新增一个 `status: 'partial'` 走专门的徽章。

### 8.2 Update 之后立刻 `fetch_skill_folder_hash` 失败 → lock 里写空 hash

[update.rs:499-510](../src-tauri/src/commands/update.rs#L499-L510)：

```rust
let new_hash = if entry_source_type == "github" {
    fetch_skill_folder_hash(...)
        .await
        .unwrap_or(None)
        .unwrap_or_default()  // ← 失败 → ""
} else { String::new() };
```

如果安装成功但抓 hash 失败（限流/网断），lock 会写入空字符串。这之后 `derive_update_capability` 会判定为 `missing-remote-hash`，下次刷新该 skill 直接 cannot-check。等于一次"伪降级"。

**建议**：失败时**保留旧 hash**而不是写空；或者把 hash 抓取放到 install 之前（已经 clone 过 tree，本地就能算）。

### 8.3 `local` 类型的 reason 字符串与触发条件不一致

[update_metadata.rs:54-62](../src-tauri/src/core/update_metadata.rs#L54-L62) 中：

```rust
let can_run_update = !metadata.source.is_empty() && metadata.source_type != "local";
if !can_run_update {
    return UpdateCapability { reason: Some("local-source".to_string()), ... };
}
```

`source` 为空字符串时也会走这个分支，但 reason 仍是 `"local-source"`，让前端 i18n 文案不准确。

**建议**：分两个 reason：`empty-source` / `local-source`。

### 8.4 GitHub token 缺失时的限流体验

[github_api.rs](../src-tauri/src/core/github_api.rs) 当所有 token 来源都拿不到时静默走公网 60 req/h。一个项目装 70 个 skill 的用户首次 sync 就直接打满。前端没有任何提示。

**建议**：在 settings 里显式提示当前是否有 token；或在 `cannot-check` 的 reason 里加 `"rate-limited"` 一种新的细分。

### 8.5 Hash 计算口径在 global 与 project 不同

- Global：lock 里的 `skill_folder_hash` 是 GitHub tree SHA（远端的）
- Project：lock 里有两个：`computed_hash`（本地 SHA-256） + `remote_hash`（远端 tree SHA）

后者是有意为之（CLI 兼容 + 升级路径），但读代码时极容易误会。`compute_skill_folder_hash` 还会跳过 `.git` / `node_modules` 但**不会**跳过 git submodule 的内部目录（[local_lock.rs](../src-tauri/src/core/local_lock.rs)），含 submodule 的 skill 可能虚假触发"本地内容变了"。

**建议**：在 `compute_skill_folder_hash` 上注释清楚约束；或扩成跳过所有 `.git*` 前缀。

### 8.6 `well-known` / `direct-url` 类型的更新链路缺测试

[update.rs:412-414](../src-tauri/src/commands/update.rs#L412-L414) 调 `fetch_wellknown_skills`，但当前测试套件里没有针对 well-known 的 update 集成测试。如果未来 well-known endpoint 的协议变化或返回结构调整，这条路径很可能默默坏掉。

**建议**：补一个 mock-server 的集成测试。

### 8.7 Windows 上 symlink 的失败回退

[installer.rs](../src-tauri/src/core/installer.rs) 在 Windows 上创建 directory symlink 需要开发者模式或 admin。当前更新流程会保留原始模式，但若原本是 symlink、用户后来关掉了开发者模式，更新时会失败。

**建议**：symlink 创建失败时降级到 copy 并 emit 一条 warning；UI 里在该 skill 上标注"已切换为 copy 模式"。

### 8.8 错误信息在跨边界传递时被压扁

[update.rs:277-286](../src-tauri/src/commands/update.rs#L277-L286) 把 `AppError` 一律 `to_string()` 进 `error: Some(...)`。前端的 toast 文案是 `t('skills.updateError', { name, error })`，对于多层嵌套的网络错误信息可读性较差。

**建议**：为 `AppError` 增加 `kind()`（机器可读）+ `display()`（人类友好），前端按 `kind` 分别渲染。

### 8.9 `update_skills_batch` 内 group 失败的写法散落

[update.rs:714-786](../src-tauri/src/commands/update.rs#L714-L786) 在每个 source-type 分支里都重复写"group 全部失败"的样板代码（push N 条 Failed result）。可以抽个 helper。

---

## 9. Onboarding Checklist（给新人）

修改 update 链路前请走一遍：

- [ ] 读完 [`update_metadata.rs`](../src-tauri/src/core/update_metadata.rs)（最短，先理解 capability 派生）
- [ ] 读 [`commands/update.rs`](../src-tauri/src/commands/update.rs) 的 `check_updates_inner` + `update_skill_single`
- [ ] 读前端 [`stores/skills-utils.ts`](../src/stores/skills-utils.ts)（缓存层）+ [`stores/skills-data.ts`](../src/stores/skills-data.ts) 中 `updateSkill` / `syncUpdates`
- [ ] 看 [`SkillCard.tsx`](../src/components/skills/SkillCard.tsx) 第 129 行那条 UI 规则
- [ ] 跑 `cargo test -p skill-deck commands::update::tests` + `pnpm test -- skills.test`
- [ ] 改完 capability / status 任一定义后 → 同步更新本文档第 2 节
