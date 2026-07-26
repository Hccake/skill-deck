# skills CLI 兼容

## 兼容目标

Skill Deck 以 [`vercel-labs/skills`](https://github.com/vercel-labs/skills) 的共享格式和基础安装语义为兼容基线，但不是 CLI 的图形包装层。已发布桌面应用不调用 `skills` binary，也不要求用户安装 Node.js。

兼容的含义是：

- CLI 和 Skill Deck 能够理解共同维护的通用 Skill 目录与 lock 字段；
- 对同一 source、ref、`skillPath` 和 Agent 目标，安装或重新安装结果可以互相解释；
- Skill Deck 写入时不破坏 CLI 已知字段，也不丢失自己不拥有的未知字段；
- CLI 写入导致 Skill Deck 增强 metadata 缺失时，应用以可解释方式降级；
- Skill Deck 可以在兼容基线之上提供 Custom Agent、Environment、批量操作、远端更新检测和 Recovery。

“兼容”不表示两个产品拥有完全相同的功能、状态或无损写回能力。

## 当前基线

仓库根目录中 gitignored 的 `vercel-skills/` 是上游核对来源。当前基线由该目录的 `package.json`、Git tag/commit 和真实源码共同确认，当前版本为 `1.5.13`。

根项目通过精确固定的开发依赖 `skills: 1.5.13` 运行真实 CLI 互操作测试；该依赖只服务开发验证，不进入桌面应用运行时。测试在临时 Eve Project 中离线覆盖 root、named、multiple、无 Eve target 和 update placement 重放。普通 Local 来源仍按 CLI 规则不可更新；重放场景使用同机 `file://` Git 仓库，只验证可更新来源的 placement 契约。

项目不维护版本化 Agent catalog fixture。同步上游时直接更新 vendored 源码，比较相关实现，再修改 Skill Deck 的 Built-in definitions 和兼容测试。文档只记录当前基线和稳定检查方法，不保存逐版本迁移流水账。

## 共享与扩展边界

| 领域 | 共享基线 | Skill Deck 扩展 |
|---|---|---|
| Source | shorthand、Git/URL/local/well-known 解析，ref 与 Skill 子路径 | Environment-aware acquisition、discovery session、风险展示 |
| Agent | Built-in ID、路径、detection 和基础目标语义 | Custom Agent、开放 Registry、scope-aware defaults、目录检查和 physical ownership |
| 安装 | 通用 Skill 目录、Agent 目录项、link/copy 和重新安装语义 | preview/execute、批量 unit、跨 Environment bridge、Recovery |
| Global lock | CLI v3 共享字段与路径 | `defaultTargetAgents`、无损局部写回 |
| Project lock | CLI v1 共享字段、排序和内容 hash | `sourceUrl`、`remoteHash`、`pluginName`、远端更新检测 |
| 更新 | 根据保存来源和 `skillPath` 重新安装 | 更新能力建模、远端比较、批量计划、修复来源 |

Environment session、payload handle、preview token、recovery marker 和 Custom Agent definition 不写入 CLI lock。它们属于 Skill Deck 本地运行时或产品配置。

## 通用 Skill 目录

CLI 与 Skill Deck 共同使用以下 canonical Skill 目录：

| Scope | 目录 |
|---|---|
| Global | `~/.agents/skills` |
| Project | `.agents/skills` |

文档将它们统称为“通用 Skill 目录”。Agent 是否读取通用 Skill 目录、是否还需要自身的 Skill 目录，以及目录检查如何形成关联 Agent，由[Agent](./agents.md)定义。Environment 中的 Home、Project 与路径解析条件由[Environment 与 Context](./environments-and-contexts.md)定义。

## Global lock

Global lock 的 canonical 位置遵循 CLI 规则：

- 设置 `XDG_STATE_HOME` 时使用 `$XDG_STATE_HOME/skills/.skill-lock.json`；
- 否则使用 `~/.agents/.skill-lock.json`。

当前 schema version 为 `3`。每个 Skill entry 的共享字段包括：

| 字段 | 语义 |
|---|---|
| `source` | 规范化来源标识 |
| `sourceType` | GitHub、GitLab、Git、local 或 well-known 等来源类别 |
| `sourceUrl` | 能够保留原始定位信息的来源 URL |
| `ref` | branch 或 tag |
| `skillPath` | 仓库相对的 `SKILL.md` 文件路径，保留磁盘实际大小写 |
| `skillFolderHash` | CLI 用于来源版本跟踪的可比较 hash；GitHub 为 Skill 目录的 Git tree object ID，GitLab 和 generic Git 为 CLI-compatible content hash |
| `installedAt` | 首次安装时间 |
| `updatedAt` | 最近更新时间 |
| `pluginName` | 可用时的 plugin 名称 |

Global lock 还包含两个选择字段：

- `lastSelectedAgents` 是 CLI 的最后选择记录。Skill Deck 只写当前 effective defaults 中能够被 CLI 理解的 Built-in projection，不写 Custom Agent ID。
- `defaultTargetAgents` 是 Skill Deck 的扩展，分别保存 Global 与 Project 默认 Agent ID，可以包含 Built-in 和 Custom Agent。

缺少 `defaultTargetAgents` 时，应用按本次明确选择、当前 scope fallback 和 Built-in 初始推荐产生向导初值。不能从 `lastSelectedAgents` 推断不存在于 CLI 模型中的 Custom Agent 偏好。

## Project lock

Project lock 位于 `<project>/skills-lock.json`，当前 schema version 为 `1`。它适合提交到项目仓库，因此写入顺序和字段语义保持稳定。

| 字段 | 归属 | 语义 |
|---|---|---|
| `source` | CLI/Skill Deck | 规范化来源标识 |
| `ref` | CLI/Skill Deck | branch 或 tag |
| `sourceType` | CLI/Skill Deck | 来源类别 |
| `skillPath` | CLI/Skill Deck | 仓库相对的 `SKILL.md` 文件路径，更新时再规范化为目录 |
| `computedHash` | CLI/Skill Deck | Project Skill 当前目录的递归内容 hash |
| `subagents` | CLI/Skill Deck | Eve Project target；空字符串表示 root agent，其他值表示 subagent 目录名。已确认属于 Eve 的 legacy entry 缺少该字段时按 root 读取，但不自动补写；无法确认 Eve 身份时不猜测 placement |
| `sourceUrl` | Skill Deck | 私有或原始来源保真 |
| `remoteHash` | Skill Deck | provider 可比较的 upstream revision；GitHub 为 Skill 目录的 Git tree object ID |
| `pluginName` | Skill Deck | 展示用 plugin metadata |

Eve root-only placement 遵循 CLI 的最小写入方式，可以省略 `subagents`；具名或 multiple placement 使用明确数组值。没有 Eve target 时不写 Eve placement，也不使用 `subagents: []` 发明新的共享语义。已存在的空数组按外部输入保留，并由已确认的 Eve Context 按兼容规则解释，不能在读取时静默迁移。字段不是数组或数组中包含非字符串值时，应用按 lock 损坏处理，不能把 malformed placement 当成字段缺失后回退到 root。

CLI 的 `computedHash` 由相对路径和文件内容以确定性顺序计算 SHA-256，并跳过 `.git`、`node_modules` 等非 Skill 内容。Skill Deck 从已经固定的 payload 计算兼容 hash，不能把 payload identity 或 `remoteHash` 写入这个字段。

Skill Deck 仍支持读取旧的 `<project>/.agents/.skill-lock.json`，并在受控写入时迁移到 canonical Project lock。Legacy 路径是 Skill Deck 的读取兼容，不要求上游 CLI 采用。

## Hash 的职责

不同 hash 解决不同问题，不能因为值的格式相同就互换：

| Hash | 负责问题 |
|---|---|
| Global `skillFolderHash` | CLI Global 来源版本追踪 |
| Project `computedHash` | 当前本地 Skill 目录内容是否变化 |
| Skill Deck `remoteHash` | 对支持的 source type 保存 provider upstream revision；当前 GitHub 使用 Skill 目录 tree object ID |
| Payload content hash | 固定一次 acquisition 后的完整目录快照 |
| Preview/token fingerprint | 判断用户确认后所依赖的 runtime facts 是否变化 |

Payload content hash 同时编码相对路径、entry kind、size、内容 hash 和 executable metadata，表示当前 Environment 能够实际保留的 payload identity。因此，同一文本内容在 Unix 与不提供 Unix executable bit 的 Native Windows 上可以形成不同的 payload hash。这个差异不进入 CLI `computedHash`；后者只按稳定相对路径和文件 bytes 计算，并通过仓库 `.gitattributes` 保持跨平台 fixture bytes 一致。

更新检查必须根据 evidence 类型选择对应的已安装基线：

| Source evidence | Global 比较基线 | Project 比较基线 | Project `remoteHash` 写入 |
|---|---|---|---|
| GitHub Skill tree object ID | `skillFolderHash` | `remoteHash` | 写入 tree object ID |
| GitLab/generic Git CLI-compatible content hash | `skillFolderHash` | `computedHash` | 不写入 |

Project 缺少 `remoteHash` 时仍可根据完整 source 与 `skillPath` 主动重新安装。GitHub 缺少 tree revision 时不能声称远端存在或不存在更新；GitLab 和 generic Git 可以把远端 clone 得到的 CLI-compatible content hash 与 Project `computedHash` 比较，不需要把该 content hash 写入 `remoteHash`。缺少 `skillPath` 时不能按 Skill name 猜测上游位置，必须进入来源修复。

`computedHash` 和 payload hash 不能作为 `remoteHash` 的 fallback。远端 CLI content hash 可以与 `computedHash` 比较，但二者的来源和字段职责保持独立。GitHub Source 使用选中 Skill 目录对应的 Git tree object ID，Host 与 WSL 都接受上游支持的 Git object ID 格式。Well-known 和缺少可靠 Git revision 的 GitHub Source 不伪造可比较版本。

## 无损写回

Skill Deck 使用 lossless JSON document 读取 Global 和 Project lock。写入时只替换当前 use case 拥有的 root field 或 Skill entry，并从最新 document 保留：

- 无关 Skill entries；
- entry 中未知的未来字段；
- root 中未知的未来字段；
- 其他进程在 capture 后写入且不与 owned field 冲突的内容。

这项保证属于 Skill Deck。上游 CLI 当前的 typed entry replacement 不保证保留 Skill Deck 扩展字段或未来未知字段。因此 CLI 更新某个 Project Skill 后，`sourceUrl`、`remoteHash` 或 `pluginName` 可能缺失。Skill Deck 把它视为能力降级，不把 lock 判定为损坏，也不虚构丢失值。

CLI 新增已知字段后，未知字段保留机制只能避免数据丢失，不能替代模型同步。维护者仍需更新 Rust DTO、业务语义、generated bindings 和兼容测试。

## Source、Discovery 与 Well-known

Skill Deck 与 CLI 对可共享来源保持以下互操作约束：

- `#ref` 在 URL parser 吞掉 fragment 之前提取；
- Skill filter 与 ref 按同一顺序解析；
- SSH/private Git 输入保留原始认证所需表达；
- `skillPath` 始终能够精确定位来源中的原 Skill；
- discovery 对有效 root、priority directory、plugin manifest 和 recursive fallback 保持相同的选择语义；
- root eligibility、Project lock filtering、父级 shadow、同名去重和稳定顺序在 Host 与 WSL 中保持一致；
- `SKILL.md` 大小写识别与磁盘上的实际路径保真分开处理；
- well-known endpoint、legacy index path safety 和 artifact digest 与上游协议保持一致；
- 安装 payload 保留 dotfiles，只排除明确的 VCS、cache 或 metadata 内容。

Skill Deck 可以增加 acquisition cache、progress、trust metadata 和安全审计展示，但不能借此放宽 CLI 共享协议的路径与内容校验。

## Agent 兼容

Built-in Agent definitions 以 vendored CLI 的 Agent registry、路径、detection、legacy behavior 和 adapter 为同步来源。Skill Deck 将这些定义投影到统一的开放 Registry，并用 Custom Agent 补充上游暂未覆盖的工具。

CLI 中的 `universal` 或静态列表是上游命令选择机制，不直接成为 Skill Deck 的读取能力。Skill Deck 根据当前 scope 的 definition 和目录检查结果判断 Agent 是否能够读取当前 Skill。

CLI 不理解 Custom Agent ID。Skill Deck 不把 Custom ID 伪装成 CLI Agent，也不要求 CLI 能管理其 Agent 目录项。二者的互操作点是通用 Skill 目录、Built-in projection 和共享 lock 字段。

Agent 模型的完整语义见[Agent](./agents.md)。

## 安装与更新语义

两者共享“根据来源重新安装完整 Skill”的核心语义。Skill Deck 的批量预览、immutable payload、unit result 和 rollback 不改变最终来源与目录的可解释性。

CLI 支持从 Local path 安装并把 `sourceType: "local"` 与内容 hash 写入 Project lock，但 `update` 会跳过 Local entry，不为它做远端版本检查或自动重装。Skill Deck 的跨 Environment copy 也沿用这条边界：Local copy 可保留 provenance，但不把它宣称为可更新 lineage；可重新获取来源的后续更新直接在目标 Environment 获取，不追踪原始来源 Environment。Copy 不改变 source capability。

CLI v1.5.13 的安装与 lock restore 已经按 source 和 ref 合并多个 Skill，一次获取来源后完成 discovery 与多个 Skill 的安装。Global 更新检测同样按 source 合并：GitHub 一次获取完整 repo tree，GitLab 和 generic Git 一次 clone 后计算多个 Skill 目录的 CLI-compatible content hash。这些行为是 Skill Deck 合并 acquisition 与 evidence 请求的兼容依据。

CLI 的更新执行仍逐 Skill 重新调用 `add`。Global 更新在完成 grouped detection 后逐项获取来源；Project 更新先按 source clone 检查上游删除，再逐 Skill 调用 `add`，因此同仓库多 Skill 可能发生重复 clone。Skill Deck 不复制这一过程性限制：用户确认更新后按 source、ref 和执行 Environment 获取一次 snapshot，再为多个 Skill 生成独立 payload 和 mutation unit。

CLI 源码中的 `HostProvider` interface 当前只覆盖远程 `SKILL.md` host 能力，vendored 基线仅由 well-known provider 实现；GitHub blob fast-path、Git clone 和 local acquisition 仍由独立能力路由。Skill Deck 因此采用组合式 acquisition 与 evidence strategy，不把所有 Source 类型收敛成承担完整生命周期的 Provider 继承体系。

CLI 与 Skill Deck 都将安装内容物化为可独立使用的完整 Skill 目录。Skill Deck 在 acquisition 阶段固定安全 payload，拒绝指向来源外部、dangling 或循环的 symlink；具体 payload safety 由[执行与恢复](./execution-and-recovery.md#payload)定义。

Project update 在有 `skillPath` 时可以直接定点重新安装，不要求先完成远端变化检测。Skill Deck 的远端比较 metadata 只用于改善检查体验，不能成为 reinstall 的必要条件。

Materialization 的平台实现可以不同：Windows 使用适合目录的 link primitive，macOS/Linux 使用 POSIX symlink，WSL 使用 distro 内的 POSIX 语义。

CLI v1.5.13 在 symlink 创建失败时会复制到 Agent 目录，并返回 `mode: "symlink"` 与 `symlinkFailed: true`。Skill Deck 当前 mutation pipeline 不自动 fallback；link 创建失败时 unit 失败，用户可以重新预览并选择 Copy。这是明确的产品差异，不能仅根据 CLI 的返回字段推断 Skill Deck 的实际 mode。

完整业务流程见[Skill 生命周期](./skill-lifecycle.md)，原子写入和冲突处理见[执行与恢复](./execution-and-recovery.md)。

## 上游同步流程

每次更新 vendored CLI 时，维护者按以下顺序处理：

1. 在 `vercel-skills/` 内确认 package version、目标 tag、commit 和工作区状态。
2. 阅读真实 diff，按行为而不是文件名归类变化。
3. 检查 Agent registry、source parser、well-known provider、discovery、installer、Global lock、Project lock 和 update command。
4. 将共享变化同步到 Skill Deck；保留 Custom Agent、Environment、lossless lock、远端检测和 Recovery 等已有增强。
5. 更新 Built-in behavior tests、lock fixtures/hash vectors、source/discovery tests 和受影响的 Frontend 状态。
6. Rust command/type 变化时重新生成 bindings，并核对 window ACL。
7. 更新本文的基线和稳定差异，不写逐版本实施记录。
8. 运行[贡献指南](../CONTRIBUTING.md)规定的完整验证。

不使用固定 `agents.json` snapshot 代替源码比较。Agent 定义只是同步面之一，lock、source、discovery、install 和 update 变化同样可能影响互操作。

## 同步检查表

| 检查面 | 需要确认的行为 |
|---|---|
| Agent registry | ID、alias、Global/Project path、detection、legacy、adapter |
| Source parser | shorthand、URL、SSH、ref、filter、alias、subpath |
| Discovery | root early-return 条件、priority/plugin path、fallback、目录深度、shadow、locked filtering、同名去重和 exact `skillPath` |
| Well-known | endpoint 顺序、schema、digest、archive/path safety |
| Installer | 通用 Skill 目录、Agent target、排除项、link/copy 与 link failure 行为 |
| Global lock | version、路径、字段、defaults projection、unknown fields |
| Project lock | version、字段、排序、hash、entry replacement |
| Update | reinstall 定位、缺失 metadata、上游删除和结果语义 |
| Skill Deck extensions | 降级仍可解释，CLI 写入后不误报损坏 |
| Contracts | Rust DTO、bindings、ACL、i18n 和 tests 同步 |

## 必须保持的规则

1. Vendored 源码和版本是同步证据，文档和 fixture 不能替代它。
2. CLI 是兼容基线，不是 Skill Deck 的功能上限。
3. 共享字段保持 CLI 语义，Skill Deck 扩展字段保持可选。
4. Skill Deck 写 lock 时保留不属于当前操作的字段和外部变化。
5. 不宣称上游 CLI 的 typed write 具有同样的未知字段保留能力。
6. `computedHash`、upstream revision、`remoteHash`、payload hash 和 preview fingerprint 不互换。
7. 缺少更新检测 metadata 时降级能力，不伪造“无更新”。
8. Custom Agent 不写入 CLI 不理解的 Agent 选择字段。
9. 上游同步同时检查 Agent、source、discovery、install、lock 和 update。
