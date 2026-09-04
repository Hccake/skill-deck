# 贡献指南

Skill Deck 使用 Tauri、React、TypeScript 和 Rust 构建。本文件说明参与开发时需要遵循的修改流程、项目约定、验证命令以及 CI 和发布流程。产品、领域和架构规则由[文档地图](./docs/README.md)中的对应文档维护，测试设计与验证范围由[测试与验证规范](./docs/testing.md)维护。

## 开发环境

本地开发需要以下环境：

- Node.js 当前 LTS 版本；
- `package.json` 中 `packageManager` 指定的 pnpm 版本；
- 满足 `src-tauri/Cargo.toml` 中 `rust-version` 要求的 Rust 工具链；
- 当前操作系统所需的 [Tauri 前置依赖](https://v2.tauri.app/start/prerequisites/)。

安装依赖后，可以分别启动前端开发服务器或完整桌面应用：

```bash
pnpm install --frozen-lockfile
pnpm dev
pnpm tauri dev
```

`pnpm dev` 只启动前端开发服务器，适合调试不依赖 Tauri 运行时的界面；`pnpm tauri dev` 启动完整桌面应用。只有复现或验收真实 WSL 行为时，才需要在 Windows 开发机上准备 WSL2 和目标发行版。

在 Windows 上调试 WSL 2 连接前，先使用目标发行版准备本次源码对应的 Worker：

```bash
pnpm prepare:wsl-worker -- --distro Ubuntu
```

该命令在指定的 WSL 2 发行版中构建固定的 musl target，并将 Worker 和摘要 manifest 写入本地 Cargo `target` 目录。源码、构建输出和测试制品不得混用其他工作区生成的 Worker。

已有 Worker artifact 可以单独校验：

```bash
pnpm verify:wsl-worker
```

Windows Tauri 构建从该目录复制 resource。macOS 和 Linux 桌面构建不要求也不携带 Worker。

通过 Agent 参与开发时，还需要遵循根目录 [`AGENTS.md`](./AGENTS.md) 中的协作规则。

## 修改流程

1. 先通过[文档地图](./docs/README.md)确认规则所属的主文档，再阅读相关实现、调用方和测试。
2. 功能开发和缺陷修复遵循测试先行（test-first）。先增加能够表达目标行为或复现问题的测试，再实现满足该测试的最小改动。
3. 修改共享函数、类型或模块前，确认受影响的调用方、用户流程和公共契约，避免扩大变更范围。
4. 只同步本次变更实际影响的内容。Rust 命令或 DTO 变化需要同步前后端类型绑定（bindings）和窗口权限；用户可见文案变化需要同步中英文语言包；稳定行为变化需要更新对应主文档。
5. 先运行最小目标测试，再根据改动类型完成相应验证。提交前检查实际差异、生成文件和受影响的平台。

## 前端修改

### 组件与交互

- 优先复用仓库现有的 shadcn/ui、Radix 原语和 `lucide-react` 图标。
- 根据交互含义选择控件：选项集合使用 `Select`，二元状态使用 Checkbox 或 Switch，破坏性操作使用 Dialog 或 AlertDialog，短暂反馈使用 toast。
- 只有图标的按钮必须提供可访问名称；不熟悉的图标还需要提供提示。
- 页面、设置区域和加载成本较高的功能按实际使用时机加载，应用启动阶段只请求首屏所需数据。
- 加载、空状态、错误、部分完成、结果过期和恢复状态使用稳定布局，并提供与当前状态对应的处理入口。
- Windows 与 WSL、随应用提供和用户添加的 Agent 信息等差异，只在用户需要据此作出选择时展示。

### 状态与异步请求

- Zustand 使用字段级选择器，避免订阅整个状态模块。
- 操作位置、当前项目或请求条件变化后，异步响应必须通过请求标识确认仍然有效，避免旧结果覆盖当前状态。
- Promise 被拒绝时，必须转换为用户可见的错误，或者进入产品已经定义的可恢复状态。
- 跨组件共享的运行时事件、窗口重新获得焦点后的刷新、缓存和进行中的请求去重，由对应的状态模块或 Hook 统一负责。
- 前端保存用户意图；文件系统状态和实际变更结果以后端预览与执行结果为准。

### 国际化

- 用户可见文案统一保存在 `src/i18n/locales/en.json` 和 `src/i18n/locales/zh-CN.json` 中，组件通过国际化键读取。
- 中文文案按照中文语境组织。Agent、Skill、WSL 等术语可以保留原文，代码类型只在实现语境中使用。
- Backend 返回稳定的错误代码和参数，前端根据这些信息生成本地化反馈；内部摘要只用于开发信息和本机日志。

前端测试的定位方式、异步状态、定时器、对象断言和模块 mock 规则见[测试与验证规范](./docs/testing.md#前端与-tauri-测试)。

## Rust、IPC 与窗口权限

后端模块职责、依赖方向和平台 Adapter 边界见[系统架构](./docs/architecture.md)。路径安全、文件修改、并发冲突和恢复规则见[执行与恢复](./docs/execution-and-recovery.md)。实现变更需要遵守这些文档维护的边界。

Rust 命令接口是进程间通信（IPC）的权威。修改命令或公开 DTO 时，需要同步完成：

1. 更新 Rust 命令、类型和正式注册；
2. 更新 `src-tauri/app_commands.rs`，保持命令排序和去重；
3. 将命令加入正确的 Tauri `permission` 和窗口 `capability`；
4. 启动一次 debug Tauri 应用，由 `tauri-specta` 重新生成 `src/bindings.ts`；
5. 更新 `useTauriApi` 封装及其调用方；
6. 增加命令接口、窗口权限和行为测试。

新增或修改文件操作、外部进程和 WSL 操作时，还需要确认：

- 后端根据类型化标识重新解析并验证目标，前端路径只用于展示；
- WSL 请求通过位置参数或结构化标准输入传递用户数据，并提供超时、取消、有界输出、协议版本检查和错误映射；
- 平台分支只实现目标文件系统或进程能力，安装、更新、复制和移除等业务流程继续由共享用例负责。

## 文档修改

长期规则只在[文档地图](./docs/README.md)指定的主文档中维护，其他文档保留理解当前主题所需的摘要和链接。领域术语变化先更新 `CONTEXT.md`，再同步受影响的主文档。

面向用户的产品能力、安装步骤或使用说明发生变化时，同时更新 `README.md` 和 `README.zh-CN.md`，两种语言保持相同的事实范围。完整命令、类型、组件、Agent 和发布资产清单由源码、配置和生成工具维护。

中文内容使用符合中文语境的陈述体。具体写作规则见[文档地图](./docs/README.md#写作规则)。

## 本地验证

根据改动内容选择验证，不要求每项修改运行全部命令。常见改动对应的基础验证如下：

| 改动内容 | 应执行的命令 |
|---|---|
| Markdown 文档 | `pnpm docs:check` |
| 仓库脚本、CI 或发布策略 | `pnpm test:scripts`；涉及 Markdown 时增加 `pnpm docs:check` |
| 前端组件、状态或工作流 | 相关 Vitest；完成前运行 `pnpm lint`、`pnpm test` 和 `pnpm build` |
| Rust 规则或业务用例 | 相关 Rust 测试；完成前运行格式检查、静态检查和受影响的 Rust 测试 |
| Tauri 命令、公开 DTO 或窗口权限 | 使用 `pnpm tauri dev` 刷新 bindings，运行 `pnpm bindings:check`，以及命令、权限和相关前端测试 |
| WSL shell 脚本或传输协议 | 相关 Rust 测试和下述 ShellCheck；涉及真实发行版时增加 Windows 与 WSL 联合检查 |

Rust 的完整检查命令为：

```bash
cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
cargo check --locked --workspace --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --locked --workspace --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --workspace --manifest-path src-tauri/Cargo.toml
```

修改随应用发布的 WSL bootstrap 脚本或传输契约时，运行与 CI 相同的 ShellCheck：

```bash
docker run --rm \
  --volume "$PWD:/work" \
  --workdir /work \
  --entrypoint shellcheck \
  koalaman/shellcheck:v0.10.0 \
  -s sh src-tauri/src/environment/wsl/scripts/*.sh
```

更具体的测试选择、平台覆盖和桌面应用验收要求见[测试与验证规范](./docs/testing.md#按改动类型选择验证)。交付说明根据最新命令输出记录已验证的平台和仍未检查的场景；已有基线失败需要说明失败项及其与本次变更的关系。

## CI

拉取请求和主分支推送通过 `.github/workflows/ci.yml` 调用共享工作流 `.github/workflows/quality.yml`。共享工作流检查指定的提交 SHA，并由 `quality-gate` 汇总以下结果：

- Rust 最低版本；
- 仓库脚本、文档链接、前后端类型绑定、ESLint、Vitest 和前端构建；
- GitHub Actions 工作流与 WSL shell 脚本；
- Rust 格式，以及 Windows、macOS、Linux 上的静态检查和测试。

修改 CI 时，应同步更新对应的脚本测试，并确认共享工作流仍然校验调用方指定的提交。具体任务、矩阵和诊断制品以工作流文件为准。

## 发布与应用更新

发布流程由 `v*` 标签或人工输入已有标签触发。流程先核对标签、`package.json` 版本、`CHANGELOG.md` 条目和目标提交，再对同一提交运行共享质量检查。

质量检查通过后，发布流程在 Linux 构建并校验固定的 Worker artifact，Windows 构建下载同一提交的 artifact 并装入安装包。官方 `tauri-action` 构建和上传各平台安装包、更新签名与 `latest.json`；Windows job 随后检查 NSIS 和 MSI 中的 Worker resource。自动校验完成后，由维护者检查版本、Release 正文、安装包和更新信息，再通过 GitHub 界面公开发布。

预发布版本在 Windows 上提供 NSIS 安装包；稳定版本同时提供 NSIS 和 MSI。MSI 的数值版本不能表达 SemVer 预发布标识，发布工作流和资产验证器根据版本类型使用对应的安装包契约。

Release 正文来自标签对应提交中的 `CHANGELOG.md`。公开安装包名称由 `README.md` 和 `README.zh-CN.md` 维护，完整资产和更新清单契约由 `scripts/verify-release-assets.mjs` 及其测试维护。该契约要求 `latest.json` 不超过 1 MiB，更新器引用的安装资产不超过 256 MiB。修改发布流程时，需要同步更新工作流、验证脚本、脚本测试和受影响的用户文档。

## 提交内容

- 提交只包含当前任务需要的源码、测试、配置、长期文档和手写权限文件。
- Rust 公共契约变化时重新生成前后端类型绑定，并与契约修改放在同一提交中。
- 构建输出、临时验证数据和本地第三方源码不进入提交。
- 每个提交围绕一个明确主题组织，不包含无关格式化、生成文件噪声或尚未确认的重构。
