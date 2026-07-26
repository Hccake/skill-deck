# 产品设计

## 产品定位

Skill Deck 是管理 AI coding agent Skills 的本地桌面应用。用户可以在 Windows、macOS 和 Linux 上浏览、安装、阅读、更新、复制和移除 Skill，并管理 Skill 对 Agent 的适配关系。安装了 WSL 且存在可用发行版的 Windows 用户，还可以在 Host 与多个 WSL 发行版之间切换工作环境。

Skill Deck 以 [skills CLI](https://github.com/vercel-labs/skills) 的共享格式和基础语义为兼容基线，同时提供桌面端工作流、项目级更新检测、批量操作、Custom Agent、跨 Environment 操作、错误恢复和应用内更新。Skill Deck 不依赖正在运行的 skills CLI，也不把 CLI 的能力范围作为产品上限。

应用不使用服务器保存用户的 Agent、Project 或 Skill 配置。业务数据保留在本机和相应 Environment 中，软件更新通过 GitHub Releases 分发。

## 用户心智模型

```mermaid
flowchart LR
    Environment["Environment\nHost 或 WSL"] --> Context["Context\nGlobal 或 Project"]
    Context --> Skills["已安装 Skills"]
    Registry["Built-in + Custom Agents"] --> Resolution["当前 Context 中的 Agent 解析"]
    Resolution --> Skills
    Source["GitHub、Git、本地或 well-known 来源"] --> Wizard["安装向导"]
    Wizard --> Skills
```

- **Environment** 表示操作在哪里执行。所有平台都有 Host；Windows 可以额外选择 WSL 发行版。
- **Context** 表示当前管理范围。Global 面向当前 Environment 的用户级目录，Project 面向已登记项目。
- **Skill** 是包含 `SKILL.md` 以及可选 scripts、references、assets 和其他文件的完整目录。
- **Agent** 是读取或接收 Skill 的 AI coding agent。Built-in 与 Custom Agent 在 Skill 工作流中使用相同行为。
- **Source** 是获取 Skill 的位置。一次来源解析可以发现一个或多个可安装 Skill。

Agent 规则见[Agent](./agents.md)，Environment 与 Context 规则见[Environment 与 Context](./environments-and-contexts.md)，Skill 的状态变化见[Skill 生命周期](./skill-lifecycle.md)。

## 应用结构

```mermaid
flowchart TD
    App["Skill Deck"] --> Main["主窗口"]
    App --> Wizard["独立安装向导"]

    Main --> Skills["Skills"]
    Main --> Discover["Discover"]
    Main --> Settings["Settings"]
    Main --> Recovery["按需出现的恢复资源入口"]
    Main --> Mutation["写操作状态栏"]

    Skills --> Context["Environment 与 Global/Project"]
    Skills --> Detail["内容、更新、Agent、复制、移除"]
    Discover --> Browse["榜单、搜索、详情和安全信息"]
    Browse --> Wizard
    Settings --> General["主题与语言"]
    Settings --> Agents["Agent definitions"]
    Settings --> Defaults["安装偏好"]
    Settings --> Git["Git"]
    Settings --> Projects["Projects"]
    Settings --> About["关于与应用更新"]
```

主窗口提供 `Skills`、`Discover` 和 `Settings` 三个一级入口。安装向导使用独立窗口，避免安装过程挤占主工作区。当前写操作使用固定状态栏；存在持久化 Recovery Resource 时，界面才显示全局恢复入口，正常状态不占用额外空间。Environment 和 Runtime Maintenance 的短暂异常留在各自的业务区域。

## Skills 工作台

Skills 工作台由 Context 侧栏、Skill 列表和详情区组成。

### Context 侧栏

- 侧栏显示当前 Environment 的 Global Context 和已登记项目。
- 应用每次启动都进入 Host 的 Global Context，不恢复上次选择的 WSL Environment，也不会仅因启动应用而连接或启动 WSL 发行版。
- Windows 存在可用 WSL 发行版时，用户可以切换 Environment。只有 Host 时不增加无意义的选择控件。
- 应用重新获得焦点时，只有最近一次成功获取的 WSL 发行版列表已超过 30 秒有效期，并且距离上一次获取结束也已达到 30 秒，才会再次获取。列表在有效期内可能短暂滞后；一次获取即使失败，也会重新开始计算间隔。Discovery 不提供独立重试按钮。
- 已有发行版列表时，后续获取在后台进行，界面继续显示当前列表，不切换为整块加载状态。获取成功后一次性更新列表；获取失败时保留当前列表并说明本次发现失败，不打断当前 Context。
- 切换到其他 Environment 成功后立即进入目标 Environment 的 Global Context；项目注册信息随后独立加载，加载失败不会回滚 Environment。当前 Environment 重新连接成功后保留现有 Context，并重新加载项目列表。连接失败时保留原 Context，只提示本次失败；当前 Environment 可以直接重试，其他 Environment 通过再次选择自然重试。切换期间提供不改变工作区布局的进度反馈。
- 用户可以添加项目、移除项目或打开项目配置资源。Project 记录按 Environment 隔离。

### Skill 列表与详情

- 列表按当前 Context 展示已安装 Skill，并提供搜索、刷新、更新检查和新增入口。
- 当前 Environment 无法读取 Project Context 时，列表使用独立的中性不可用状态，不同时展示更新结论或管理命令。用户可以检查项目位置，或自行切换到已经添加该项目的 Environment。
- 选中 Skill 后，界面切换为列表与详情分栏。详情展示 Skill metadata、`SKILL.md` 内容、安装位置、关联 Agent 和更新状态。
- Skill card 和详情只展示已检测到并且能够实际读取当前 Skill 的关联 Agent。读取通用 Skill 目录的 Agent 要求该目录中存在当前 Skill；其他 Agent 要求其自身的 Skill 目录中已经存在有效链接或文件。
- 可用操作包括检查更新、执行更新、管理 Agent、复制到项目、修复来源和移除。
- Skill 内容读取失败不会清空其他列表状态，界面保留重试入口。
- 写操作进行时，会冲突的入口统一禁用，避免用户同时发起第二个 mutation。

## Discover

Discover 用于浏览远端可安装 Skill。

- 用户可以查看 popular、trending 和 hot 列表，也可以按关键词搜索。
- 详情区展示简介、README 或 `SKILL.md` 内容、来源信息、安装位置和可用的安全审计信息。
- 已在 Global 或某个 Project 安装的 Skill 会显示对应位置。
- 点击安装会打开独立向导，并预填来源和 Skill 名称。
- 安全审计属于辅助信号。审计服务暂时不可用时，用户仍可继续查看来源和决定是否安装。

## 安装工作流

```mermaid
flowchart LR
    Scope["选择 Context"] --> Source["输入或解析来源"]
    Source --> Select["选择 Skills"]
    Select --> Agents["选择 Agent 与安装模式"]
    Agents --> Preview["风险、覆盖与执行预览"]
    Preview --> Execute["执行安装"]
    Execute --> Result["逐项结果"]
    Result -->|可重试| Preview
    Result -->|相关文件需要检查| Recovery["Recovery Resource"]
```

1. 用户选择 Global 或 Project Context。来自 Discover 或 Skill 维护入口的向导会携带相应 Context。
2. 用户输入 GitHub、Git、本地路径、well-known 地址，或者粘贴受支持的 `skills add` 命令。
3. 应用获取来源并列出其中可安装的 Skill。用户可以选择一个或多个 Skill。
4. 用户选择 Agent 目标以及 `symlink` 或 `copy`。读取通用 Skill 目录的 Agent 不要求重复选择。
   Project 被识别为 Eve 项目时，用户还可以选择 Eve root agent 或已经发现的 subagent 作为具体目标。
5. 确认页展示来源风险、目标、覆盖项和实际执行范围。受保护来源需要用户明确确认风险。
6. 确认页先完成安装准备：固定 payload、生成执行预览，并独立加载 best-effort 安全审计。准备失败时停留在确认页，显示失败阶段和可本地化的原因；安装按钮不会被错误放行。预览已经过期时，界面要求重新准备并确认。
7. 完成页按 Skill 和目标项目展示成功、失败、取消、跳过、未运行或需要恢复。每项同时显示目标 Environment 与 Global/Project 范围，失败项可以按需展开错误详情，可重试项可以保留原意图重新执行。

安装的技术语义见[Skill 生命周期](./skill-lifecycle.md)，预览、一致性和恢复规则见[执行与恢复](./execution-and-recovery.md)。

## 更新与来源维护

- 更新检查按 Context 执行，可以覆盖单个 Skill 或当前范围内支持检查的 Skill。
- 更新能力与本次检查结果分开表达。来源暂时不可达不会被显示成“已是最新版本”。
- 列表加载、Context 切换和 Project 切换发起自动检查时，十五分钟内复用仍然 fresh 的结果；只有结果缺失或过期且当前不处于限流或退避状态时才访问远端，界面导航本身不会重复请求同一来源。
- 用户主动检查单个 Skill 或全部 Skill 时绕过结果有效期并请求最新状态；同一来源正在进行的检查会合并，远端限流或网络退避期间不会立即重复请求。检查成功后刷新结果及其有效期，检查失败时保留上次结果并明确标记其已经过期或本次检查失败。
- 检查失败在 Section 中按受影响 Skill 数量汇总，并在对应 Skill 上显示统一的“检查失败”状态；具体原因通过可聚焦的 Tooltip 查看，不在列表中重复展示 Source 或 Provider 诊断。当前 Section 的全部可检查 Skill 都处于 Backend 判定的冷却窗口时，检查操作暂时不可用；Frontend 不维护精确倒计时，也不承诺自动解锁或自动重试。
- 点击单个或全部更新后立即打开确认界面，并在稳定的内容区域加载本地更新计划；这个阶段不获取安装 payload，也不触发 clone。单个更新保持简洁，批量更新按 Source 组织 Skill，避免把来源与 Skill 层级混在一起。
- 更新计划展示本次实际会处理的主 Skill、已有 Adapter 目标、自动同步 copy 和冲突 copy，不展示仅具备读取能力但没有实际目录项的潜在 Agent。冲突 copy 默认保留，只有明确勾选后才覆盖。
- 遮罩点击不会关闭更新界面。计划与结果阶段可以通过 `Escape` 或关闭按钮退出，底部取消只用于放弃尚未开始的更新；用户确认后界面立即进入执行状态，并在当前操作仍支持取消时提供含义明确的“停止更新”。
- 执行中持续展示当前阶段和正在处理的 Skill，批量更新同时展示已处理数量和整体进度。执行期间不允许通过 `Escape` 或关闭按钮离开；停止请求完成后仍进入结果页，保留已经完成和未执行项的真实状态。
- 完成后先展示整体结果摘要，再展示失败、partial、warning 和未完成操作等需要关注的 Skill。部分成功不会抹掉失败项的重试入口。
- 支持远端证据的 Global 与 Project Skill 可以利用对应来源 hash 提前判断远端变化，但实际更新仍然是根据已保存来源重新安装当前内容。
- 来源记录不完整、上游路径已经删除或普通检查无法继续时，界面提供删除、保留或修复来源等维护动作。
- 修复来源使用独立的 Repair Source 弹窗。弹窗按验证、准备、安装阶段展示当前状态；执行中的遮罩、Escape 和右上角关闭按钮都不能中断操作，用户需要使用明确的“停止修复”按钮。成功后关闭，部分成功、失败或停止会保留弹窗和结果，允许用户继续处理或重试，不会在后台猜测新的远端目录。

## Agent 管理

Skill 工作流不会把 Built-in 与 Custom Agent 分成两套操作。

- Agent 选择区分“可直接使用”和“需要单独接入”，并允许用户保留已经存在的额外 Agent 目录项。安装向导、`Manage Agents` 和安装偏好使用一致的选择语义。
- 多个 Agent 使用同一实际目录时作为一组选择，避免重复操作或误删仍被其他 Agent 使用的内容。具体分组规则见[Agent](./agents.md#目标选择与目录分组)。
- 保存前先生成当前 Agent Registry、Context 和目录事实的 preview；保存阶段会再次 preview 并重新校验。单个 Skill 的关联 Agent、主目录和 lock 作为一个原子 unit，只会全部成功、失败或在无法确认一致时保留 Recovery Resource，不产生 partial。保存成功才关闭弹窗，失败会保留当前选择，stale 会刷新 preview 并要求用户复核后再保存。
- 安装向导遇到尚未定义的 Agent 时，可以打开 `Settings > Agents` 创建 definition；保存后返回原安装步骤。取消或失败不会丢失当前安装意图。
- Agent 定义的创建、编辑、复制和删除只出现在 Settings。删除 Custom Agent 前会展示受影响路径、默认项引用和管理能力风险，并要求输入 Agent ID 二次确认。
- 删除 Custom Agent 不会删除现有 Skill 文件。后续默认项清理失败时，界面会显示警告，但已经确认的删除仍然有效。

## 复制到项目

- 复制操作以一个 Project Context 中已安装的 Skill 为来源，并选择一个目标 Environment。Global Skill 不提供此入口。
- 一个批次可以选择该目标 Environment 中的多个项目，但不能同时混选不同 Environment。
- 目标 Environment 必须拥有目标项目路径；来源 Environment 可以不同，应用通过受控内容传递完成跨 Environment copy。
- 目标项目的读取失败会显示为错误，不会被当成“尚未安装”。
- 应用在写入前检查同一物理项目、路径重叠、存储能力和覆盖风险。
- 多项目结果相互独立，可以出现部分成功。已成功的项目从下一次重试范围中排除，普通失败项目保留清晰的重试信息；`RecoveryRequired` 项目保留独立的 Recovery 入口，不加入普通 retry；只有全部成功时才自动关闭复制弹窗。单目标 Copy 不显示为 partial。
- 项目位于其他 storage owner 时，当前 Environment 只能读取、提示并引导切换，不能直接执行受保护写入；切换到 owner Environment 后，目标项目才可进入 copy、install、update、remove 或 Manage Agents。
- 复制成功后，Remote、Git 和 Well-known 来源的目标 Project 保留可解释的来源、ref、Skill path 和更新基线，但不依赖来源 Environment 或来源 Project 继续可用；后续更新直接在目标 storage owner Environment 按目标 Project 自己的 lock 重新获取来源。Local 来源只保留路径和内容基线作为 provenance，明确显示没有自动更新能力。
- Copy 不改变源 Skill 原有的更新能力；Local source 沿用列表中已有的“不可更新”状态，不增加 copy 专用提示或确认步骤。

## 移除与 Recovery Resource

- 从 Skill Card 发起移除时，弹窗展示通用 Skill 目录和当前检测到的全部 Agent 接入。用户确认一次后，应用删除这些位置，并同步清理由 Skill Deck 管理的本地记录。
- Agent 接入在界面中只区分软连接和副本，不展示平台相关的底层链接类型。多个 Agent 使用同一实际目录时，应用合并展示并只处理一次。
- Skill 删除不提供部分 Agent 选择。用户需要保留 Skill、只调整部分 Agent 时，通过 Manage Agents 完成。
- 受保护写入未能安全收敛、相关文件和 lock 状态需要检查时，应用会保留 Recovery Resource，并显示按需出现的全局入口。
- Recovery 详情在应用启动和 Environment 状态变化后刷新，也允许用户在状态 Dialog 中主动刷新。
- 用户可以打开 Backend 确认过的恢复资源并刷新状态；只有 Backend 证明文件与 lock 已一致后，用户才能明确确认清理。产品不承诺自动或手动恢复一定成功。
- Recovery 不续跑已经中断的旧计划，也不允许前端提交任意 backup 路径执行删除。

## Settings

### 常规

用户可以切换浅色/深色主题以及简体中文/English。主题和语言应用于主窗口与安装向导。

### Agents

- 页面用于查看 Agent definition 的 Global/Project 读取能力和 Detection，并管理 Custom Agent；它不管理 Agent 本体，也不创建 Skill。
- Built-in definitions 只读，Custom definitions 可以创建、编辑、复制和删除。两种来源使用相同的 Agent 领域模型。
- 当前 Environment 是路径解析和 Detection 的查看条件。切换后重新解析同一 Registry，不为每个 Environment 维护重复 definition。
- 页面集中说明通用 Skill 目录，并为每个 Agent 表达是否读取通用目录、此 Agent 的 Skill 目录或两者。Project 只展示相对规则，不借用任意 Project 推断绝对路径。
- Runtime 解析失败时保留 definitions，明确显示失败并提供重试；Custom definition storage 异常不会伪装成空列表。
- 删除 Custom definition 不删除已有 Skill 文件，并在确认前说明默认项引用和后续管理影响。完整领域规则见[Agent](./agents.md)。

### 安装偏好

用户可以分别设置当前 Environment 中 Global 和 Project 安装的默认 Agent。界面使用与安装向导一致的目标分组，保存统一的 Agent ID；Detection 暂时不可用不会自动删除用户已经保存的 Custom Agent 默认项。

### Git

用户可以设置 Git clone timeout，也可以恢复默认值。保存失败时界面保留原配置并显示错误。

### Projects

用户可以切换 Environment，并管理该 Environment 的 Project 列表。Environment 不可用或正在切换时，新增和移除操作保持禁用。

### 关于与应用更新

- 关于页展示应用版本、项目链接和 skills CLI compatibility 信息。
- 应用保留标准本地日志，但不把诊断目录、最近记录复制或自由文本导出建设成产品能力。业务 Dialog 只根据 stable error code 和 parameters 提供可执行反馈，不展示 technical details，也不会自动上传日志。
- 应用可以检查 GitHub Release 更新，显示 release notes、下载进度和失败重试。
- 下载并安装完成后，用户可以在受保护的生命周期流程中重启应用。
- 应用启动时根据上次检查结果判断是否发起一次自动检查。成功检查使用正常间隔，失败检查使用较短的重试间隔；应用不会在进程内持续轮询。

## 通用状态与反馈

业务 Dialog 的遮罩点击不会关闭弹窗。尚未开始执行时，用户可以使用底部取消、Escape 或右上角关闭按钮；进入写操作后，Escape 和右上角关闭按钮会暂时失效，底部按钮只执行该业务定义的停止或取消语义。失败、partial 和 stale 默认留在原弹窗内，成功才自动关闭。

| 状态 | 产品行为 |
|---|---|
| Loading | 保留稳定布局并显示当前加载目标，不用空白界面替换已提交状态 |
| Empty | 说明当前 Context 没有内容，并提供与该页面相关的下一步操作 |
| Error | 显示失败对象，不把失败降级成“未安装”或“没有更新”；只有重新执行可能成功的流程才提供独立重试 |
| Partial | 明确区分成功、失败、跳过和未运行项，保留失败项后续操作 |
| Stale | 重新获取预览，要求用户复核已经变化的风险或目标 |
| Cancelled | 停止后续写入，保留已经完成 unit 的真实结果 |
| 操作未完成，相关文件需要检查 | 持续展示 Recovery Resource，不把它归类为普通可重试失败，也不把内部 restore 阶段作为用户可见原因 |

## 平台范围

| Desktop platform | Environment | 主要行为 |
|---|---|---|
| Windows | Host；安装 WSL 且发现发行版后可增加一个或多个 WSL distro | Host 使用 Windows filesystem 语义；WSL 使用 Linux/POSIX 语义并通过 `wsl.exe` 执行 |
| macOS | Host | 使用原生 POSIX 语义 |
| Linux | Host | 使用原生 POSIX 语义 |

Skill Deck 不提供 WSL 安装、创建、终止、注销或重启功能，也不向发行版部署常驻 helper。用户选择 WSL Environment 后，应用按需连接并读取该发行版的 Home、Project 和 Agent 状态；连接已存在但处于停止状态的发行版时，`wsl.exe` 可能在连接过程中按需启动它。WSL 不可用时，Host 功能继续工作。

Host 访问 WSL UNC、WSL 访问 Windows mounted path 都属于可观察的 cross-storage 场景。应用可以读取事实、显示风险并引导切换，但不会因为 backend 能够访问路径就允许跨 owner 的受保护写入；跨 Environment copy 仍须由目标 storage owner Environment 执行。

## 当前限制

- Skill 没有独立的“禁用”状态。用户通过安装、移除或调整 Agent 适配控制可用范围。
- Custom Agent definition 不提供远程 catalog、云同步、导入导出或自定义 icon。
- Settings 不汇总其他 Environment 的 Agent 状态。用户切换 Environment 后查看对应结果。
- 一次复制只选择一个目标 Environment。
- Recovery 只处理已识别的恢复资源，不恢复旧 mutation plan。
- 应用不提供 WSL 生命周期管理界面，也不支持 SSH、容器或远程主机作为 Environment。
