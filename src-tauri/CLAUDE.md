# src-tauri/CLAUDE.md — Rust Backend Rules

## Adding a New Tauri Command

1. 在 `commands/` 下创建或修改对应的 command handler 文件
2. 使用 `#[tauri::command]` 和 `#[specta::specta]` 两个宏标注函数
3. 返回类型 MUST 为 `Result<T, AppError>`（见 `error.rs`）
4. 在 `commands/mod.rs` 中 `pub mod` 注册模块
5. 在 `lib.rs` 的 `collect_commands![]` 宏中注册命令
6. 运行 `cargo check` 确认编译通过
7. 前端运行 `pnpm tauri dev`（debug 模式会自动重新生成 `bindings.ts`）
8. 前端运行 `pnpm patch-bindings` 修复 TS 严格模式问题
9. 在 `src/hooks/useTauriApi.ts` 中添加对应的 wrapper 函数

## Error Handling

- 所有命令 MUST 返回 `Result<T, AppError>`
- `AppError` 是统一 enum（`error.rs`），17 个 variant，使用 `thiserror` 派生
- 新增 error 场景 → 在 `AppError` 中添加 variant，NEVER 直接用 `String` 作为 error
- 实现了 `From<std::io::Error>`, `From<serde_yaml::Error>`, `From<serde_json::Error>`, `From<reqwest::Error>` 等自动转换

## Core Module Responsibilities

| 模块 | 职责 |
|------|------|
| `core/source_parser.rs` | 解析 9 种 skill source 格式 → `SkillSource` enum |
| `core/installer.rs` | 安装逻辑：clone/copy → 写入 agent config dir |
| `core/uninstaller.rs` | 卸载逻辑：支持 partial removal (按 agent 移除) |
| `core/agents.rs` | 检测系统中已安装的 AI agents（38+ 种） |
| `core/discovery.rs` | 从远程 source 获取可用 skills 列表 |
| `core/git.rs` | Git clone 操作封装 |
| `core/github_api.rs` | GitHub API 调用（获取 repo 内容） |
| `core/skill_lock.rs` | 全局 lock 文件管理（`~/.agents/.skill-lock.json`） |
| `core/local_lock.rs` | 项目级 lock 文件管理 |
| `core/paths.rs` | 各 agent 的配置目录路径解析 |
| `core/skill.rs` | Skill 元数据解析（SKILL.md → SkillMetadata） |
| `core/plugin_manifest.rs` | Plugin 分组支持 |
| `core/audit.rs` | 安全审计数据获取 |

## Commands Directory

| 文件 | 对应前端 API |
|------|-------------|
| `commands/agents.rs` | `listAgents()` |
| `commands/skills.rs` | `listSkills()` |
| `commands/config.rs` | `getConfig()`, `saveConfig()`, project CRUD, `getLastSelectedAgents()` |
| `commands/install.rs` | `fetchAvailable()`, `installSkills()` |
| `commands/overwrites.rs` | `checkOverwrites()` |
| `commands/remove.rs` | `removeSkill()` |
| `commands/remove_details.rs` | `getSkillAgentDetails()` |
| `commands/update.rs` | `checkUpdates()`, `updateSkill()` |
| `commands/wizard.rs` | `openInstallWizard()` |
| `commands/audit.rs` | `checkSkillAudit()` |
