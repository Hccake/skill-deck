# Changelog

All notable changes to Skill Deck will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.0] - 2026-02-26

### Changed

- **重构 ConfirmStep 确认页面** — 移除冗余的 Scope 信息卡片和重复的路径前缀、agent badges、mode label，新增集中覆盖警告条与行内 Tooltip 标记，新增安装信息区展示安装方式和安装目录列表
- **优化安装进度展示** — 安装过程新增细粒度进度状态反馈，提升安装体验
- 搜索安装 skill 时窗口自适应高度
- 移除安装步骤中内容区域多余的 padding top
- 优化 ConfirmStep 布局层级和交互体验
- ESLint 校验范围限定为 `src` 目录

## [0.5.0] - 2026-02-24

### Fixed

- 修复设置页「检查更新」按钮在首次检查后永久消失的问题，改为始终显示刷新按钮允许手动重新检查
- 修复检查更新失败时无任何错误提示的问题，新增错误状态展示和重试按钮
- 更新检查 UI 改为由 store 状态驱动，移除对 `localStorage` 的直接依赖

## [0.4.0] - 2026-02-24

### Added

- **Cortex & Universal Agent 支持** — 新增 Cortex（Snowflake）和 Universal（`.agents/skills`）两种 Agent 类型
- **项目级 Local Lock** — 新增 `skills-lock.json`，使用 SHA-256 哈希追踪项目级 skill 状态，兼容 skills CLI v1.4.1
- **安全审计 API** — 调用 `add-skill.vercel.sh/audit` 接口获取 skill 风险等级，3 秒超时优雅降级
- **RiskBadge 组件** — 在 SkillCard 和安装确认步骤展示 skill 安全风险等级（safe / low / medium / high / critical / unknown）
- **Source 别名** — 支持源地址别名解析（如 `coinbase/agentWallet` → `coinbase/agentic-wallet-skills`）

### Changed

- 安装流程不再排除 `README.md`（仅排除 `metadata.json`）
- 项目级 skill 的安装/卸载/更新/列表全部切换到 Local Lock
- 更新检测支持从 Local Lock 读取 `remoteHash` 进行比对
- Replit Agent 检测标识从 `.agents` 改为 `.replit`
- Cursor 项目级 skill 目录从 `.cursor/skills` 改为 `.agents/skills`，成为 Universal Agent
- OpenClaw 全局目录三路径均不存在时默认回退到 `.openclaw/skills`（对齐 CLI v1.4.1）

### Fixed

- 卸载 skill 时增加安全检查，避免误删被多个 Agent 共享的 canonical 目录
- 移除 Antigravity 的 `cwd/.agent` 检测，减少误判（对齐 CLI v1.4.1）
- 移除 GitHub Copilot 的 `cwd/.github` 检测，`.github` 是仓库标记而非 Copilot 安装标记（对齐 CLI v1.4.1）
- Git 克隆时设置 `GIT_TERMINAL_PROMPT=0`，防止私有仓库弹出凭据提示导致进程挂起

## [0.3.2] - 2026-02-24

### Changed

- 版本号统一由 `package.json` 管理，构建时自动同步到 `Cargo.toml` 和 `tauri.conf.json`

## [0.3.1] - 2026-02-24

### Fixed

- 修复 macOS 和 Ubuntu 编译失败问题

## [0.3.0] - 2026-02-24

### Added

- **发现页** — 通过 skills.sh 搜索在线 skill 并一键安装
- **更新检测** — 支持检测已安装 skill 的新版本并一键更新
- 安装 skill 弹窗优化为独立窗口

### Changed

- 使用 tauri-specta 替代手动类型桥接，Rust 类型自动生成 TypeScript 绑定
- React 代码全面优化，消除潜在隐患

## [0.2.0] - 2026-02-11

### Added

- **安装错误页** — 安装 skill 报错时展示详细错误信息和修复建议

### Fixed

- 修复 Windows 环境执行 git 命令时弹出控制台窗口的问题
- 修复命令行解析安装时 source 传递错误的问题
- 修复 TypeScript 类型错误

## [0.1.0] - 2026-02-11

### Added

- **首个发布版本**
- Skill 管理核心功能：安装、卸载、更新
- 支持 38+ AI Agent（Claude Code、Cursor、Windsurf、Copilot 等）
- 多来源支持：GitHub shorthand、URL、本地路径、安装命令解析
- 安装模式：符号链接（推荐）和复制
- Global / Project 双层 scope 管理
- Agent 过滤和 Display Name 展示
- 国际化支持（English / 简体中文）
- 深色/浅色主题切换
- GitHub Actions CI/CD 构建流水线（Windows / macOS / Ubuntu）

[Unreleased]: https://github.com/hccake/skill-deck/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/hccake/skill-deck/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/hccake/skill-deck/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/hccake/skill-deck/compare/v0.3.2...v0.4.0
[0.3.2]: https://github.com/hccake/skill-deck/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/hccake/skill-deck/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/hccake/skill-deck/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/hccake/skill-deck/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/hccake/skill-deck/releases/tag/v0.1.0
