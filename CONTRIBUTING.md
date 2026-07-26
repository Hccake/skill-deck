# 贡献指南

感谢参与 Skill Deck 开发。本项目是 Tauri 桌面应用，Frontend 使用 React/TypeScript，Backend 使用 Rust。贡献需要同时维护用户行为、generated IPC contract、window ACL 和跨平台语义，而不是只让单个平台的局部实现通过。

开始修改前先阅读[文档地图](./docs/README.md)，再根据任务进入对应的产品、架构或领域文档。
测试设计、fixture、跨平台边界与证据声明遵循[测试与验证规范](./docs/testing.md)。

## 开发环境

构建环境以仓库中的机器可读配置为准：

- Node.js 使用 CI 声明的 active LTS policy，pnpm 版本由 `package.json` 的 `packageManager` 固定，Frontend dependency 以 lockfile 为准；
- Rust 最低版本以 `src-tauri/Cargo.toml` 的 `rust-version` 为准；
- Linux、macOS 和 Windows 的系统依赖遵循 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)；
- 如需手动排查实际 WSL 行为，Windows 开发机需要 WSL2 和一个满足支持基线的发行版；这不是仓库自动验证的前置条件。

公开开发命令使用标准工具形式：

```bash
pnpm install --frozen-lockfile
pnpm dev
pnpm tauri dev
```

参与开发的 AI Agent 还需要遵循根目录 `AGENTS.md` 中的协作规则。开发环境应满足仓库声明的 runtime 与 package manager 要求。

## 修改流程

1. 先确认问题属于哪一份长期文档，并阅读对应实现与测试。
2. 在修改共享 symbol 前执行 GitNexus upstream impact analysis，确认直接调用方和受影响流程。
3. 功能和缺陷修复遵循 test-first，先让目标测试因缺少行为而失败，再实现最小改动。
4. 同一变更同步维护 Rust contract、generated bindings、Frontend、ACL、i18n 和长期文档。
5. 在 dirty worktree 中只修改和验证本任务文件，不回退、覆盖或顺带整理其他人的改动。
6. 提交前执行 GitNexus `detect_changes`，再按风险运行相应验证。

设计 spec、实施计划和阶段性 review 可以保存在 gitignored 的 `docs/plans/**` 或 `docs/superpowers/**`，但它们不是长期权威，也不得作为普通提交内容。最终行为必须进入源码、测试和对应长期文档。

## Frontend

Frontend 负责呈现与用户交互，不在组件中重新实现 Backend 业务规则。

### 组件与交互

- 优先使用仓库已有的 shadcn/ui 与 Radix primitive，再考虑增加新的基础组件。
- 按控件语义选择组件。选项集合使用 `Select`，二元状态使用 Checkbox/Switch，确认破坏性操作使用 Dialog/AlertDialog，短暂反馈使用 toast。
- 图标优先使用 `lucide-react`，icon-only button 提供可访问名称或 tooltip。
- 页面、Settings section 和高成本功能按路由或实际使用时机加载，不在应用启动时无条件拉取全部数据。
- Loading、Empty、Error、Partial、Stale 和 Recovery 状态都需要稳定布局和明确下一步，不把请求失败显示成“没有数据”。
- Windows/WSL、Built-in/Custom 等技术差异只在用户决策需要时展示，不把实现分层直接暴露成额外操作步骤。

### 状态与异步请求

- Zustand consumer 使用字段级 selector，避免订阅整个 store。
- Environment、Context 或 request key 变化后，异步响应必须使用 generation/request identity 防止旧结果覆盖新状态。
- Promise rejection 需要落入用户可见错误或明确的业务降级，不使用空 `catch` 吞掉连接、存在性或写入失败。
- 跨组件共享的 runtime event、focus refresh、cache 和 in-flight dedup 由 store/hook owner 管理，不让多个页面各自监听并重复请求。
- 组件保存用户意图，Backend preview 和 execute result 才决定 filesystem 与 mutation 事实。

### i18n 与文案

- 用户可见文案进入 `src/i18n/locales/en.json` 和 `zh-CN.json`，不在组件中硬编码两套文本。
- 中文使用自然陈述，避免逐词翻译。Agent、Skill、Environment、Context、Backend、WSL 等产品或工程术语可以保留原文。
- Backend 返回稳定 error/warning code 与 parameters；Frontend 负责本地化，不直接展示 Backend 内部英文摘要。

### Frontend 测试

测试分层、用户可观察行为、mock 证据边界、deterministic fixture 和 React async 约束统一遵循[测试与验证规范](./docs/testing.md)。

## Backend

Backend 的依赖方向为：

```text
commands -> application -> core/environment/storage
```

`src-tauri/src/lib.rs` 是 composition root，负责构造长生命周期 state、注册 command、连接 event 和执行启动 maintenance。

### 分层规则

- `commands` 是 Tauri transport adapter，只负责 DTO、`State`、调用来源、mutation admission 和 error conversion。
- `application` 负责 use case、runtime facts、planner、payload session、coordinator 和跨模块编排。
- `core` 保存稳定领域类型与纯规则，不依赖 Tauri window 或具体 process transport。
- `environment` 负责 Context、Environment、Native/WSL backend、path mapping 和 filesystem capability。
- `storage` 负责 atomic document、lossless lock commit、payload/recovery persistence。
- Host/WSL 分支停留在 Environment adapter，不复制 install/update/remove 等业务流程。

### Error 与安全边界

- 公共失败使用 `AppError` 或稳定 operation code，不依赖字符串匹配控制 Frontend 行为。
- Frontend path 不是授权凭据。读取、打开、删除和恢复 command 接收 typed identity，由 Backend 重新解析并验证目标。
- 所有 Skill mutation 复用 preview/execute、planner/coordinator 和 recovery，不在 command 中直接写目录或 lock。
- Payload 表示完整 Skill 目录；任何 materialization 不能只复制 `SKILL.md`。
- WSL operation 使用 bundled script asset、typed request/response、positional arguments 或结构化 stdin，不把用户输入拼进 shell source。
- 新增 WSL operation 时同时实现 timeout、cancellation、bounded output、protocol/version check 和 error mapping。

### Command、bindings 与 ACL

Rust command surface 是唯一 IPC 权威。修改 command 或公开 DTO 时同时完成：

1. 更新 Rust command、type 和 canonical registration；
2. 更新 `src-tauri/app_commands.rs` 并保持排序、去重；
3. 将 command 加入正确的 Tauri permission 与 window capability；
4. 重新生成 `src/bindings.ts`；
5. 更新 `useTauriApi` wrapper 与调用方；
6. 增加 command surface、ACL 和行为测试。

生成 bindings：

```bash
pnpm bindings:generate
pnpm bindings:check
```

Main 与 Install Wizard 只共享两边实际使用的安装与运行时 command，Settings、Recovery、Updater 等能力仍按窗口维持最小权限。不要为了通过调用而授权没有真实调用点的 command，也不要绕过 application command manifest 直接依赖 UI 隐藏。

## 文档

长期文档按事实发生变化的原因划分，职责见[文档地图](./docs/README.md)。修改行为时只更新真正拥有该事实的文档，其他位置使用链接。

- 产品能力或交互规则变化时更新 `docs/product.md`。
- 顶层分层、IPC、window、security、runtime ownership、WSL transport 或 platform backend 变化时更新 `docs/architecture.md`。
- Source、安装、读取、更新、修复、Manage Agents、复制或移除变化时更新 `docs/skill-lifecycle.md`。
- Agent Registry、definition、Detection、关联 Agent、选择分组或默认目标变化时更新 `docs/agents.md`。
- Environment、Context、ProjectBinding、路径解析或 storage access 变化时更新 `docs/environments-and-contexts.md`。
- Payload、preview/execute、atomic write、cancellation 或 recovery 变化时更新 `docs/execution-and-recovery.md`。
- vendored CLI、共享 lock 或互操作行为变化时更新 `docs/skills-cli-compatibility.md`。
- 测试分层、fixture、跨平台能力或验证证据变化时更新 `docs/testing.md`。
- 开发、验证、CI 或 Release 流程变化时更新本文。

正文使用符合中文语境的陈述体。复杂结构和时序可以使用 Mermaid；简单规则不为了形式增加图示。不要维护能够从源码生成的完整 command、type、component、Agent 或 artifact 清单。

## 本地验证

### Frontend 与脚本

```bash
pnpm test:scripts
pnpm bindings:check
pnpm lint
pnpm test
pnpm build
```

### Rust

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --locked --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

根据改动范围先运行最小目标测试，再在完成前运行上述完整集合。只有最新命令输出为 exit 0 才能声称验证通过；已有 baseline failure 需要记录命令、失败项和与本次改动的关系。
选择目标测试、platform acceptance 和 desktop E2E 的规则见[测试与验证规范](./docs/testing.md)。

## CI

Pull request CI 包含以下门禁：

- Rust MSRV check；
- script tests、bindings、ESLint、Vitest 和 Frontend build；
- Windows、macOS、Linux 上的 Rust fmt/check/clippy/test；
- bundled WSL `/bin/sh` assets 的 ShellCheck。

Native integration tests 使用真实临时 filesystem 验证完整 workflow。WSL 验证由跨平台 Rust parser/protocol tests、ShellCheck、Linux 上真实执行 bundled session script，以及 Windows、macOS、Linux 三平台 Rust matrix 组成；仓库不维护真实发行版 acceptance workflow。

## Release 与应用更新

Release workflow 由 `v*` tag 或人工输入触发，并校验 tag、package version、CHANGELOG entry 和 commit SHA 一致。构建矩阵覆盖 macOS arm64/x64、Linux x64 和 Windows x64，为每个平台生成签名 updater artifact，最后由单一 aggregate job 生成 `latest.json` 并保持 GitHub Release 为 Draft。

发布 Draft 前需要满足：

- Frontend contract/build 和 Backend tests 通过；
- 所有 updater package 与 signature 非空且 metadata 与 tag/SHA 一致；
- `latest.json` 覆盖完整平台矩阵，URL 指向当前 repository/tag；
- Release 不包含内部 metadata fragment 或意外的旧 managed asset。

维护者确认 Draft Release 的版本、平台制品、签名和 `latest.json` 无误后，通过 GitHub UI 手动公开。发布不以真实 WSL 发行版验收为门禁；WSL 支持边界由前述自动化协议与 shell 验证固定。

应用 updater 依赖 Tauri signature verification。不得发布缺少 signature 的 package，也不得手工拼接与当前 tag 不一致的 `latest.json`。

## 提交范围

- 不提交 build output、临时 evidence、vendored CLI 的本地辅助文件、`docs/plans/**` 或 `docs/superpowers/**`。
- 不提交 `src-tauri/permissions/autogenerated/**`；它由 Tauri build 根据 `app_commands.rs` 生成，手写的 window command set 和 capability 需要提交。
- 不在同一提交中混入无关格式化、generated churn 或用户尚未确认的重构。
- generated binding 只有在 Rust contract 确实变化时更新。
- 修改 Release/CI 时保留现有 Rust、WSL 和 updater gate，除非变更本身明确重新设计这些门禁。
