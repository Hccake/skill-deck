# 测试与验证规范

## 目标与适用范围

本文负责 Skill Deck 的测试分层、证据边界、跨平台规则和验证要求，适用于 Rust 后端、React/TypeScript 前端、Tauri 命令、Native/WSL Environment、仓库脚本和 GitHub Actions。

测试围绕明确的不变量建立可信证据。每项测试都应说明验证的行为、运行平台、依赖的真实能力和失败定位方式。

## 测试分层与证据边界

测试按证据强度分为五层。低层测试提供快速、稳定的规则反馈，高层测试补充操作系统、桌面运行时和真实集成证据。

| 层级 | 适用内容 | 本层覆盖 | 另需验证 |
|---|---|---|---|
| L0 纯规则与领域单元测试 | 解析、哈希、计划、状态转换、错误映射 | 输入到输出的确定性规则 | 操作系统调用、动态库、Tauri 运行时和真实网络由更高层验证 |
| L1 适配器与契约测试 | 命令 DTO、ACL、协议、Git/HTTP 适配器 | 边界形状、序列化、权限映射和错误分类 | 真实桌面进程与目标平台行为由 L3/L4 验证 |
| L2 Native 工作流集成测试 | 临时目录中的真实文件树、lock、写入协调 | 多模块协作、真实文件读写、清理和冲突语义 | 其他操作系统语义和真实 WSL 发行版由平台验收补充 |
| L3 平台验收 | Windows、macOS、Linux 平台分支，junction/reparse point、Unix shell 和 WSL 传输协议 | 目标平台 API、加载器、权限和随应用发布脚本的行为 | WebView 协作由 L4 验证，具体 WSL 发行版按问题补充验收 |
| L4 应用启动冒烟与关键流程 | 真实 Tauri 应用、插件、窗口和少量关键用户路径 | 可执行文件启动以及 manifest、动态库、插件、窗口和关键链路协作 | 纯业务规则的输入组合由 L0-L2 覆盖 |

测试名称和 CI 任务应准确表达证据层级。采用 `MockRuntime`、模拟文件系统或 HTTP 测试服务器时，使用单元、契约或集成测试名称；`e2e`、`desktop` 和 `acceptance` 用于相应的真实运行环境。

当前仓库的常规自动化主要覆盖 L0 至 L3。Windows 集成测试验证 Common Controls manifest、原生对话框插件加载，以及删除 junction 时不跟随其目标等行为。完整 Tauri 应用启动属于 L4，交付说明会单独报告是否具备这项证据。

## 编写流程

1. 先写清不变量和失败条件，再选择测试层级。回归测试应在修复前直接复现同一个根因。
2. 先写最小失败测试，再实现行为。测试通过后，确认它能够区分正确实现和绕过测试的实现。
3. 明确平台范围：平台无关规则在三个 Host 平台运行，平台特有分支在对应平台运行，`cfg` 只包围真实的平台依赖。
4. 协议、文件系统、进程、加载器和 Tauri 启动等外部状态在对应集成层验证，模拟实现负责较低层规则和契约。
5. 完成后运行与改动范围相符的验证，并检查编译警告、未处理的异步错误、超时和资源清理。

## 跨平台规则

### Native 路径与 POSIX 路径

Rust 的 `std::path::Path` 和 `PathBuf` 按当前 Host 平台解释路径。Native 路径使用 `PathBuf::join`、`components` 等平台 API，测试也通过平台 API 组合路径。

WSL、远端 Unix、Git 协议和脚本参数中的 POSIX 路径属于协议数据，使用明确的 POSIX 路径解析器或值类型。跨平台规则测试覆盖 `/`、`.`、`..`、符号链接目标和绝对路径。

### 条件编译与平台覆盖

`#[cfg(unix)]`、`#[cfg(windows)]` 用于依赖操作系统 API、权限或进程能力的测试。解析、协议和业务规则保持跨平台运行。条件编译同时覆盖相关导入、测试夹具和辅助函数，确保 `-D warnings` 下没有未使用代码警告。

每个新增平台分支至少提供一项对应平台证据。合并请求 CI 无法运行的验证安排到定时、手动或发布阶段，并在测试与交付说明中记录证据范围。

### Cargo 测试目标与 Windows 可执行文件

Cargo 会分别构建并运行库单元测试、二进制测试、示例和 `tests/` 下的各个集成测试可执行文件，详见 [Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html) 与 [Cargo test](https://doc.rust-lang.org/cargo/commands/cargo-test.html)。编译证据和测试进程启动证据分别报告。

Windows manifest、Common Controls、资源文件、动态库和原生插件依赖覆盖所有会被 CI 启动的测试目标。Cargo 构建脚本按目标类型使用 `rustc-link-arg-bins`、`rustc-link-arg-tests` 等指令，详见 [Cargo build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html)。Tauri 的 Windows 资源配置以当前 lockfile 固定版本的实现为准。

涉及加载器、manifest 或原生插件的修复，需要在 Windows 执行环境中启动最小测试程序。`cargo check` 提供编译证据，进程启动提供加载器证据。Windows 应用 manifest 和并行程序集依赖遵循 [Microsoft application manifests](https://learn.microsoft.com/en-us/windows/win32/sbscs/application-manifests) 与 [TaskDialogIndirect](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-taskdialogindirect) 的要求。

### Unix shell 与 WSL

直接执行通用 `/bin/sh`、依赖 Unix 权限位或 POSIX 系统调用的测试使用 `#[cfg(unix)]`。随应用发布、并依赖 GNU 兼容用户态工具的 WSL 会话脚本，只在 `#[cfg(target_os = "linux")]` 下执行真实脚本测试。

WSL 操作的解析和协议测试在所有平台运行，ShellCheck 和脚本契约测试在 Linux CI 运行。会话脚本覆盖完整基线成功、工具缺失，以及工具存在但缺少本操作所需行为的情况，例如 `xargs -0/-r`、`sort -z/-f`、`sha256sum --`、`readlink -f --` 和稳定的 `stat` 输出。

上述自动化提供脚本和协议证据。真实 WSL 发行版、Windows 到 WSL 的传输链路和用户机器配置按具体问题补充验证。WSL 是 Windows 上的可选 Environment，常规 CI 和发布继续使用三平台构建、协议测试和 Linux 脚本测试作为门禁。

Shell 测试捕获标准输出、标准错误和退出状态，设置超时，并清理子进程与临时目录。用户值通过位置参数或结构化标准输入传入。ShellCheck 报告按错误处理，局部忽略需要说明理由。

## 测试夹具

### 文件系统

Native 文件系统工作流使用真实临时目录，并明确清理责任。测试覆盖目标缺失、目标已经存在、悬空链接，以及删除链接后目标内容继续保留等“按目录项处理链接”语义。

Windows junction 和 reparse point 可能表现为不同文件类型，目标路径前缀也与 Unix 不同。断言验证业务语义，平台返回的字符串或枚举名称只在对应适配器契约中固定。

Native 路径按当前平台构造；JSON、WSL 路径和协议内容中的 POSIX 路径使用独立表示。测试显式提供工作目录、Home、语言、Git 配置和环境变量。

### Git 与换行符

需要保持字节稳定的 Git 测试夹具使用仓库 `.gitattributes` 固定换行符，并在测试中显式写入 LF 或二进制内容。哈希向量需要说明哪些元数据属于领域契约，例如可执行权限位。Windows 无法提供 Unix 权限位时，应使用明确的平台向量，不能让测试因隐含假设随机失败。

Windows 平台验收使用 Windows 原生检出或克隆。Git worktree 的管理文件和路径由创建它的 Host Git 解释；每个平台在自己的检出中初始化测试夹具，从而保持路径、权限和换行符语义一致。

后台进程的常规跨平台证据由三平台 CI、Clippy 静态检查和 Windows 进程策略测试组成。真实应用窗口可见性在具备交互式桌面的 Windows 环境中观察，用于问题复现和辅助诊断。

### HTTP、Git 与进程

测试夹具覆盖协议边界、生命周期、超时、并发请求和资源清理。一次 `Read::read` 可能只返回部分字节，详见 [`std::io::Read`](https://doc.rust-lang.org/std/io/trait.Read.html)。HTTP 测试使用成熟的服务器库；本仓库已经使用 [`tiny_http::Server`](https://docs.rs/tiny_http/latest/tiny_http/struct.Server.html)。Git 测试使用可控仓库或传输。

网络失败、远端引用变化、克隆失败和请求退避都应能够注入并稳定复现。时间、随机数、端口和线程调度保持可控；测试使用通道、带截止时间的轮询或库提供的同步原语判断完成时机。

Git 来源的应用层测试使用确定性传输，并由传输返回克隆时捕获的修订号。真实 `file://` 克隆、远端引用探测和修订号一致性由独立的 `ProcessGitTransport` 契约测试负责。

## 后端测试规则

- 核心领域优先验证纯输入输出和不变量，Tauri、文件系统和进程依赖留在适配器与集成测试；
- 应用工作流使用真实临时文件系统和共享 `test_support`，验证预览、执行、冲突、恢复、结果过期、清理和错误映射；
- 适配器测试固定协议版本、ACL、DTO 和错误代码；普通字符串只用于日志或诊断；
- 共享测试辅助代码具有清晰生命周期和默认隔离，每项测试显式提供当前目录和环境变量；
- 忽略测试写明运行条件、证据层级和执行命令，并由其他自动化或人工验证提供回归证据；
- Rust CI 使用 `fmt --check`、`check --all-targets`、`clippy --all-targets -- -D warnings` 和 `cargo test --locked`。警告、panic 和测试进程启动失败都作为失败处理。

## 前端与 Tauri 测试规则

前端测试按用户可观察行为编写，遵循 [Testing Library guiding principles](https://testing-library.com/docs/guiding-principles)：优先通过 `role`、`label`、文本和可见状态定位元素。React 异步更新按 [`act`](https://react.dev/reference/react/act) 规则等待完成，测试结束时不应出现未处理的 Promise rejection 或 `act(...)` 警告。Vitest 是执行器，实际脚本以 `package.json` 为准。

Store 和工作流测试覆盖请求版本、过期响应、部分完成、错误与重试，以及 Environment/Context 隔离。Tauri API 模拟提供 IPC/ACL 契约和前端状态转换证据；真实 Wry/WebView 启动和窗口协作属于 L4 验证范围。

依赖真实应用启动、窗口、插件或 WebView 协作的改动补充相应 L4 验证。验证可以安排在合并请求、定时任务、发布任务或人工验收，并在交付说明中准确报告。

## CI 与验证矩阵

GitHub Actions 使用矩阵覆盖 Ubuntu、Windows 和 macOS，并保留 `fail-fast: false`，让同一次变更能够暴露各平台差异。失败时保留工具链、格式检查、编译检查、Clippy、测试和平台诊断信息，用于区分编译失败、测试进程加载失败、断言失败、进程超时和外部依赖失败。

不同改动至少需要以下验证：

| 变更 | 必需验证 |
|---|---|
| 纯 Rust 或领域规则 | 目标单元测试、格式检查、编译检查和 Clippy；涉及公共流程时运行完整 Rust 测试 |
| 文件系统、进程或 Environment | Linux、Windows、macOS Rust 矩阵；平台分支在对应执行环境真实运行 |
| WSL 脚本或协议 | 跨平台 Rust 解析与协议测试、ShellCheck、Linux 中执行随应用发布的脚本，以及三平台 Rust 矩阵 |
| Tauri 命令、ACL 或 bindings | 命令与 ACL 集成测试、`bindings:check` 和前端测试；依赖插件或启动行为时增加相应的 L4 证据 |
| 前端工作流 | `pnpm test:scripts`、`pnpm lint`、`pnpm test` 和 `pnpm build` |
| CI 或发布策略 | 工作流策略测试、目标任务审查和与改动相关的完整门禁 |

成本较高的平台验收可以按合并请求、定时、手动或发布阶段分层，每一层记录责任、触发方式和最近结果。覆盖率用于观察趋势和发现缺口，平台证据继续按测试层级报告。

具体命令和当前 CI 入口由[贡献指南](../CONTRIBUTING.md)负责。

## 回归测试与评审清单

提交回归测试前逐项确认以下条件：

- 测试在修复前能够复现同一个根因；加载器或测试启动问题通过启动对应可执行文件提供证据；
- 测试层级、运行平台和另需验证的能力已经写清；
- Native 路径、POSIX 路径、换行符、权限位、链接类型和 shell 能力分别使用对应表示；
- 测试夹具显式提供当前目录、环境变量、网络、系统语言和时序条件，并包含超时与清理；
- 每个 `cfg` 同时覆盖相关实现、导入和测试夹具，`cargo clippy ... --all-targets -- -D warnings` 输出没有警告；
- Windows、macOS 和 Linux 的相关结果都已观察，交付说明如实列出尚未运行的真实环境；
- CI 输出能够区分编译失败、测试进程启动失败、断言失败、进程超时和外部依赖失败；
- 修改共享测试辅助代码或生产符号前，已经确认直接调用方和受影响流程；提交前核对实际差异与预期范围一致。

## 权威参考

- [Cargo test](https://doc.rust-lang.org/cargo/commands/cargo-test.html)
- [Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html)
- [Cargo build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html)
- [Rust conditional compilation](https://doc.rust-lang.org/reference/conditional-compilation.html)
- [Rust `std::path`](https://doc.rust-lang.org/std/path/)
- [Rust `std::io::Read`](https://doc.rust-lang.org/std/io/trait.Read.html)
- [Tauri testing](https://v2.tauri.app/develop/tests/)
- [Microsoft application manifests](https://learn.microsoft.com/en-us/windows/win32/sbscs/application-manifests)
- [Microsoft TaskDialogIndirect](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-taskdialogindirect)
- [GitHub Actions matrix](https://docs.github.com/actions/using-jobs/using-a-matrix-for-your-jobs)
- [Git attributes](https://git-scm.com/docs/gitattributes)
- [Git worktree](https://git-scm.com/docs/git-worktree)
- [ShellCheck](https://www.shellcheck.net/)
- [Testing Library guiding principles](https://testing-library.com/docs/guiding-principles)
- [React `act`](https://react.dev/reference/react/act)
