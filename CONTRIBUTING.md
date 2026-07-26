# 贡献指南

感谢参与 Skill Deck 开发。本项目是基于 Tauri 的桌面应用，前端使用 React/TypeScript，后端使用 Rust。贡献需要同时维护用户行为、生成的 IPC 契约、窗口 ACL 和跨平台语义，不能只让单个平台的局部实现通过。

开始修改前先阅读[文档地图](./docs/README.md)，再根据任务进入对应的产品、架构或领域文档。测试设计、测试夹具、跨平台边界和证据声明遵循[测试与验证规范](./docs/testing.md)。

## 开发环境

构建环境以仓库中的机器可读配置为准：

- Node.js 版本遵循 CI 声明的当前 LTS 策略，pnpm 版本由 `package.json` 的 `packageManager` 固定，前端依赖以 lockfile 为准；
- Rust 最低版本以 `src-tauri/Cargo.toml` 的 `rust-version` 为准；
- Linux、macOS 和 Windows 的系统依赖遵循 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)；
- 如需手动排查实际 WSL 行为，Windows 开发机需要 WSL2 和一个满足支持基线的发行版；这不是仓库自动验证的前置条件。

公开开发命令使用标准工具形式：

```bash
pnpm install --frozen-lockfile
pnpm dev
pnpm tauri dev
```

参与开发的 Agent 还需要遵循根目录 `AGENTS.md` 中的协作规则。开发环境应满足仓库声明的运行时和包管理器要求。

## 修改流程

1. 先确认问题属于哪一份长期文档，并阅读对应实现与测试。
2. 在修改共享函数、类型或模块前，执行 GitNexus 上游影响分析，确认直接调用方和受影响流程。
3. 功能和缺陷修复遵循测试先行（test-first），先让目标测试因缺少行为而失败，再实现最小改动。
4. 同一变更同步维护 Rust 公共契约、生成的 bindings、前端、ACL、国际化文案和长期文档。
5. 在有未提交改动的工作区中，只修改和验证本任务文件，不回退、覆盖或顺带整理其他人的改动。
6. 提交前执行 GitNexus `detect_changes`，再按风险运行相应验证。

设计说明、实施计划和阶段性评审可以保存在 Git 忽略的 `docs/plans/**` 或 `docs/superpowers/**` 中，但它们不是长期权威文档，也不得作为普通提交内容。最终行为必须进入源码、测试和对应的长期负责文档。

## 前端

前端负责呈现和用户交互，不在组件中重新实现后端业务规则。

### 组件与交互

- 优先使用仓库已有的 shadcn/ui 与 Radix 原语，再考虑增加新的基础组件。
- 按控件语义选择组件。选项集合使用 `Select`，二元状态使用 Checkbox/Switch，确认破坏性操作使用 Dialog/AlertDialog，短暂反馈使用 toast。
- 图标优先使用 `lucide-react`；只有图标的按钮必须提供可访问名称或提示。
- 页面、`Settings` 页面区域和高成本功能按路由或实际使用时机加载，不在应用启动时无条件拉取全部数据。
- 加载、空状态、错误、部分完成、过期和恢复状态都需要稳定布局和明确下一步，不能把请求失败显示成“没有数据”。
- Windows/WSL、Built-in/Custom 等技术差异只在用户需要作出决策时展示，不能把实现分层直接暴露成额外操作步骤。

### 状态与异步请求

- Zustand 使用字段级选择器，避免订阅整个 store。
- Environment、Context 或请求键变化后，异步响应必须使用请求代次或请求标识，防止旧结果覆盖新状态。
- Promise 拒绝需要落入用户可见错误或明确的业务降级，不能使用空 `catch` 吞掉连接、存在性或写入失败。
- 跨组件共享的运行时事件、窗口重新获得焦点后的刷新、缓存和进行中的请求去重由对应的 store 或 hook 负责，不能让多个页面各自监听并重复请求。
- 组件保存用户意图，后端的预览与执行结果才决定文件系统和变更事实。

### 国际化与文案

- 用户可见文案进入 `src/i18n/locales/en.json` 和 `zh-CN.json`，不能在组件中硬编码两套文本。
- 中文使用自然陈述，避免逐词翻译。Agent、Skill、Environment、Context、WSL 等产品或工程术语可以保留原文。
- 后端返回稳定的错误或警告代码及参数；前端负责本地化，不能直接展示后端内部英文摘要。

### 前端测试

测试分层、用户可观察行为、模拟数据的证据边界、确定性测试夹具和 React 异步约束统一遵循[测试与验证规范](./docs/testing.md)。

## 后端

后端的依赖方向为：

```text
commands -> application -> core/environment/storage
```

`src-tauri/src/lib.rs` 是组合根，负责构造长生命周期状态、注册命令、连接事件和执行启动维护。

### 分层规则

- `commands` 是 Tauri 传输适配层，只负责 DTO、`State`、调用来源、变更准入和错误转换。
- `application` 负责业务用例、运行时事实、计划器、内容快照会话、协调器和跨模块编排。
- `core` 保存稳定领域类型与纯规则，不依赖 Tauri 窗口或具体进程传输。
- `environment` 负责 Context、Environment、原生/WSL 后端、路径映射和文件系统能力。
- `storage` 负责原子文档、无损锁提交、内容快照和恢复持久化。
- Host/WSL 分支停留在 Environment 适配层，不复制安装、更新和移除等业务流程。

### 错误与安全边界

- 公共失败使用 `AppError` 或稳定的操作代码，不能依赖字符串匹配控制前端行为。
- 前端传入的路径不是授权凭据。读取、打开、删除和恢复命令接收类型化标识，后端必须重新解析并验证目标。
- 所有 Skill 变更复用预览与执行流程、计划器、协调器和恢复机制，不能在命令处理层直接写目录或锁文件。
- 内容快照（`Payload`）表示完整的 Skill 目录；任何落盘方式都不能只复制 `SKILL.md`。
- WSL 操作使用随应用发布的脚本资源、类型化请求与响应、位置参数或结构化标准输入，不能把用户输入拼进 shell 源码。
- 新增 WSL 操作时同时实现超时、取消、有界输出、协议版本检查和错误映射。

### 命令、bindings 与 ACL

Rust 命令接口是唯一的 IPC 权威。修改命令或公开 DTO 时同时完成：

1. 更新 Rust 命令、类型和正式注册；
2. 更新 `src-tauri/app_commands.rs`，保持排序和去重；
3. 将命令加入正确的 Tauri `permission` 与窗口 `capability`；
4. 重新生成 `src/bindings.ts`；
5. 更新 `useTauriApi` wrapper 与调用方；
6. 增加命令接口、ACL 和行为测试。

生成 bindings：

```bash
pnpm bindings:generate
pnpm bindings:check
```

Main 与 Install Wizard 只共享两边实际使用的安装与运行时命令；Settings、Recovery、Updater 等能力仍按窗口维持最小权限。不要为了通过调用而授权没有真实调用点的命令，也不要绕过应用命令清单直接依赖 UI 隐藏。

## 文档

长期文档按事实发生变化的原因划分，职责见[文档地图](./docs/README.md)。修改行为时只更新真正拥有该事实的文档，其他位置使用链接。

- 产品能力、页面交互或 Agent 筛选规则变化时更新 `docs/product.md`。
- 顶层分层、IPC、窗口、安全、运行时职责、WSL 传输、平台实现或跨层快照一致性变化时更新 `docs/architecture.md`。
- 来源、安装、读取、更新、修复、管理 Agent、复制或移除变化时更新 `docs/skill-lifecycle.md`。
- Agent 注册表、定义、检测、关联 Agent、筛选候选、选择分组或默认目标变化时更新 `docs/agents.md`。
- Environment、Context、项目绑定、路径解析或存储访问变化时更新 `docs/environments-and-contexts.md`。
- 内容快照、预览与执行、原子写入、取消或恢复变化时更新 `docs/execution-and-recovery.md`。
- 上游 CLI、共享 lock 或互操作行为变化时更新 `docs/skills-cli-compatibility.md`。
- 测试分层、测试夹具、跨平台能力或验证证据变化时更新 `docs/testing.md`。
- 开发、验证、CI 或发布流程变化时更新本文。

正文使用符合中文语境的陈述体。复杂结构和时序可以使用 Mermaid；简单规则不为了形式增加图示。不要维护能够从源码生成的完整命令、类型、组件、Agent 或制品清单。

## 本地验证

### 前端与脚本

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

### WSL shell 额外验证

只有修改随应用发布的 WSL shell 脚本或 WSL 传输契约时，才需要额外运行 CI 同款 ShellCheck：

```bash
docker run --rm \
  --volume "$PWD:/work" \
  --workdir /work \
  --entrypoint shellcheck \
  koalaman/shellcheck:v0.10.0 \
  -s sh src-tauri/src/environment/wsl/scripts/*.sh
```

该命令只静态检查随 Windows 应用发布的目标环境脚本，不要求本机安装 WSL，也不作为普通 Host、Linux 或 macOS 改动的额外前置条件。实际执行契约由仅在 Linux 上运行的 Rust 测试覆盖；Windows 和 macOS CI 只验证各自会编译、运行的平台分支。

根据改动范围先运行最小目标测试，再在完成前运行上述完整集合。只有最新命令输出为 exit 0 才能声称验证通过；已有基线失败需要记录命令、失败项和与本次改动的关系。选择目标测试、平台验收和桌面端 E2E 的规则见[测试与验证规范](./docs/testing.md)。

## CI

拉取请求的 CI 包含以下门禁：

- Rust MSRV 检查；
- 脚本测试、bindings、ESLint、Vitest 和前端构建；
- Windows、macOS、Linux 上的 Rust fmt/check/clippy/test；
- 随应用发布的 WSL `/bin/sh` 脚本的 ShellCheck。

原生集成测试使用真实的临时文件系统验证完整流程。WSL 验证由跨平台 Rust 解析器与协议测试、ShellCheck、Linux 上真实执行随应用发布的会话脚本，以及 Windows、macOS、Linux 三平台 Rust 构建矩阵组成；仓库不维护真实发行版的验收流程。

## 发布与应用更新

发布流程由 `v*` 标签或人工输入触发，并校验标签、包版本、变更日志条目和提交 SHA 一致。构建矩阵覆盖 macOS arm64/x64、Linux x64 和 Windows x64，为每个平台生成签名更新器制品，最后由单一汇总任务生成 `latest.json`，并保持 GitHub Release 为 Draft。

发布草稿前需要满足：

- 前端契约与构建、后端测试通过；
- 所有更新器安装包与签名文件非空，且元数据与标签/SHA 一致；
- `latest.json` 覆盖完整平台矩阵，URL 指向当前仓库和标签；
- Release 不包含内部元数据片段或意外的旧托管制品。

维护者确认 Draft Release 的版本、平台制品、签名和 `latest.json` 无误后，通过 GitHub UI 手动公开。发布不以真实 WSL 发行版验收为门禁；WSL 支持边界由前述自动化协议与 shell 验证固定。

应用更新器依赖 Tauri 的签名校验。不得发布缺少签名的安装包，也不得手工拼接与当前标签不一致的 `latest.json`。

## 提交范围

- 不提交构建输出、临时证据、上游 CLI 的本地辅助文件、`docs/plans/**` 或 `docs/superpowers/**`。
- 不提交 `src-tauri/permissions/autogenerated/**`；它由 Tauri 构建根据 `app_commands.rs` 生成，手写的窗口命令集合和 `capability` 需要提交。
- 不在同一提交中混入无关格式化、生成文件噪声或用户尚未确认的重构。
- 只有在 Rust 契约确实变化时才更新生成的 bindings。
- 修改发布流程或 CI 时保留现有 Rust、WSL 和更新器门禁，除非变更本身明确重新设计这些门禁。
