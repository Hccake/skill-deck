# 测试与验证规范

## 目标与适用范围

本文是 Skill Deck 测试设计、编写、review 和 CI 验证的唯一 owner。它适用于 Rust Backend、React/TypeScript Frontend、Tauri command、Native/WSL Environment、脚本和 GitHub Actions。源码中的测试承担可执行行为，本文规定测试应该放在哪一层、能够证明什么，以及跨平台和 fixture 必须遵守的边界。

测试的目标不是让某个 runner 上的数字变绿，而是为一个明确的不变量提供可信证据。每个测试在提交前都必须能够回答：被验证的行为是什么、运行在哪个平台、依赖哪些真实能力、失败时能否定位到根因。

## 测试分层与证据边界

仓库采用按证据强度递进的分层。低层测试应该快且稳定，高层测试负责验证低层无法模拟的系统事实。

| 层级 | 适用内容 | 可以证明 | 不能证明 |
|---|---|---|---|
| L0 纯规则/Domain unit | parser、hash、planner、状态机、错误映射 | 输入到输出的确定性规则 | OS syscall、动态库、Tauri runtime、真实网络 |
| L1 Adapter/contract | command DTO、ACL、protocol、Git/HTTP adapter | 边界形状、序列化、权限映射、错误分类 | 真实桌面进程启动、目标平台特有行为 |
| L2 Native workflow integration | `tempdir` 中的真实文件树、lock、mutation、协调器 | 多模块协作、真实 filesystem 读写、清理和冲突语义 | 另一种 OS 的 filesystem 语义、WSL distro |
| L3 Platform acceptance | Windows/macOS/Linux 的平台分支、junction/reparse、Unix shell 和 WSL transport contract | 目标平台 API、loader、权限与 bundled shell 行为 | 没有实际启动应用时的 WebView/UI journey，也不能证明具体 WSL 发行版的全部差异 |
| L4 Started-application smoke/journey | 真实 Tauri application、插件、窗口和关键用户路径 | executable 能启动、manifest/DLL/plugin/window 注册和端到端关键路径 | 纯业务规则的全部组合 |

测试名称和 CI job 应体现层级或能力。`MockRuntime`、fake filesystem 或 HTTP stub 的测试不得命名为 `e2e`、`desktop` 或 `acceptance`，也不得在文档中声称它验证了真实应用启动。

当前仓库已有较完整的 L0-L2：Rust inline unit tests、`src-tauri/src/test_support`、Native workflow tests、Tauri ACL tests、Vitest component/store/workflow tests，以及三平台 Rust matrix。Windows integration harness 已覆盖 Common Controls manifest、dialog plugin loader 和 junction no-follow 语义，但它不启动 Tauri application，因此属于 L3，不作为 L4 证据。当前长期缺口是没有稳定的 L4 started-app smoke 门禁。WSL 不建设仓库级真实发行版 acceptance；连接基线由跨平台 parser/protocol tests、ShellCheck 和 Linux 上真实执行 bundled session script 共同约束。

## 编写流程

1. 先写不变量和失败条件，再决定测试层级。一个回归测试必须在修复前因同一个根因失败，而不是只碰巧覆盖相邻代码。
2. 先写最小失败测试（test-first），再实现行为；测试通过后检查是否仍然能区分正确实现和“绕过测试”的实现。
3. 明确平台矩阵：平台无关规则在所有 runner 运行；平台分支在对应 runner 运行；不能用跳过失败平台的 `cfg` 伪造覆盖率。
4. 为外部能力选择真实边界。协议、filesystem、process、loader 和 Tauri startup 这类事实必须在对应集成层验证，不能只用 mock 得出结论。
5. 测试通过后运行与改动范围相符的完整门禁，并检查 `-D warnings`、未处理 rejection、超时和资源清理。

## 跨平台规则

### Host path 与 guest path

Rust `std::path::Path`/`PathBuf` 按当前 host 平台解释路径。Native host 路径必须使用 `PathBuf::join`、`components` 等平台 API，测试不得硬编码 `/` 或 `\\`。

WSL、远程 Unix、Git 协议或脚本 payload 中的 POSIX 路径属于 guest/string domain，不得在 Windows 上交给 host `Path` 解析。应使用明确的 POSIX parser/value type，并对 `/`、`.`、`..`、link target 和绝对路径写跨平台规则测试。

### `cfg` 与平台覆盖

`#[cfg(unix)]`、`#[cfg(windows)]` 只用于确实依赖 OS API、权限或进程能力的测试。解析、协议和业务规则应保持跨平台运行。条件编译必须包住对应的 import、fixture 和 helper，避免在 `-D warnings` 下留下 unused import。

每个新增 platform branch 至少有一个目标平台测试；如果 CI 无法在 Pull Request 运行，必须登记到 scheduled/manual/release acceptance，并在测试名称和文档中说明未覆盖的能力。

### Cargo test target 与 Windows executable

Cargo 会为 library unit tests、binary tests、examples 和 `tests/` 下的每个 integration test 分别构建和运行 executable（见 [Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html) 与 [Cargo test 文档](https://doc.rust-lang.org/cargo/commands/cargo-test.html)）。因此“能编译”不等于“test harness 能启动”。

Windows manifest、Common Controls、资源文件、DLL 或 plugin 依赖必须覆盖所有会被 CI 启动的 test target，而不只是应用主 binary。Cargo build script 需要按 target 类型使用 `rustc-link-arg-bins`、`rustc-link-arg-tests` 等指令（见 [build script instructions](https://doc.rust-lang.org/cargo/reference/build-scripts.html)）；Tauri 的 Windows resource 配置也应以当前 lockfile pinned 的 `tauri-build` 源码为准（[WindowsAttributes source](https://github.com/tauri-apps/tauri/blob/tauri-build-v2.5.5/crates/tauri-build/src/lib.rs)）。涉及 loader、manifest 或 native plugin 的修复，必须在 Windows runner 实际执行最小测试 executable；`cargo check` 不能替代启动验证。Windows 应用 manifest 和 side-by-side 依赖遵循 [Microsoft application manifests](https://learn.microsoft.com/en-us/windows/win32/sbscs/application-manifests) 与 [TaskDialogIndirect](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-taskdialogindirect) 的要求。

### Unix shell 与 WSL

直接执行 `/bin/sh`、依赖 Unix mode bit 或 POSIX syscall 的测试使用 `#[cfg(unix)]`。WSL operation 的 parser/protocol 测试在所有平台运行；shell asset 的 [ShellCheck](https://www.shellcheck.net/) 和真实执行在 Linux CI 运行。Bundled session script 需要分别覆盖完整基线成功，以及 Git、`xargs -0`、`sort -z`、`sha256sum`、`readlink -f`、稳定 `stat` 任一不可用时连接失败。三平台 Rust matrix 不启动真实 WSL 发行版，也不把发行版差异描述为已验证。

Shell 测试必须捕获 stdout、stderr 和 exit status，设置 timeout，并保证子进程和临时目录清理。用户值只能通过 positional arguments 或结构化 stdin 传入，不能拼接 shell source。ShellCheck 报告（包括 [SC2016](https://www.shellcheck.net/wiki/SC2016)）按错误处理，除非有带理由的局部 suppress。

## Fixture 规则

### Filesystem

Native filesystem workflow 使用真实临时目录和显式 cleanup。测试必须覆盖 missing、existing、dangling link，以及删除 link 后 target 仍保留等 no-follow 语义。Windows junction/reparse point 可能表现为不同 file type，且 target prefix 与 Unix 不同；断言应验证语义，不应只比较某个平台的字符串或枚举名称。

路径断言使用当前平台构造；fixture 内容中的协议路径、WSL 路径和 JSON 字段使用明确的 guest/domain 表示。不要依赖当前工作目录、用户 home、系统 locale、全局 Git config 或预先存在的环境变量。

### Git 与 line ending

需要字节稳定的 Git fixture 使用仓库 `.gitattributes` 固定 line ending，并在测试中显式写入 LF 或 binary bytes。Git 属性行为以 [gitattributes 文档](https://git-scm.com/docs/gitattributes) 为准。hash vector 必须写清哪些 metadata 是 domain contract（例如 mode bit）；Windows 不具备 Unix mode 时应采用平台-aware vector，而不是让测试隐式失败。

Windows acceptance 不应复用由 WSL/Linux 创建的 linked worktree。Git worktree 的管理文件和路径由 host Git 解释（见 [git-worktree](https://git-scm.com/docs/git-worktree)）；跨 OS 共享同一 worktree 会把路径、权限和 line ending 假设带入测试。需要 Windows 行为时，在 Windows runner 使用 native checkout/clone，并将 fixture 初始化纳入测试生命周期。

后台进程的跨平台门禁由三平台 CI、Clippy 静态检查和 Windows 进程策略测试组成。针对真实 `app.exe` 的可见窗口检查只能在具备交互式桌面的 Windows 环境中证明用户界面现象，因此作为问题复现和辅助诊断手段，不作为所有开发者、合并请求或跨平台发布的硬性门禁。

### HTTP、Git 和 process fixture

fixture 必须完整实现协议 framing、生命周期、超时、并发请求和 graceful cleanup。一次 `Read::read` 可能只返回部分 bytes（见 [`std::io::Read`](https://doc.rust-lang.org/std/io/trait.Read.html)），不要手写假定“一次 read 就返回完整请求”的 TCP server；HTTP 使用成熟 fixture library（本仓库已采用 [`tiny_http::Server`](https://docs.rs/tiny_http/latest/tiny_http/struct.Server.html)），Git fixture 应通过可控 repository/transport 建立，不依赖公共网络。网络失败、remote ref 变化、clone failure 和 retry/cooldown 必须是可注入、可重复的场景。

时间、随机数、端口和线程调度都应可控。测试不得通过 sleep 猜测完成时机；使用 channel、poll with deadline 或 library 提供的同步原语。Git source 的 application tests 使用不启动 Git process 的 deterministic transport，并由 transport 返回 clone 时捕获的 revision；真实 `file://` clone、ref probe 和 revision 一致性由独立的 `ProcessGitTransport` contract test 负责。

## Backend 规则

- Core/domain 优先测试纯输入输出和不变量；不要为了测试把 Tauri、filesystem 或 process 依赖泄漏进纯模块。
- Application workflow 使用真实 temporary filesystem 和共享 `test_support`，验证 preview/execute、conflict、recovery、stale request、cleanup 和错误映射。
- Adapter 测试固定 protocol/version、ACL、DTO 和 error code。字符串只作为日志或诊断断言，不作为稳定业务契约。
- 共享 test helper 必须拥有清晰的生命周期和默认隔离；禁止通过全局 mutable state、当前目录或环境变量在测试之间传递状态。
- 新增 ignored test 必须写明运行条件、覆盖的证据层级和执行命令；ignored 不是通过，也不能作为唯一的回归覆盖。
- Rust CI 使用 `fmt --check`、`check --all-targets`、`clippy --all-targets -- -D warnings` 和 `cargo test --locked`。warning、panic、harness startup failure 都是失败，不得通过放宽 lint 隐藏平台问题。

## Frontend 与 Tauri 规则

Frontend test 按用户可观察行为编写，遵循 [Testing Library guiding principles](https://testing-library.com/docs/guiding-principles)：优先 role、label、文本和可见状态，不锁定无意义 DOM 结构。React 异步更新必须按 [`act`](https://react.dev/reference/react/act) 规则等待完成；测试输出不得有 unhandled rejection 或 `act(...)` warning。Vitest 是执行器，具体脚本以 `package.json` 为准。

Store/workflow 测试必须覆盖 request generation、stale response、partial/error/retry，以及 Environment/Context 隔离。Tauri API mock 只证明 IPC/ACL contract 和 Frontend 状态转换；Tauri 官方的 [mock runtime testing](https://v2.tauri.app/develop/tests/) 不启动真实 Wry/WebView，不能替代 L4。

仓库应补充一个跨平台 started-app smoke suite，至少验证：测试 executable 能启动、Tauri plugin/native dialog 能加载、window/capability/command 注册成功，以及一条关键 workflow 能完成。该 suite 可按 runner 成本安排在 PR、nightly 或 release gate，但必须有明确 owner 和可追溯 artifact。

## 本轮故障映射

| 故障现象 | 根因类别 | 规范要求 |
|---|---|---|
| ShellCheck `SC2016` | shell asset 没有作为 CI lint gate 管理 | ShellCheck 在 Unix CI 运行，warning 视为失败，必要 suppress 必须有理由 |
| Windows `TaskDialogIndirect` / `STATUS_ENTRYPOINT_NOT_FOUND` | test executable 没有得到与主 binary 相同的 Windows manifest/resource | 按 Cargo test target 注入资源，并在目标 runner 实际启动 harness；L4 smoke 负责长期回归 |
| Windows 反斜杠、WSL POSIX path 断言失败 | host path 与 guest path 混用 | Native 使用 `PathBuf`；WSL/remote path 使用独立 string/parser domain |
| `Permissions::from_mode`、`/bin/sh`、Unix mode vector 在非 Unix 失败 | Unix capability 被错误地当成跨平台规则 | 用 `cfg` 隔离 syscall/fixture；纯 parser/protocol 仍跨平台运行 |
| junction/reparse type、link target prefix 和删除行为差异 | Windows filesystem 语义未被真实验证 | Windows acceptance 使用真实 link workflow，断言 no-follow 语义而非字符串/枚举偶然值 |
| macOS/Windows Git clone、remote probe 和 hand-rolled TCP fixture 失败 | fixture 依赖平台时序、short read 或外部网络 | 使用完整 HTTP fixture、deterministic Git fake 和少量真实 process contract；所有网络失败可注入、可复现 |
| macOS/Windows `unused import` 在 `-D warnings` 下失败 | 条件编译只包住了测试主体，没有包住 import/helper | `cfg` 同时包住实现、import 和 fixture，并在 all-targets clippy 下验证 |
| 测试进程在 harness 前退出 | 只做了 compile/check，没有覆盖 loader/startup 证据 | 将 harness startup 与 plugin/window 注册纳入 L4 smoke，不能用 unit test 数量替代 |

## CI 与验证矩阵

GitHub Actions 使用 matrix 覆盖 Ubuntu、Windows、macOS，并保留 `fail-fast: false`，让一次变更同时暴露平台差异。Matrix 设计遵循 [GitHub Actions matrix 文档](https://docs.github.com/actions/using-jobs/using-a-matrix-for-your-jobs)；runner 行为以 [runner-images](https://github.com/actions/runner-images) 为准。失败时上传 toolchain、fmt/check/clippy/test 和平台诊断 artifact，便于区分编译失败、harness loader 失败和测试断言失败。

验证至少分为：

| 变更 | 必需验证 |
|---|---|
| 纯 Rust/domain | 目标 unit tests、fmt、check、clippy；涉及公共流程时补全 `cargo test` |
| filesystem/process/Environment | Linux + Windows + macOS Rust matrix；平台分支在对应 runner 真实执行 |
| WSL script/protocol | 跨平台 Rust parser/protocol tests、ShellCheck、Linux 上执行 bundled shell，以及三平台 Rust matrix |
| Tauri command/ACL/bindings | ACL/command integration、`bindings:check`、Frontend tests；涉及 plugin/startup 时补 L4 |
| Frontend workflow | `pnpm test:scripts`、`pnpm lint`、`pnpm test`、`pnpm build` |
| CI/release policy | 对 workflow 的 policy tests、目标 job dry-run/审查和完整相关门禁 |

大而昂贵的 acceptance 不应被偷偷改成 skip。可以按 PR、nightly、manual 或 release 分层，但每一层的责任、触发方式和最近一次结果必须可查。可将 coverage 作为趋势和缺口诊断；在测试语义尚未稳定前，不以单一百分比 gate 替代平台证据。

## 回归测试与 review 清单

提交回归测试前逐项确认：

- 测试在修复前确实复现同一个根因；如果是 loader/harness 问题，必须启动对应 executable，而不是只增加 compile test。
- 测试层级、平台和未覆盖能力已写清，mock 没有被描述成真实系统验证。
- host path、guest path、line ending、mode bit、link type 和 shell 能力没有混用。
- fixture 不依赖 ambient CWD、环境变量、网络、locale、时序或全局状态，并有 timeout 和 cleanup。
- 每个 `cfg` 同时包住实现、import 和 fixture；`cargo clippy ... --all-targets -- -D warnings` 无 warning。
- Windows、macOS、Linux 的结果都被观察；条件编译排除的能力必须明确证据边界，不能把未运行的真实环境描述为已验证。
- CI 失败日志能区分 build、harness startup、test assertion、process timeout 和 external dependency failure。
- 改动共享测试 helper 或生产 symbol 前完成 GitNexus impact analysis；提交前运行 `detect_changes`，确认受影响范围符合预期。

## 当前改进优先级

1. **P0：保持现有三平台 Rust matrix、ShellCheck、diagnostics artifact 和 warning-free gate，并补充根 `.gitattributes` 固定 fixture 的 LF/binary 语义。** 这些门禁已经捕获了本轮 Windows manifest、path、junction、Unix shell、line ending、fixture protocol 和 conditional import 问题，不能退回到单平台或只编译不运行。
2. **P1：建立主发布平台的 L4 started-app smoke。** 低频覆盖 loader/manifest、窗口与 plugin 注册、HTTP、外链，以及 Wizard 的 Project、audit 和 install 关键旅程；不建设三平台完整 GUI E2E。
3. **P1：沉淀跨平台 test helper 和 domain value type。** 统一 native path、POSIX guest path、fixture bytes、link capability 和 process timeout，减少平台偶然行为对 unit test 的影响。
4. **P2：按 L0-L4 拆分过大的 inline test module 和 shared support。** 先改善命名和 helper ownership，再在有明确维护收益时移动文件；不为形式重构测试。
5. **P2：增加 coverage 和 flaky-test 趋势报告作为诊断。** 先看 platform/branch/fixture 缺口，再决定是否设置局部 gate。

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
- [GitHub Actions artifacts](https://docs.github.com/actions/using-workflows/storing-workflow-data-as-artifacts)
- [Git attributes](https://git-scm.com/docs/gitattributes)
- [Git worktree](https://git-scm.com/docs/git-worktree)
- [`tiny_http::Server`](https://docs.rs/tiny_http/latest/tiny_http/struct.Server.html)
- [ShellCheck](https://www.shellcheck.net/)
- [Testing Library guiding principles](https://testing-library.com/docs/guiding-principles)
- [React `act`](https://react.dev/reference/react/act)
