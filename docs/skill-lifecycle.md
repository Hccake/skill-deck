# Skill 生命周期

## 生命周期概览

Skill Deck 以完整目录作为 Skill 的业务对象。目录包含 `SKILL.md`，也可以包含 `scripts`、`references`、`assets`、隐藏文件、嵌套目录和可执行文件。来源、安装位置、Agent 目录项和 lock 元数据共同描述已安装状态。

```mermaid
flowchart LR
    Input["来源输入"] --> Parse["解析来源"]
    Parse --> Acquire["获取并固定内容"]
    Acquire --> Discover["发现可安装 Skill"]
    Discover --> Select["选择 Skill 与目标"]
    Select --> Preview["预览风险与变更"]
    Preview --> Install["安装"]
    Install --> Installed["已安装"]

    Installed --> Read["读取内容"]
    Installed --> Check["检查更新"]
    Check --> Update["按保存来源更新"]
    Installed --> Repair["修复来源"]
    Installed --> Manage["管理 Agent"]
    Installed --> Copy["复制到项目"]
    Installed --> Remove["移除"]

    Update --> Installed
    Repair --> Installed
    Manage --> Installed
    Copy --> Installed
    Remove --> End["完成本地清理"]
```

安装、更新、来源修复、复制、管理 Agent 和移除都会先生成预览，执行时再根据最新目录、lock、Agent 和 Environment 状态重新验证。本文说明这些操作的业务流程；内容快照、变更计划、lock 和恢复资源的执行规则见[执行与恢复](./execution-and-recovery.md)。

## 来源

来源（`Source`）是能够定位 Skill 内容的位置声明。支持的输入包括：

- GitHub 简写、GitHub/GitLab URL 和仓库子路径；
- 带 `branch`、`tag` 或 Skill filter 的 Git 来源；
- SSH 以及其他可通过 Git clone 获取的 URL；
- 当前 Host 或 WSL Environment 中的本地路径；
- Well-known 地址；
- 可以解析出来源、Skill 和 Agent 选择的受支持 `skills add` 命令。

解析结果保留规范化来源、来源类型、原始 URL、`ref`、仓库子路径和 Skill 筛选条件。私有 Git 来源继续使用包含认证信息的原始 SSH 或 URL 表达。

来源解析器负责理解输入，后续获取步骤执行 Git clone，安装步骤处理目标 Agent。与上游 `skills` CLI 共用的解析规则见[skills CLI 兼容](./skills-cli-compatibility.md)。

## 获取与发现

来源获取在当前执行 Environment 中完成。来源类型与 Environment 共同决定获取后端。一次获取会建立发现会话，并保存来源信息、发现到的 Skill 路径和生成内容快照所需的数据。跨 Environment 复制由来源 Environment 承担这一获取步骤。

应用按照与 `skills` CLI 兼容的目录根、优先级、插件清单（plugin manifest）和递归回退规则发现有效 Skill。Host 与 WSL 使用相同的选择规则，并由各自后端完成目录读取。具体协议见[skills CLI 兼容](./skills-cli-compatibility.md)。

用户确认后，应用从发现会话生成不可变的完整 Skill 内容快照。安装、来源修复和跨 Environment 复制在进入预览前固定快照，执行阶段使用同一份内容。获取失败时返回来源错误；快照或预览过期后，流程回到来源获取与准备步骤。

## 安全信息

Discover 和安装确认页可以展示来源风险、Well-known 信任信息和远端安全审计结果。审计属于辅助信息；服务超时或不可用时显示“审计未完成”，用户仍可查看来源并决定是否安装。

被判定为需要保护的来源，必须由用户明确确认风险后才能执行安装。来源、目标或预览发生变化后，需要重新确认。

## 安装

安装请求包含目标 Context、一个或多个已获取的 Skill、目标 Agent、Eve 等专用适配目标，以及用户选择的 `symlink` 或 `copy` 方式和必要的覆盖确认。

每个 Context 都有一份主 Skill 目录。读取通用 Skill 目录的 Agent 直接使用主目录；读取自身 Skill 目录的 Agent 在对应位置建立目录项。读取能力、目标分组和目录项含义见[Agent](./agents.md#目标选择与目录分组)。

Eve 项目使用专用适配目标，用户可以选择根 Agent 或已经发现的子 Agent。安装时从固定的 Skill 内容快照生成与 `skills` CLI 兼容的 Eve 内容，并在 Project lock 中记录目标；后续读取和更新按该记录恢复同一位置。

具体规则见[Agent](./agents.md#eve-适配目标)和[skills CLI 兼容](./skills-cli-compatibility.md)。

符号链接（`symlink`）和复制（`copy`）是本次安装的落盘方式。链接创建失败时，本次操作返回失败，用户重新预览后可以改选复制。Agent 定义描述读取方式；平台差异和写入一致性见[执行与恢复](./execution-and-recovery.md)。

成功安装后，主 Skill 目录、必要的 Agent 目录项和本次负责的 lock 字段保持一致。批量安装按 Skill 返回独立结果，每个 Skill 保留自己的成功或失败状态。

## 已安装状态与读取

已安装 Skill 的展示综合当前 Context 的目录状态、lock 元数据、来源信息、`skillPath`、哈希基线和 Environment 状态。目录与 lock 分别提供内容和来源证据；其中一项缺失或不可用时，列表展示不完整状态，并提供查看、修复或移除入口。

读取 `SKILL.md` 或打开资源时，应用根据 Skill 标识和 Context 重新解析实际目录。后端使用业务标识完成授权和路径校验，展示路径用于界面说明。资源读取边界见[系统架构](./architecture.md)。

## 更新检查与重新安装

更新能力、更新检查结果和更新执行是三个不同概念：

| 概念 | 含义 |
|---|---|
| 可以重新安装 | 保存的来源、`ref` 和 `skillPath` 足以再次获取当前 Skill |
| 可以检查更新 | 当前来源类型具有可比较的远端版本证据 |
| 检查结果 | 本次检查得到有更新、没有更新、来源不可达、上游已删除或信息不足 |
| 执行更新 | 按保存的来源获取新内容，并按当前目标重新安装 |

本地（`Local`）来源标记为不可更新，并跳过远端更新检查。远端（`Remote`）、Git 和 Well-known 来源在信息完整时可以重新获取；跨 Environment 复制后，更新直接在目标存储归属环境获取来源。缺少可比较版本证据的来源仍可在信息完整时由用户主动重新安装，检查状态显示为“无法比较”。

更新按照保存的来源、`ref` 和仓库相对的 `skillPath` 重新安装完整目录。`skillPath` 精确定位原 Skill。指向主目录的链接会随主目录更新；已经发生本地修改的副本默认保留，用户明确选择后才覆盖。更新目标来自当前已经关联的 Agent 和用户本次选择，注册表中的其他 Agent 保持未关联状态。

检查失败显示为“检查未完成”，来源暂时不可达显示对应错误。“没有更新”只表示本次比较已经成功完成。详细的哈希、lock 字段和上游差异见[skills CLI 兼容](./skills-cli-compatibility.md)；预览、执行和批量结果见[执行与恢复](./execution-and-recovery.md)。

## 来源修复

来源修复用于处理 Skill 仍在本地、但普通更新无法可靠定位上游的情况，例如 lock 缺少 `skillPath`、上游目录发生移动或删除、来源失效，或私有来源信息不足。

来源修复复用正常的获取、准备、预览和安装链路。用户重新选择来源后，应用重新发现 Skill，并通过来源中的实际 `skillPath` 生成新预览。上游已经删除的本地 Skill 继续保留，由用户决定修复、保留或移除。

来源修复进入受保护写入后，如果系统无法确认文件与 lock 一致，操作会保留恢复资源。用户可以在恢复入口中检查相关文件，具体边界见[执行与恢复](./execution-and-recovery.md)。中断的来源修复需要从原入口重新发起。

## 管理 Agent

管理 Agent 使用当前主 Skill 内容调整 Agent 目录项：

- 添加目标时，用当前主 Skill 内容创建缺失的目录项；
- 移除目标时，处理目录检查确认属于当前 Skill 的目录项；
- 清理额外目录项后，通用 Skill 目录中的主 Skill 继续保留；
- 多个 Agent 指向同一实际目录时只执行一次物理变更，同时保留仍有效的 Agent 关系。

单个 Skill 的主目录、Agent 目录项和相关 lock 变更作为一个整体处理；多个 Skill 或 Project 组成的批次才可能出现部分完成。目标分组、检测和关联 Agent 的含义见[Agent](./agents.md)，写入一致性见[执行与恢复](./execution-and-recovery.md)。

## 复制到项目

复制以一个 Project Context 中已经安装的 Skill 为来源。用户选择一个目标 Environment，并在其中选择一个或多个 Project Context。目标 Environment 同时承担目标项目路径的存储归属职责；来源 Environment 可以不同。Global Skill 通过安装流程进入 Project。

复制传递完整 Skill 内容，包括嵌套目录、脚本和元数据。跨 Environment 时，来源先固定内容快照，再将经过校验的内容交给目标 Environment。快照固定后，本次执行只依赖目标 Environment，并由目标后端完成最终写入。

复制资格由后端预览统一判断，判断依据包括来源记录、项目关系、路径和存储能力。来源记录缺失或无法解释时，复制流程提供来源修复入口；修复成功后，用户重新点击复制并确认新的预览。

每个目标 Project 独立返回结果，读取失败、路径冲突、覆盖风险和存储能力不足分别归入对应目标。部分完成时，下一次重试保留尚未成功且适合重试的目标；需要检查相关文件的目标保留独立恢复入口。

复制成功后：

- 远端（`Remote`）、Git 和 Well-known 来源的目标 Project 保留来源、`ref`、Skill 路径和更新基线；之后的更新直接在目标存储归属环境重新获取来源；
- 本地（`Local`）来源保留路径和内容基线作为来源凭据，目标 Skill 标记为不可更新。

复制前后，源 Skill 的更新能力保持不变。目标 Project 获得独立内容和来源记录，后续工作直接在目标 Environment 中完成。

## 移除

移除预览展示通用 Skill 目录和当前确认属于该 Skill 的 Agent 目录项。用户确认后，应用删除这些位置，并同步清理由 Skill Deck 管理的本地记录。移除处理整个 Skill；部分 Agent 调整通过管理 Agent 完成。

多个 Agent 使用同一实际目录时，应用处理一次物理目录，并在结果中保留仍有效的 Agent 关系。界面使用“符号链接”和“副本”表达目录项类型。

Skill 的可用范围由安装状态和 Agent 适配关系共同决定。移除会删除当前 Context 中的 Skill，管理 Agent 用于调整具体 Agent 的读取关系。

## 生命周期规则

- 来源、Skill 路径、Context 和 Environment 共同组成业务身份，Skill 名称主要用于展示；
- 发现会话和内容快照是临时事实，lock 与已安装目录构成长期本地状态；
- Agent 检测结果与用户显式选择分别保存，目录检查与 lock 元数据分别提供当前内容和来源信息；
- 更新检查成功后才能得出“有更新”或“没有更新”，上游删除由用户选择修复、保留或移除本地内容；
- 来源修复复用正常安装链路，管理 Agent 复用当前主 Skill 内容；
- 受保护写入无法确认一致时，返回需要检查的结果并保留恢复资源；单个 Skill 使用恢复状态表达未完成的一致性处理。
