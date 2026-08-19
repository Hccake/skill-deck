# `skills` CLI 参考与兼容

## Skill Deck 与 `skills` CLI 的关系

[`skills` CLI](https://github.com/vercel-labs/skills) 是由 `vercel-labs/skills` 独立维护的第三方 Skill 管理工具。Skill Deck 不调用该 CLI，也不依赖 Node.js 运行；来源获取、Agent 解析、文件写入和用户工作流都由 Skill Deck 自行实现。

Skill Deck 和 `skills` CLI 都会在通用 Skill 目录中读写 Skill，也会读写 lock 中含义相同的字段。因此，一个工具安装到通用目录中的 Skill 可以由另一个工具继续管理。兼容范围不包括完整的 lock 内容、交互流程和执行结果：使用第三方 CLI 更新 Skill 后，Skill Deck 的扩展字段可能丢失；两个工具对安装失败也有各自的处理方式。

Skill Deck 固定一个 `skills` CLI 版本作为兼容参考，以此确认两个工具共同使用的数据，并了解 Agent 生态的变化。第三方 CLI 的新功能只说明可能出现了新的用户需求，不会自动成为 Skill Deck 的产品需求；确定需要支持后，再根据桌面应用的使用方式重新设计。

## 当前参考版本

| 项目 | 当前值 |
|---|---|
| 参考仓库 | `vercel-labs/skills` |
| package 版本 | `1.5.23` |
| Git tag | `v1.5.23` |
| Git 提交 | `435076e78988e1e6ec40d00b0b1d76bdbbc5419a` |
| 开发依赖 | `skills: 1.5.23` |

仓库通过精确固定的开发依赖运行兼容测试，桌面应用不会加载这个依赖。Eve 项目可以把 Skill 安装到根 Agent 或具名子 Agent；当前测试会调用实际 CLI，检查版本，并在离线临时项目中覆盖这两类位置、同时选择多个位置、只选择其他 Agent，以及从本机 Git 来源更新后重新安装。

下述兼容规则以该版本为准。Agent 的读取位置和检测方式还需要以对应 Agent 的官方资料和实际行为为准，第三方 CLI 的配置不能单独作为 Agent 支持依据。

## 兼容范围

| 内容 | 两个工具共同使用的部分 | Skill Deck 独立实现的能力 |
|---|---|---|
| Skill 目录 | 通用 Skill 目录和完整 Skill 内容 | 内容快照、风险检查、恢复数据以及 Windows 与 WSL 之间的写入 |
| 来源 | 简写、Git URL、本地路径、通过约定地址发现（Well-known）、`ref` 和 Skill 子路径的兼容含义 | 获取进度、信任信息、缓存和界面反馈 |
| Agent | 当前参考版本已知的 Agent ID，以及写入 lock 的兼容选择信息 | 用户添加的 Agent 信息、检测结果、关联关系和安装选项 |
| 全局 lock | `skills` CLI v3 路径和共同字段 | 修改 lock 时无损保留未涉及的内容 |
| 项目 lock | `skills` CLI v1 字段、排序、内容哈希和 Eve 安装位置 | `sourceUrl`、`remoteHash`、`pluginName` 和旧路径迁移 |
| 安装与更新 | 来源、Skill 子路径、通用目录和兼容 lock 记录 | 预览、批量执行、取消、恢复和具体错误反馈 |

Skill Deck 的运行状态、预览信息、内容快照、恢复记录和用户添加的 Agent 信息不写入第三方 CLI 的数据格式。Skill Deck 只对两个工具都会读写的数据提供兼容保证。

## 通用 Skill 目录

[Agent Skills 格式规范](https://agentskills.io/specification)定义 `SKILL.md` 及 Skill 目录内部的组织方式。[Agent Skills 客户端接入指南](https://agentskills.io/client-implementation/adding-skills-support#where-to-scan)说明了生态中广泛采用的 `.agents/skills` 目录约定。Skill Deck 与 `skills` CLI 都识别以下位置：

| Skill 类型 | 目录 |
|---|---|
| 全局 Skill | `~/.agents/skills` |
| 项目 Skill | `.agents/skills` |

这些位置统称为通用 Skill 目录。具体 Agent 是否读取通用目录、是否还需要写入 Agent 专用 Skill 目录，由[Agent 模型](./agent-model.md)说明。用户目录、项目目录以及 Windows/WSL 位置切换后的路径解析规则见[Environment、Skill 位置与项目管理](./environments-and-projects.md)。

## 全局 lock

`skills` CLI 的全局 lock 使用以下路径，Skill Deck 读写相同位置：

- 设置 `XDG_STATE_HOME` 时使用 `$XDG_STATE_HOME/skills/.skill-lock.json`；
- 否则使用 `~/.agents/.skill-lock.json`。

当前格式版本为 `3`。

| 字段 | 定义来源 | 含义 |
|---|---|---|
| `source` | `skills` CLI | 规范化后的来源标识 |
| `sourceType` | `skills` CLI | GitHub、GitLab、Git、Local 或 Well-known 等来源类型 |
| `sourceUrl` | `skills` CLI | Git 等来源需要保留时记录原始来源 URL；Well-known 来源记录具体 Skill 的制品地址 |
| `sourceBaseUrl` | `skills` CLI v1.5.22 Well-known 字段 | 用于定位 Well-known 索引的输入地址 |
| `wellKnownDigest` | `skills` CLI v1.5.22 Well-known 字段 | 索引提供或根据旧版内容计算的 Skill 版本摘要 |
| `ref` | `skills` CLI | 分支或标签 |
| `skillPath` | `skills` CLI | 来源中指向 Skill 目录的相对路径，保留磁盘实际大小写 |
| `skillFolderHash` | `skills` CLI | 用于跟踪来源版本的哈希 |
| `installedAt` | `skills` CLI | 首次安装时间 |
| `updatedAt` | `skills` CLI | 最近更新时间 |
| `pluginName` | `skills` CLI | 能够识别时记录的 plugin 名称 |
| `lastSelectedAgents` | `skills` CLI | CLI 最近一次选择的 Agent ID |
| `defaultTargetAgents` | 旧版 Skill Deck | 当前版本不读取、不写入且不清理；重写全局 lock 时原样保留 |

Skill Deck 在用户确认安装目标后写入 `lastSelectedAgents`，内容只包含当前参考版本能够识别的内置 Agent ID。全局安装和项目安装共用当前 Environment 的全局字段；明确指定 Agent 的安装入口不会更新该字段。初始选择和失败处理规则见[Agent 模型](./agent-model.md#安装初始选择与最近选择)。

## 项目 lock 与 Eve 安装位置

项目 lock 位于 `<project>/skills-lock.json`，当前格式版本为 `1`。该文件通常会提交到项目仓库，因此 Skill Deck 保存后的字段含义和排序需要继续与 `skills` CLI 兼容。

| 字段 | 定义来源 | 含义 |
|---|---|---|
| `source` | `skills` CLI | 规范化后的来源标识 |
| `ref` | `skills` CLI | 分支或标签 |
| `sourceType` | `skills` CLI | 来源类型 |
| `skillPath` | `skills` CLI | 来源中指向 Skill 目录的相对路径 |
| `computedHash` | `skills` CLI | 当前项目 Skill 目录的内容哈希 |
| `subagents` | `skills` CLI | Eve 根 Agent 或具名子 Agent 的安装位置 |
| `sourceUrl` | Skill Deck 扩展；`skills` CLI v1.5.22 Well-known 字段 | Git 等来源使用完整的原始来源地址；Well-known 来源使用能够重新定位索引的输入地址 |
| `wellKnownDigest` | `skills` CLI v1.5.22 Well-known 字段 | 索引提供或根据旧版内容计算的 Skill 版本摘要 |
| `remoteHash` | Skill Deck 扩展 | 来源能够提供的远端版本标识 |
| `pluginName` | Skill Deck 扩展 | 用于界面展示的 plugin 信息 |

Eve 项目可以把 Skill 安装到根 Agent 或具名子 Agent。当前参考版本使用 `subagents` 记录这些位置：

- 只安装到根 Agent 时可以省略 `subagents`；
- 同时安装到多个位置时使用字符串数组，空字符串表示根 Agent，其他值表示子 Agent 目录名；
- 普通项目不写 Eve 安装位置，也不解释现有的 `subagents`；
- 已确认属于 Eve 的旧记录缺少 `subagents` 时按根 Agent 读取，下次由 Skill Deck 修改该记录时再根据用户选择更新；
- 外部已有的 `subagents: []` 原样保留，在已确认的 Eve 项目中按照当前参考版本的规则读取；字段类型错误或数组中出现非字符串值时，按 lock 损坏处理。

Eve 的用户选择和目标模型见[Agent 模型](./agent-model.md#eve-专用安装目标)。

`computedHash` 按确定顺序组合相对路径和文件内容并计算 SHA-256，同时排除 `.git`、`node_modules` 等不属于 Skill 内容的目录。Skill Deck 从本次已经固定的完整内容计算同样的哈希。

项目 lock 中的 Local 来源尽可能保存相对于项目根目录的路径，并统一使用 `/` 分隔符。读取时根据当前项目根目录解析，因此项目整体移动后仍可定位同一相对来源；Windows 来源与项目不在同一盘符或来源使用 UNC 地址时保留绝对路径。

Skill Deck 仍可读取旧的 `<project>/.agents/.skill-lock.json`，并在下次修改该项目 lock 时迁移到当前路径。旧路径只属于 Skill Deck 的读取兼容，第三方 CLI 使用 `<project>/skills-lock.json`。

## 版本字段的用途

| 字段 | 解决的问题 |
|---|---|
| 全局 `skillFolderHash` | 保存 `skills` CLI 用于跟踪来源版本的值；GitHub 来源使用 Skill 目录的 Git tree object ID，GitLab 和普通 Git 使用兼容内容哈希 |
| 项目 `computedHash` | 判断当前项目中的 Skill 内容是否变化 |
| Skill Deck `remoteHash` | 保存来源能够提供的远端版本标识；GitHub 来源使用 Skill 目录的 tree object ID |
| `wellKnownDigest` | 保存 Well-known 索引提供或根据旧版内容计算的版本摘要 |

这些字段不能互相替代。`computedHash` 描述当前本地内容，`skillFolderHash`、`remoteHash` 和 `wellKnownDigest` 描述安装时保存的来源版本信息。远端版本如何获取、缺少比较信息时显示什么状态，由[更新检查](./update-checking.md)说明。

## 修改 lock 文件后的数据保留情况

### Skill Deck 修改 lock 文件

Skill Deck 修改全局或项目 lock 时，只替换本次操作负责的根字段或 Skill 记录。保存结果会保留：

- 其他 Skill 记录；
- Skill 记录中的未知字段；
- 根对象中的未知字段；
- 其他进程在本次操作期间写入、且不与本次修改冲突的内容。

尚未纳入兼容范围的字段会作为未知内容继续保留。如果其他进程修改了本次操作负责的同一字段或 Skill 记录，Skill Deck 会报告冲突，不会覆盖对方的修改。多个进程同时修改 lock 文件时的完整处理规则见[执行与恢复](./execution-and-recovery.md#原子写入与-lock-提交)。

来源类型发生变化时，Skill Deck 会清除原来源已经失效的已知字段，并继续保留未知字段。Skill 原始名称是新记录的 lock 键，安全化后的名称只用于磁盘目录；读取旧记录时，应用先精确匹配原始名称，再兼容与安装目录同名的旧安全化键。其他安全化后相同的名称不会被猜测为当前 Skill。后续正常安装、更新、复制、移除或管理 Agent 的事务会完成必要的键迁移，不提供独立迁移页面。

### 使用 `skills` CLI 修改 lock 文件

当前参考版本的 `skills` CLI 会根据自身支持的字段重新生成记录，不保证保留 Skill Deck 扩展字段或未来新增的未知字段。使用 CLI 更新项目 Skill 后，`sourceUrl`、`remoteHash` 或 `pluginName` 可能丢失。Skill Deck 会继续读取其余兼容字段；依赖缺失字段的功能会显示为不可用或需要修复，应用不会猜测或编造已经丢失的值。

## 来源解析与发现

为了让两个工具能够理解相同的来源和安装记录，Skill Deck 保持以下兼容行为：

- 在解析 Git URL 时先提取 `#ref`，并按相同顺序处理 Skill 筛选条件和 `ref`；
- SSH 和私有 Git 输入保留认证所需的原始写法；
- scoped Well-known 地址只读取对应 scope 的 catalog；精确选择命名的 internal Skill 时只放行这些名称，wildcard 仍只选择公开 Skill；
- `skillPath` 能够精确定位来源中的原 Skill，并保留磁盘实际大小写；
- 根目录、优先目录、plugin manifest 和递归查找使用兼容的发现顺序；
- 项目 lock 筛选、父目录遮蔽、同名去重和结果排序在应用所在系统与 WSL 中保持一致；
- Well-known 地址、旧版索引、制品摘要和归档路径遵循当前参考版本的兼容规则；
- 安装内容保留隐藏文件，并按兼容规则排除版本控制、缓存和工具生成的数据。

Skill Deck 可以自行增加获取缓存、进度、信任信息和安全检查，只要最终识别的 Skill、相对路径和兼容 lock 数据保持相同含义。

## 参考 CLI 中的 Agent 信息和新功能

`skills` CLI 的 Agent 注册表可以帮助发现新的 Agent、读取位置和适配方式。Skill Deck 更新随应用提供的 Agent 信息前，还要核对对应 Agent 的官方资料和实际行为，再把确认后的内容纳入自身注册表。

CLI 的 Agent 选择字段只保存当前参考版本能够理解的 Agent ID。用户添加的 Agent ID 保存在 Skill Deck 扩展字段中。检测结果、关联关系、安装位置分组和界面筛选由 Skill Deck 自行计算，完整规则见[Agent 模型](./agent-model.md)。

第三方 CLI 推出新功能后，先确认它解决的用户问题、适用人群以及是否与 Skill Deck 的使用场景相同。确定需要支持后，再结合桌面应用如何展示状态、批量处理、请求确认、报告错误，以及如何支持取消、恢复和无障碍操作，重新设计具体方案。

## 两个工具在安装和更新时的差异

两个工具都会根据保存的来源重新安装完整 Skill。Skill Deck 安装到通用 Skill 目录中的 Skill，以及写入 lock 的兼容来源信息，可以继续由 CLI 读取；预览、批量执行、取消、恢复和错误反馈则属于 Skill Deck 自身的工作流。

第三方 CLI 可以从本地路径安装，并在项目 lock 中保存 `sourceType: "local"`；这类记录没有远端更新能力。Skill Deck 在复制本地来源 Skill 时保持相同含义。没有有效来源记录的 Skill 只复制实际内容，不伪造来源类型；具体规则见[Skill 生命周期](./skill-lifecycle.md#复制到项目)。

两个工具对符号链接失败的处理不同。当前参考版本的 CLI 会自动改用复制，但仍在结果中保留原始链接模式，并标记 `symlinkFailed: true`。Skill Deck 会终止本次操作，用户重新预览后可以改选复制。读取相关结果字段时，需要按照实际执行操作的工具进行解释。

完整安装、更新和复制流程见[Skill 生命周期](./skill-lifecycle.md)，内容安全、原子写入和恢复规则见[执行与恢复](./execution-and-recovery.md)。

## 更新参考版本

更新参考版本时使用同一份检查清单：

| 检查项 | 需要完成的工作 |
|---|---|
| 参考版本 | 确认 package 版本、Git 标签、Git 提交和源码工作区状态，阅读两个固定版本之间的实际源码差异 |
| 仓库内版本信息 | 同步 `package.json`、lockfile、`src/constants.ts`、CLI 兼容测试中的版本断言、About 页面测试以及本节版本表 |
| 兼容数据 | 检查通用 Skill 目录、全局 lock、项目 lock、字段类型、排序、哈希、Eve `subagents` 和未知字段行为；新增字段时，确认两个工具是否都会读写，再决定是否补充数据类型、业务规则和兼容测试 |
| 来源与发现 | 检查简写、URL、SSH、`ref`、Skill 子路径、Well-known、plugin manifest、发现顺序和排除项 |
| Agent 信息 | 比较 Agent ID、别名、读取位置和检测方式，并根据 Agent 官方资料与实际行为复核 |
| 安装与更新 | 检查来源重新获取、符号链接与复制、失败结果、缺失元数据和 CLI 修改 lock 后的字段保留情况 |
| 新功能 | 区分数据兼容变化和产品功能变化；产品功能按 Skill Deck 的用户场景重新评估 |
| 跨层同步 | 公共命令或类型变化时同步 Rust、生成的前后端类型绑定、窗口权限、国际化文案和相关测试 |
| 验证 | 运行 CLI 兼容测试、受影响的 Rust 与前端测试，以及[贡献指南](../CONTRIBUTING.md)规定的完整检查 |
