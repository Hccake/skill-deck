# `skills` CLI 兼容

## 兼容目标

Skill Deck 以 [`vercel-labs/skills`](https://github.com/vercel-labs/skills) 的共享目录、lock 格式和基础安装语义为兼容基线。桌面应用自行实现这些能力，运行时不调用 CLI，也不依赖 Node.js。

这里的“兼容”表示：

- CLI 和 Skill Deck 能够理解共同维护的通用 Skill 目录和 lock 字段；
- 对同一来源、`ref`、`skillPath` 和 Agent 目标，双方的安装结果能够互相解释；
- Skill Deck 写入时保留 CLI 已知字段和本次操作范围外的未知字段；
- CLI 写入导致 Skill Deck 扩展元数据缺失时，应用根据剩余信息调整可用能力；
- Skill Deck 可以在共享基线之外提供自定义 Agent、Environment、批量操作、远端更新检查和恢复资源。

兼容范围集中在共享目录、lock 字段和基础安装结果。两个产品分别维护自己的功能、运行时状态和扩展字段策略。

## 当前基线

维护者可以将用于核对的固定版本上游 CLI 源码放在本地 `vercel-skills/` 目录中，该目录不属于版本化源码。同步时同时确认该目录的 `package.json`、Git tag、commit 和实际源码。当前兼容基线是 `1.5.13`。

根项目通过精确固定的开发依赖 `skills: 1.5.13` 运行 CLI 互操作测试。该依赖用于开发验证，桌面应用运行时使用自身实现。测试在临时 Eve 项目中离线覆盖根 Agent、具名子 Agent、多个子 Agent、普通项目和更新后重新安装。Local 来源按 CLI 规则标记为不可更新；更新测试使用同机 `file://` Git 仓库提供可重新获取的来源。

同步上游版本时，维护者直接比较固定版本源码，再更新 Skill Deck 的内置 Agent 定义和兼容测试。本文记录当前基线与稳定兼容范围，版本迁移过程保留在提交历史中。

## 共享与扩展边界

| 领域 | 共享基线 | Skill Deck 扩展 |
|---|---|---|
| 来源 | 简写、Git/URL/Local/Well-known 解析，`ref` 与 Skill 子路径 | 按 Environment 获取、发现会话和风险信息 |
| Agent | 内置 Agent ID、路径、检测和基础目标语义 | 自定义 Agent、开放注册表、按 Context 选择默认目标、目录检查和物理归属 |
| 安装 | 通用 Skill 目录、Agent 目录项、link/copy 和重新安装语义 | 预览与执行、批量执行单元、跨 Environment 内容传递和恢复资源 |
| Global lock | CLI v3 共享字段与路径 | `defaultTargetAgents` 和无损局部写回 |
| Project lock | CLI v1 共享字段、排序和内容哈希 | `sourceUrl`、`remoteHash`、`pluginName` 和远端更新检查 |
| 更新 | 根据保存的来源与 `skillPath` 重新安装 | 更新能力判断、远端比较、批量计划和来源修复 |

CLI lock 保存双方共享的来源、Skill 和 Agent 投影。Environment 会话、内容快照引用、预览凭据、恢复标记和自定义 Agent 定义由 Skill Deck 的运行时与产品配置保存。

## 通用 Skill 目录

CLI 与 Skill Deck 共同使用以下目录：

| 范围 | 目录 |
|---|---|
| Global | `~/.agents/skills` |
| Project | `.agents/skills` |

本文将它们统称为“通用 Skill 目录”。Agent 是否读取通用 Skill 目录、是否还需要自身目录，以及目录检查如何形成关联 Agent，由[Agent](./agents.md)定义。Environment 中的 Home、项目和路径解析由[Environment 与 Context](./environments-and-contexts.md)定义。

## Global lock

Global lock 的标准位置遵循 CLI 规则：

- 设置 `XDG_STATE_HOME` 时使用 `$XDG_STATE_HOME/skills/.skill-lock.json`；
- 否则使用 `~/.agents/.skill-lock.json`。

当前格式版本为 `3`。每个 Skill 记录的共享字段包括：

| 字段 | 语义 |
|---|---|
| `source` | 规范化来源标识 |
| `sourceType` | GitHub、GitLab、Git、Local 或 Well-known 等来源类别 |
| `sourceUrl` | 能够保留原始定位信息的来源 URL |
| `ref` | branch 或 tag |
| `skillPath` | 仓库内指向 `SKILL.md` 的相对路径，保留磁盘实际大小写 |
| `skillFolderHash` | CLI 用于来源版本跟踪的可比较哈希；GitHub 使用 Skill 目录的 Git tree object ID，GitLab 和普通 Git 使用与 CLI 一致的内容哈希 |
| `installedAt` | 首次安装时间 |
| `updatedAt` | 最近更新时间 |
| `pluginName` | 能够识别时保存的 plugin 名称 |

Global lock 还包含两个 Agent 选择字段：

- `lastSelectedAgents` 是 CLI 最近一次选择记录。Skill Deck 只写当前有效默认目标中 CLI 能够理解的内置 Agent，不写自定义 Agent ID；
- `defaultTargetAgents` 是 Skill Deck 扩展，分别保存 Global 与 Project 的默认 Agent ID，可以包含内置和自定义 Agent。

缺少 `defaultTargetAgents` 时，安装向导按本次明确选择、当前范围的回退规则和内置初始推荐生成默认值。`lastSelectedAgents` 只提供 CLI 能够理解的内置 Agent 选择，自定义 Agent 偏好来自 Skill Deck 的扩展字段。

## Project lock

Project lock 位于 `<project>/skills-lock.json`，当前格式版本为 `1`。它适合提交到项目仓库，因此字段语义和写入顺序需要保持稳定。

| 字段 | 归属 | 语义 |
|---|---|---|
| `source` | CLI / Skill Deck | 规范化来源标识 |
| `ref` | CLI / Skill Deck | branch 或 tag |
| `sourceType` | CLI / Skill Deck | 来源类别 |
| `skillPath` | CLI / Skill Deck | 仓库内指向 `SKILL.md` 的相对路径，更新写回时规范化为目录 |
| `computedHash` | CLI / Skill Deck | Project Skill 当前目录的递归内容哈希 |
| `subagents` | CLI / Skill Deck | Eve 项目的落位信息；空字符串表示 root Agent，其他字符串表示 subagent 目录名 |
| `sourceUrl` | Skill Deck | 私有来源或原始来源的保真信息 |
| `remoteHash` | Skill Deck | 上游提供者可比较的修订号；GitHub 使用 Skill 目录的 tree object ID |
| `pluginName` | Skill Deck | 用于展示的 plugin 元数据 |

Eve 兼容遵循上游 CLI 的现有落位规则：

- 只安装到 Eve 根 Agent 时采用 CLI 的最小写入形式，可以省略 `subagents`；
- 安装到具名或多个子 Agent 时，写入明确的字符串数组；
- 普通项目省略 Eve 目标信息；
- 已确认属于 Eve 的旧记录缺少 `subagents` 时按根 Agent 读取，但不自动补写；后续受控写入根据用户当次选择更新；
- 只有已经确认的 Eve Context 才解释 Eve 目标，其他 Context 保持原记录，不推断目标；
- 外部已有的 `subagents: []` 原样保留，并在已经确认的 Eve Context 中按兼容规则解释；
- `subagents` 类型错误或数组包含非字符串值时，记录按 lock 损坏处理。

CLI 的 `computedHash` 按确定性顺序组合相对路径和文件内容并计算 SHA-256，同时排除 `.git`、`node_modules` 等不属于 Skill 内容的目录。Skill Deck 从已经固定的内容快照计算兼容哈希；快照身份和 `remoteHash` 使用各自字段。

Skill Deck 可以读取旧的 `<project>/.agents/.skill-lock.json`，并在受控写入时迁移到标准 Project lock。该路径属于 Skill Deck 的旧版读取兼容，上游 CLI 继续使用标准 Project lock。

## 各类哈希的职责

不同哈希分别解决以下问题：

| 哈希 | 负责问题 |
|---|---|
| Global `skillFolderHash` | CLI 的 Global 来源版本跟踪 |
| Project `computedHash` | 当前本地 Skill 目录内容是否变化 |
| Skill Deck `remoteHash` | 保存受支持来源的上游修订号；GitHub 当前使用 Skill 目录的 tree object ID |
| 内容快照哈希 | 标识一次来源获取后固定的完整目录内容 |
| 预览指纹 | 判断用户确认所依赖的运行时事实是否已经变化 |

内容快照哈希同时编码相对路径、目录项类型、大小、内容哈希和可执行权限，表示当前 Environment 实际能够保留的内容身份。因此，同样的文件内容在 Unix 与不提供 Unix 可执行位的 Native Windows 上可能得到不同的快照哈希。这个差异不进入 `computedHash`；后者只按稳定相对路径和文件字节计算，并通过仓库 `.gitattributes` 保持测试内容一致。

更新检查根据来源证据选择比较基线：

| 来源证据 | Global 比较基线 | Project 比较基线 | Project `remoteHash` 写入 |
|---|---|---|---|
| GitHub Skill tree object ID | `skillFolderHash` | `remoteHash` | 写入 tree object ID |
| GitLab 或普通 Git 的 CLI 兼容内容哈希 | `skillFolderHash` | `computedHash` | 不写入 |

Project 缺少 `remoteHash` 时，只要完整来源和 `skillPath` 仍然存在，就可以主动重新安装。GitHub 缺少 tree revision 时，检查结果为“无法比较”；GitLab 和普通 Git 可以将远端克隆得到的兼容内容哈希与 `computedHash` 比较。缺少 `skillPath` 时，来源修复负责重新定位上游 Skill。

`computedHash`、内容快照哈希和 `remoteHash` 分别保存本地内容、固定快照和上游修订信息。GitHub 来源使用选中 Skill 目录对应的 Git tree object ID；Host 与 WSL 都接受上游支持的 Git object ID 格式。Well-known 和缺少可靠 Git 修订号的 GitHub 来源返回“无法比较”。

## 无损写回

Skill Deck 使用无损 JSON 文档读取 Global 和 Project lock。写入时只替换当前业务用例负责的根字段或 Skill 记录，并从最新文档保留：

- 无关 Skill 记录；
- Skill 记录中的未知未来字段；
- 根对象中的未知未来字段；
- 其他进程在读取后写入、且不与本次负责字段冲突的内容。

Skill Deck 的无损写回机制保留扩展字段和未知字段。上游 CLI 当前按固定类型重建记录，不保证保留 Skill Deck 扩展字段或未来未知字段；更新某个 Project Skill 后，`sourceUrl`、`remoteHash` 或 `pluginName` 可能缺失。Skill Deck 根据剩余字段继续读取，将相关功能显示为不可用或待修复，同时保持 lock 可读，不补造已经丢失的字段值。

上游新增已知字段后，维护者需要同步 Rust DTO、业务含义、生成的 bindings 和兼容测试。未知字段保留机制负责在同步完成前保持原始数据。

## 来源发现与 Well-known

Skill Deck 与 CLI 对共享来源保持以下互操作约束：

- 在 URL 解析器吞掉 fragment 前提取 `#ref`；
- Skill 筛选条件与 `ref` 按同一顺序解析；
- SSH 和私有 Git 输入保留认证所需的原始表达；
- `skillPath` 始终能够精确定位来源中的原 Skill；
- 根目录、优先目录、plugin manifest 和递归回退保持相同的发现顺序；
- 根目录资格、Project lock 筛选、父目录遮蔽、同名去重和稳定顺序在 Host 与 WSL 中保持一致；
- `SKILL.md` 的大小写识别与磁盘实际路径保真分别处理；
- Well-known endpoint、旧索引路径安全和制品摘要与上游协议保持一致；
- 安装内容保留隐藏文件，版本控制、缓存和工具元数据按共享排除规则处理。

Skill Deck 可以增加获取缓存、进度、信任信息和安全审计展示，共享协议的路径与内容校验继续保持一致。

## Agent 兼容

内置 Agent 定义以上游固定版本源码中的注册表、路径、检测规则、旧行为和适配器为同步依据。Skill Deck 将这些定义放入统一的开放注册表，再通过自定义 Agent 补充上游尚未覆盖的工具。

CLI 中的 `universal` 或静态 Agent 列表服务于 CLI 自身的目标选择。Skill Deck 根据当前范围的 Agent 定义、检测结果和实际目录检查计算关联 Agent。

CLI Agent 选择字段保存上游能够理解的内置 Agent ID。Skill Deck 的自定义 Agent ID 保存在自身扩展字段中，并由 Skill Deck 管理相应目录项。双方通过通用 Skill 目录、内置 Agent 投影和共享 lock 字段互操作。

Eve 适配属于 CLI 兼容规则的一部分。Skill Deck 通过 Eve 目标表达根 Agent 和子 Agent 的安装位置，并按照 Project lock 的 `subagents` 契约读写。完整 Agent 规则见[Agent](./agents.md)。

## 安装与更新语义

CLI 与 Skill Deck 都根据保存的来源重新安装完整 Skill。Skill Deck 在此基础上增加批量预览、不可变内容快照、逐项结果和还原机制，最终目录与共享来源信息仍保持 CLI 可解释。

CLI 可以从 Local 路径安装，并把 `sourceType: "local"` 与内容哈希写入 Project lock；Local 记录保持不可更新状态。Skill Deck 的跨 Environment 复制沿用同一规则，目标保留 Local 来源凭据并标记为不可更新。可重新获取的来源在目标 Environment 直接获取，复制前后保持来源原有能力。

CLI v1.5.13 会按来源和 `ref` 合并多个 Skill 的获取与发现。Skill Deck 同样合并同一来源的获取和比较证据。用户确认更新后，Skill Deck 按来源、`ref` 和执行 Environment 获取一次内容，再为各个 Skill 建立独立执行单元。

Project 更新在存在 `skillPath` 时可以直接定位并重新安装。远端比较元数据用于提供更新检查结果，重新安装资格由来源和 `skillPath` 决定。

两者都会生成可独立使用的完整 Skill 目录。Skill Deck 在获取阶段固定经过安全检查的内容，符号链接目标需要位于来源内、真实存在且没有循环；安全边界见[执行与恢复](./execution-and-recovery.md#skill-内容快照)。

不同平台可以使用不同落盘能力：Windows 使用适合目录的链接方式，macOS、Linux 和 WSL 使用 POSIX 符号链接语义。

CLI v1.5.13 在符号链接创建失败时会退回复制，但仍返回 `mode: "symlink"` 和 `symlinkFailed: true`。Skill Deck 保持用户本次选择，链接创建失败时返回失败，用户重新预览后可以选择复制。双方的结果字段含义不同，需要分别按各自契约解释。

完整业务流程见[Skill 生命周期](./skill-lifecycle.md)，原子写入和冲突处理见[执行与恢复](./execution-and-recovery.md)。

## 上游同步流程

更新上游 CLI 基线时，维护者按以下顺序处理：

1. 在 `vercel-skills/` 中确认 package version、目标 tag、commit 和工作区状态；
2. 阅读真实源码差异，并按行为归类变化；
3. 检查 Agent 注册表、来源解析、Well-known、发现、安装、Global lock、Project lock 和更新命令；
4. 将共享变化同步到 Skill Deck，同时保留自定义 Agent、Environment、无损 lock、远端检查和恢复资源等扩展；
5. 更新内置 Agent 行为测试、lock 测试夹具与哈希向量、来源与发现测试，以及受影响的前端状态；
6. Rust 命令或类型变化时重新生成 bindings，并核对窗口 ACL；
7. 更新本文的基线和稳定差异，实施过程保留在提交历史中；
8. 运行[贡献指南](../CONTRIBUTING.md)规定的相关验证。

上游源码和版本信息共同构成同步依据。Agent 定义、lock、来源、发现、安装和更新变化都需要纳入比较。

## 同步检查表

| 检查面 | 需要确认的行为 |
|---|---|
| Agent 注册表 | ID、alias、Global/Project 路径、检测、旧行为和适配器 |
| 来源解析 | 简写、URL、SSH、`ref`、筛选条件、alias 和子路径 |
| 发现 | 根目录提前返回条件、优先目录、plugin 路径、递归回退、目录深度、遮蔽、lock 筛选、同名去重和精确 `skillPath` |
| Well-known | endpoint 顺序、格式、摘要、归档与路径安全 |
| 安装 | 通用 Skill 目录、Agent 目标、排除项、link/copy 和链接失败行为 |
| Global lock | 版本、路径、字段、默认目标投影和未知字段 |
| Project lock | 版本、字段、排序、哈希和记录替换 |
| 更新 | 重新安装定位、缺失元数据、上游删除和结果语义 |
| Skill Deck 扩展 | 降级后仍可解释，CLI 写入后不误报损坏 |
| 跨层契约 | Rust DTO、bindings、ACL、国际化和测试同步 |

## 兼容保证

1. 固定版本的上游源码和版本信息构成同步证据，文档与测试夹具提供补充说明和回归验证。
2. CLI 提供兼容基线，Skill Deck 可以在共享规则之外扩展桌面端能力。
3. 共享字段保持 CLI 含义，Skill Deck 扩展字段保持可选。
4. Skill Deck 写 lock 时保留本次操作范围外的字段和外部变化。
5. 上游 CLI 当前不保证保留 Skill Deck 扩展字段或未来未知字段。
6. `computedHash`、上游修订号、`remoteHash`、内容快照哈希和预览指纹分别承担各自职责。
7. 缺少更新检查元数据时，检查状态显示为“无法比较”。
8. 自定义 Agent ID 保存在 Skill Deck 扩展字段中，CLI Agent 选择字段只包含上游已知 ID。
9. 上游同步同时检查 Agent、来源、发现、安装、lock 和更新。
