# 贡献指南

本指南说明参与 Skill Deck 开发、验证改动和提交 Pull Request（PR）的基本流程。产品与技术资料见[文档目录](./docs/README.md)。

## 参与贡献

可以通过问题反馈、代码、文档或翻译参与项目。缺陷和功能建议通过 [GitHub Issues](https://github.com/hccake/skill-deck/issues) 提交，提交前先搜索已有事项，避免重复记录。

缺陷报告应包含应用版本、操作系统、复现步骤、预期行为和实际结果；涉及 WSL 时注明发行版。必要时附上最小样例、日志或截图。功能建议说明使用场景和预期效果。

## 本地开发

Windows、macOS 和 Linux 使用各自的原生工具链，需要以下依赖：

- Node.js 当前 LTS 版本。
- [package.json](./package.json) 中 `packageManager` 指定的 pnpm 版本。
- 满足 [Cargo.toml](./src-tauri/Cargo.toml) 中 `rust-version` 要求的 Rust 工具链。
- 当前系统所需的 [Tauri 前置依赖](https://v2.tauri.app/start/prerequisites/)。

当前 Windows 构建还需要 WSL 辅助进程及摘要清单，启动前完成下文的[资源准备](#windows-构建资源)。macOS 和 Linux 桌面构建不需要这些资源。

在仓库根目录安装依赖并启动桌面应用：

```bash
pnpm install --frozen-lockfile
pnpm tauri dev
```

`pnpm tauri dev` 同时启动前端服务器和 Tauri 应用。单独调试前端界面可使用 `pnpm dev`；依赖前后端通信、插件和原生窗口的功能在完整桌面应用中验证。

### Windows 构建资源

Windows 工作区的 `src-tauri/target/wsl-worker/current/` 需要包含 `worker` 和 `manifest.json`，并与当前源码匹配。资源可以在 Linux 环境中生成，或通过本机已有的 WSL 2 发行版生成。应用内的 WSL 开关不影响这项构建要求。

本机已有 WSL 2 时，在发行版中准备 Rust 工具链及 `x86_64-unknown-linux-musl` 目标，再从 Windows 工作区执行以下命令；`Ubuntu` 替换为实际发行版名称：

```bash
pnpm prepare:wsl-worker -- --distro Ubuntu
pnpm verify:wsl-worker
```

使用独立的 Linux 环境时，在具有 Node.js 和 Rust 工具链的同一份源码中执行：

```bash
rustup target add x86_64-unknown-linux-musl
node scripts/prepare-wsl-worker.mjs
```

将生成的两份资源复制到 Windows 工作区的同一相对位置，再运行 `pnpm verify:wsl-worker`。校验只检查文件与清单的一致性，不启动 WSL。Worker 相关源码变化后重新取得或构建资源。

## 修改要求

- 每次改动围绕一个明确主题，先阅读相关文档、实现和调用方，复用现有组件与模块约定。
- 功能开发和缺陷修复遵循测试先行，先表达预期行为或复现问题，再实现改动。
- Rust 命令或公开类型变化时，按实际影响同步 `src-tauri/app_commands.rs` 注册、窗口权限、调用方和测试。通过 debug Tauri 启动重新生成前后端类型绑定 `src/bindings.ts`，生成文件与接口修改一并提交。
- 用户可见文案同步中英文语言包，遵循[界面文案约定](./docs/product.md#界面文案)。行为变化同步对应主文档和受影响的两份 README，版本变化记录在 `CHANGELOG.md`。
- 文档按主题陈述已确认的事实和规则，中文自然、术语统一，保留必要的上下文与限制。禁止机械翻译、问答式标题和思考过程。术语以 `CONTEXT.md` 为准，每项规则由对应主文档维护。

模块边界、异步状态和文件操作约束见[系统架构](./docs/architecture.md)与[执行与恢复](./docs/execution-and-recovery.md)。使用 Agent 协助开发时，同时遵循 [AGENTS.md](./AGENTS.md)。

## 提交前检查

先运行相关定向测试，再完成改动类型对应的检查：

| 改动内容 | 检查要求 |
|---|---|
| Markdown 文档 | `pnpm docs:check` |
| 前端组件、状态或工作流 | 相关测试，以及 `pnpm lint`、`pnpm test`、`pnpm build` |
| Rust 规则或业务用例 | 相关 Rust 测试、格式和静态检查，命令见下文 |
| Tauri 命令、公开类型或窗口权限 | 刷新 bindings，运行 `pnpm bindings:check`，以及命令、权限和相关前端测试 |
| 仓库脚本、CI 或发布配置 | `pnpm test:scripts`；涉及文档时增加文档检查 |
| 原生界面、主题或窗口布局 | 在目标平台运行 `pnpm test:desktop`，检查截图和日志 |
| WSL 脚本或传输协议 | 相关 Rust 测试与 ShellCheck，执行方式见[质量工作流](./.github/workflows/quality.yml)；涉及发行版时增加 Windows 与真实 WSL 联合检查 |

Rust 完整检查命令如下，按本次影响范围执行：

```bash
cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
cargo check --locked --workspace --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --locked --workspace --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --workspace --manifest-path src-tauri/Cargo.toml
```

覆盖范围及平台要求见[测试与验证规范](./docs/testing.md#按改动类型选择验证)。验证记录说明实际执行的命令、结果、平台和未验证范围；已有基线失败单独说明。

## 提交 PR

将改动推送到自己的分支或 Fork 后，通过 [GitHub](https://github.com/hccake/skill-deck/compare) 发起 PR。说明中包含：

- 改动原因、预期行为和影响范围，关联已有 Issue。
- 已执行的验证命令、结果和运行平台，以及尚未验证的部分。
- 界面变化的截图或录屏，以及必要的人工验收说明。

提交只包含本次改动所需的源码、测试、配置、长期文档和手写权限文件。生成的 bindings 与对应接口修改一并提交；构建输出、临时验证数据和本地第三方源码不进入提交。

提交前自查差异，避免无关格式化、生成文件噪声和未确认的重构。根据审阅意见更新同一个 PR，相关改动后重新运行受影响的检查。

PR 使用现有 [GitHub Actions](./.github/workflows/ci.yml) 完成跨平台质量检查，具体任务与平台矩阵见[共享质量工作流](./.github/workflows/quality.yml)。

### 发布与应用更新

维护者通过[发布工作流](./.github/workflows/release.yml)处理版本发布。`v*` 标签或人工输入已有标签触发版本、提交及 Changelog 校验，再对同一提交执行质量检查和各平台打包。Worker 在 Linux 上生成，由同一提交的 Windows 构建使用；发布前校验安装包中的资源。自动检查完成后，维护者核对版本、发布说明、安装包、更新签名和 `latest.json`，再公开 Release。

预发布的 Windows 安装包使用 NSIS，稳定版提供 NSIS 和 MSI；MSI 数值版本不支持 SemVer 预发布标识。Release 正文来自对应提交的 `CHANGELOG.md`，公开文件名由两份 README 维护。[资产校验器](./scripts/verify-release-assets.mjs)及其测试维护完整制品契约，其中 `latest.json` 上限为 1 MiB，更新安装资产上限为 256 MiB。修改工作流时同步相关脚本、测试和用户文档，并保持提交校验一致。
