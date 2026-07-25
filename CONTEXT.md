# Skill Deck 运行环境上下文

本上下文描述 Skill Deck 中运行环境、环境切换和项目注册信息之间的产品语义。它只记录稳定的领域语言，不记录具体实现方式。

## 环境与上下文

**Environment（运行环境）**：Skill Deck 执行 Skill、读取配置和访问项目所依据的 Host 或某个 WSL 发行版。
_Avoid_: 把 WSL 发行版本身称为项目、连接或后台任务。

**环境连接**：确认某个 Environment 当前可以被 Skill Deck 使用的过程。连接成功表示环境可用，不代表该环境中的项目注册信息已经加载完成。
_Avoid_: 把环境连接和项目加载合称为一次不可区分的切换操作。

**环境切换**：用户将当前选中的 Environment 从一个环境改为另一个环境的产品行为。连接成功后即可完成环境切换；项目注册信息随后独立加载，失败时不撤销已经完成的环境切换。
_Avoid_: 用“切换成功”泛指连接、项目加载和运行时维护全部完成。

**项目注册信息**：某个 Environment 中已登记的项目绑定及其展示信息。它是环境可用性之外的独立状态，可以处于加载中、可用或加载失败。
_Avoid_: 把项目注册信息为空和项目注册信息加载失败混为一谈。

**Storage owner（存储归属）**：实际保存项目或 Skill 路径、并决定其 filesystem 语义和写入边界的 Environment。当前执行 Environment 可以与 storage owner 不同，但二者不能因此被视为同一个写入位置。
_Avoid_: 仅根据当前选中的 Environment 推断路径归属，或把 storage owner 当成新的 Environment 类型。

**跨 storage 访问**：当前 Environment 读取或观察由其他 storage owner 保存的路径。它可以用于只读事实、风险提示和引导切换，但不进入受保护写入。
_Avoid_: 把跨 storage 的 capability fallback 当成受支持的 Skill 写入模式。

**跨 Environment copy**：来源 Environment 与目标 Environment 可以不同。来源只提供已经固定的完整 Skill 内容，目标 Environment 必须同时是目标路径的 storage owner，并在该 Environment 中完成受保护写入。它是内容传递边界，不是让一个 Environment 代替另一个 owner 写入其路径。
_Avoid_: 把跨 Environment copy 等同于跨 storage protected write，或把来源 Environment 的 backend 当成目标路径的写入 owner。

**复制来源 provenance 与 lineage**：跨 Environment copy 完成后，目标 Project 始终拥有自己的目录、Context 和 lock 生命周期。对 Remote、Git 和 Well-known 等可重新获取的来源，目标 lock 保留 source、ref、Skill path 和内容基线作为更新 lineage；对 Local 来源，这些字段只作为 provenance，复制结果不具备自动更新能力。
_Avoid_: 把 Local provenance 当成远端更新能力，或把来源 Environment/Project 的当前可用性当成可重新获取来源的目标 Skill 的持续依赖。

**目标环境重新获取**：带有可重新获取 lineage 的目标 Skill 更新时，直接在目标 storage owner Environment 获取来源并执行写入，不追踪或自动重连原来源 Environment。网络、凭据或来源不可用时，按来源获取失败处理。
_Avoid_: 为了更新一个远端来源保存来源 Environment 拓扑，或把 source acquisition 与原始 copy Environment 绑定。

**已固定 payload 执行**：payload 一旦固定，后续 Execute 不再重新读取原始来源。Install、Update、Repair 中，当前 Environment 同时负责获取和写入，因此它仍必须在线；只有跨 Environment Copy 存在独立的来源 Environment，来源在 payload 固定后可以断开，但目标 Environment 仍必须在线。payload 固定前获取失败按普通获取失败处理；固定后不因来源断开自动触发重连或重新获取。
_Avoid_: 把所有 Source 都附会成独立的来源 Environment，或为了维持 Copy 的旧来源连接建立后台重连状态机。

**WSL 最低用户态**：Skill Deck 正式支持的 WSL Environment 基线，包括常见 Ubuntu/Debian 等 GNU/Linux 用户态、Git、POSIX shell 和当前操作所需的 GNU coreutils。超出这条基线的发行版或用户态组合不自动获得完整兼容承诺。
_Avoid_: 把发现到的每个 WSL distro 都称为完整受支持的 Environment。

**不满足基线的 WSL Environment**：无法满足 WSL 最低用户态或当前操作前置条件的 Environment。它应快速失败并说明缺少的条件，不通过隐藏 fallback 把不确定的执行结果伪装成成功。
_Avoid_: 用 capability 矩阵或猜测性 shell 替代来扩大正式支持范围。

**WSL baseline preflight**：连接时对 WSL 最低用户态进行的一次二元检查，结果只表示当前 Environment 是否满足正式支持基线，并在失败时给出缺失条件。具体 Skill 操作仍可以执行自己的窄 preflight，但不汇总成长期 capability matrix。
_Avoid_: 把每项底层工具或 filesystem primitive 建模成用户长期管理的能力状态。

**Runtime Maintenance（运行时维护）**：Environment 可用后由应用执行的后台一致性检查和清理工作。它描述 Environment 的写入准备状态，不属于 Recovery Resource，也不代表用户请求的 Skill 操作失败；失败后通过重新连接或重启建立新的尝试，不在前端维护长期 retry 状态机。
_Avoid_: 将正常的维护进行中状态称为告警、Recovery 或用户操作失败。

**维护重新进入**：用户处理 Environment 问题后重新连接或重启应用，使 Runtime Maintenance 以新的运行时事实再次执行。重新进入必须能够绕过旧的失败结果，不能因相同 revision 永久复用失败状态。
_Avoid_: 把“请稍后重试”当成没有实际重新执行保证的恢复方案。

## Skill 操作与恢复

**未完成的 Skill 操作**：用户请求的 Skill 文件变更未能被 Skill Deck 安全收敛，相关文件状态需要检查。它描述用户可见的业务结果，不描述内部 restore 阶段。
_Avoid_: 自动恢复失败、手动恢复即可、Recovery 问题。

**受保护写入**：已经进入可能改变 Skill 目录、Agent 目录项或关联 lock 的 destructive write，并要求这些目标在同一操作单元内保持一致的 Skill 操作。Source 获取、Environment 连接、Runtime Maintenance、普通配置保存和临时 Payload 清理不属于受保护写入。
_Avoid_: 把所有本地文件操作、下载或后台清理都归入受保护写入。

**原子 Skill 操作**：针对一个 Skill 的一次受保护写入，主 Skill 目录、关联 Agent 目录项和由该操作拥有的 lock 变更必须作为一个整体收敛。它的结果可以是成功、未完成或需要检查，但不会把单个 Skill 的中间结果当成可独立提交的部分。
_Avoid_: 把单个 Skill 的中间结果称为 partial，或让批次总状态覆盖这个操作单元的真实结果。

**批次 partial（批次部分完成）**：由多个 Skill 或 Project 操作单元组成的批次中，部分单元成功、失败、跳过或未运行的整体结果。partial 只描述批次聚合，不改变每个操作单元各自的结果和后续处理方式；其中的 `RecoveryRequired` unit 仍保持独立状态，不转成普通失败或 retry。
_Avoid_: 把单个 Skill 的原子失败称为 partial，或用 partial 代替逐单元结果。

**Recovery Resource（恢复资源）**：Skill Deck 在 destructive write 未能安全完成时持久保留的受控恢复证据，用于重新评估相关文件和 lock 是否一致。它归属于产生它的 Environment 和 storage owner；产品只承诺保留现场、提供受控查看与重新评估，并在状态一致后清理，不承诺自动或手动恢复一定成功，也不续跑旧任务。
_Avoid_: Recovery 问题、需要恢复处理、把 Environment 或 Runtime Maintenance 异常称为 Recovery。

**恢复归属**：Recovery Resource 只能由产生它的 Environment 重新检查、打开和清理。Environment 不可用时保留资源，不跨 Environment 或 storage 搬运 backup 来完成恢复。
_Avoid_: 用另一个 Environment 代替原 owner 修复 Recovery Resource。

**恢复资源清理**：即使 Backend 已确认目标和 lock 一致，Recovery Resource 也要在用户明确确认后才清理。清理只移除 Skill Deck 自己拥有的 marker、backup 和其他恢复证据，不改变已经确认一致的目标文件。
_Avoid_: 对 Recovery Resource 做隐式删除、TTL 自动清理或把清理当成重新执行旧操作。

**恢复阻断范围**：Recovery Resource 只阻断当前操作单元或后续会触碰同一 physical target 的受保护写入；无关 Skill、项目和其他 Environment 可以继续工作。一个资源的问题不能升级成全局写入锁。
_Avoid_: 因单个 Recovery Resource 锁死整个应用或所有 Environment。

**Recovery Center（恢复中心）**：展示和处理持久化 Recovery Resource 的全局入口，包括尚需检查、Environment 暂不可用、记录无效以及已确认一致但尚未清理的资源；每个资源独立处理，缺失资源在刷新后移除。Environment 连接、Runtime Maintenance、Source 获取等短暂异常留在各自的业务反馈中，不作为 Recovery Center 的资源。
_Avoid_: 把所有需要用户注意的状态都汇总成 Recovery Center 项目。

## 应用边界

**UI 分区**：Main 和 Install Wizard 是同一 Skill Deck 应用中的不同交互窗口，不是彼此隔离的 trust domain。它们可以共享业务 command capability，窗口差异只在确有必要的交互和 lifecycle 约束中体现。
_Avoid_: 把两个 WebView 的业务权限矩阵当成两个独立后端或安全域。

**应用级 command capability**：对同一应用业务 command 的统一调用许可，受 Tauri default-deny、CSP、sanitizer 和实际使用的 plugin resource scope 约束。它不替代 Backend 对 typed identity、revision、路径和 ownership 的授权校验。
_Avoid_: 把 window ACL 当成本地文件 ACL 或业务 authorization。

**Backend authority（Backend 权威校验）**：Backend 对业务 identity、当前事实、Environment、physical target 和 ownership 重新解析并决定是否允许操作的边界。Frontend 或窗口 capability 不能绕过这层校验。
_Avoid_: 让前端传入的 display path 或窗口标签成为最终授权依据。

**Cooldown authority（冷却权威）**：Backend 对某项操作当前是否允许再次执行的唯一判断。Frontend 只展示当前结果并在用户重新请求时再次询问，不维护精确倒计时、自动解锁或第二套 backoff。
_Avoid_: 把 Frontend 本地计时归零当成操作一定可执行的事实。

## Eve 与 CLI 兼容

**Eve placement（Eve 布局）**：Eve Project Context 中 Skill 在 root agent 或具名 subagent 目录中的接入位置。明确的 `[""]` 表示 root，`["name"]` 表示具名 subagent；当 entry 已由当前 facts 证明属于 Eve 时，缺失字段按 CLI 兼容规则读取为 legacy root，但不自动写回；无法确认 Eve 身份时，缺失字段仍保持 unknown。没有 Eve target 时不产生 Eve placement，也不把空数组定义为共享的“无目标”编码。placement 是外部 lock 可见的兼容事实，安装、更新和移除必须保留它，而不是把 Eve 当成一个只能写入 root 的特殊 Agent。
_Avoid_: 用当前目录观察结果覆盖 lock 中仍然有效的 placement，把 Eve placement 隐藏成普通 Agent ID，或用 `[]` 发明 Skill Deck 专属的无目标语义。

**vendored CLI 兼容基线**：仓库内固定版本的 `vercel-skills` CLI 对共享 lock、placement 和目录语义的外部契约。Skill Deck 可以在此基线上增加自己的 workflow 和安全校验，但不复制 CLI 的内部实现，也不另造一套互不兼容的 Eve 格式。
_Avoid_: 以 CLI 当前未覆盖的内部 helper 作为共享契约，或把 Skill Deck 的扩展字段写回成 CLI 无法理解的格式。
