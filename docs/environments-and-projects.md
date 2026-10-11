# Environment、Skill 位置与项目管理

Environment 表示 Skill、项目和文件操作所属的用户空间。Skill Deck 默认使用桌面应用所在系统的当前用户空间；Windows 用户还可以选择某个 WSL 发行版中的 Linux 用户空间。

| 桌面系统 | Skill 和项目的管理位置 | 切换范围 |
|---|---|---|
| Windows | 默认使用当前 Windows 用户；启用 WSL 支持后还可以选择某个 WSL 发行版的 Linux 用户空间 | 可以在 Windows 与已经发现的 WSL 发行版之间切换 |
| macOS | 使用当前 macOS 用户的目录和文件系统 | 不提供切换功能 |
| Linux | 使用当前 Linux 用户的目录和文件系统 | 不提供切换功能 |

## 管理全局 Skill 和项目 Skill

当前 Environment 中的全局管理范围和每个项目管理范围分别构成一个 Scope。用户在界面中选择的 Skill 位置就是这类逻辑范围：

- 全局 Skill 安装在某个 Environment 的全局位置，不属于具体 Project；
- 项目 Skill 安装在具体 Project 中，属于该 Project。

Scope 包含对应的通用 Skill 目录和 Agent 专用 Skill 目录。Environment 决定这些目录属于哪个用户空间；一次操作固定使用的 Environment 和 Scope 共同构成操作位置。

Agent 能否发现并加载这些 Skill，取决于对应 Agent 的 Skill 读取位置和 Skill 的实际安装目录。

切换 Environment 不会移动或复制 Skill。Windows 用户切换到某个 WSL 发行版后，界面改为展示该发行版中的全局 Skill 和已添加项目；切回 Windows 后，界面重新展示 Windows 用户空间中的内容。

Agent 的 Skill 读取位置和 Agent 检测位置在应用中统一保存。Windows 用户空间与各 WSL 发行版共享这套信息，其中的用户主目录（Home）、配置目录（ConfigHome）、项目相对路径和检测结果会按照当前 Environment 重新解析。完整规则见[Agent 模型](./agent-model.md)。

## 在 Windows 和 WSL 之间切换

Environment 切换只在 Windows 上出现。WSL 支持默认关闭；用户在通用设置中启用后，Skill Deck 才会发现已经安装的 WSL 2 发行版，并在发现可用发行版后显示切换入口。WSL 1 不进入 Environment 列表。Skill Deck 始终作为 Windows 桌面应用运行，切换到 WSL Environment 只会改变后续路径解析和文件操作的位置。

Windows 及其系统工具负责 WSL 和发行版的安装、导入、启动与移除。Skill Deck 负责发现发行版、建立连接，并在用户选中的发行版中执行 Skill 操作。连接一个已经安装但处于停止状态的发行版时，`wsl.exe` 可能按需启动它。

主窗口启动后先展示当前 Windows 用户的全局 Skill。用户选择某个 WSL 发行版后，应用先完成连接并展示该发行版的全局 Skill，再独立加载其中的项目列表。项目列表加载失败不会撤销已经完成的切换，用户仍可使用该发行版中不依赖项目列表的功能。

`Skills`、`Skill 库`、`发现` 以及设置中与 Agent 和项目有关的页面共同使用主窗口当前的 Environment。独立安装向导在打开时固定本次 Environment：从 `Skills` 页发起时，入口同时确定全局或项目位置；从 `发现` 页发起时，用户在该 Environment 中选择全局或已添加项目。主窗口随后切换 Environment 或项目，不会改变向导中的选择。完整流程见[产品行为与交互](./product.md#统一安装流程)。

在 Windows 与 WSL 之间复制 Skill 时，复制流程使用用户明确选择的目标，主窗口继续显示原位置。

用户关闭 WSL 支持时，应用先切回 Windows，再停止发现和使用 WSL。已经保存在 WSL 中的项目记录、恢复记录和其他持久化数据继续保留；重新启用并连接对应发行版后，Skill Deck 会重新读取这些内容。关闭 WSL 支持不会影响 Windows Environment 中的 Skill、项目和 Agent 工作流。

发行版发现失败时，Windows Environment 和已经可用的其他发行版继续工作。发现过程负责更新可选发行版列表，连接过程负责确认某个发行版当前能否使用；其中一个过程失败，不会清空另一个过程已经确认的信息。

连接阶段读取用户身份和目录环境，并启动发行版中的辅助进程 Environment Worker。Git、`wslpath` 等具体工具在对应操作首次使用时检查；工具缺失只影响依赖它的操作，具体执行边界见[系统架构](./architecture.md#wsl-连接与-worker-部署)。

## 添加和移除项目

添加项目让 Skill Deck 记住项目位置，方便用户在后续会话中再次访问。项目记录按 Environment 分别保存：macOS 和 Linux 使用应用所在系统的项目列表，Windows 与各 WSL 发行版分别使用自己的项目列表。

项目记录使用稳定 ID 标识项目，操作时由后端根据 ID 重新取得记录并解析实际路径。界面优先显示去除首尾空格后的项目展示名称；没有配置展示名称时，按目标平台的路径规则取得末级目录名，完整原生路径作为次要定位信息。只有选中具体项目后，项目相对路径才能解析为绝对路径。

同一 Environment 内，规范化后相同的路径共用一条项目记录，重复添加时返回已有记录。不同路径通过符号链接或 Windows 目录联接（junction）指向同一物理目录时，仍可能形成独立记录；每条记录保存规范化后的路径，并分别展示访问状态。具体比较规则见[项目路径规范化](#识别和转换项目路径)。

当前 Environment 的用户主目录已经用于全局 Skill，不能再作为项目添加；用户主目录下的具体项目目录可以正常添加。路径别名和用户主目录的判断见[识别和转换项目路径](#识别和转换项目路径)。

移除项目会删除访问记录和由 Skill Deck 管理的库应用链接，项目目录、直接安装 Skill 和其他项目文件继续保留。项目仍应用 Skill 库时，移除流程按以下顺序处理：

1. 继续尚未完成的库应用变更。
2. 清理该项目中受管理的通用目录链接、Agent 专用目录链接和应用关系。
3. 清理成功后移除项目记录。

项目不可访问、未完成变更无法继续或链接所有权无法确认时，移除流程返回对应原因并保留项目记录。

项目列表成功加载后会形成一份完整快照。后续刷新期间继续展示已有项目；刷新失败时保留最近一次成功加载的列表，并单独显示本次错误。项目列表是否完整与 WSL 是否连接是两个独立状态；项目列表读取失败不会使已经连接的发行版失效。

WSL 项目记录由对应发行版中的 Environment Worker 写入。记录被其他操作修改时，本次请求返回结果过期，应用重新读取后再处理。单文档写入协议见[执行与恢复](./execution-and-recovery.md#原子写入与-lock-提交)。

## 访问 Windows 和 WSL 中的项目

在 Windows 上，保存项目记录的 Environment 与项目文件实际所在的文件系统是两个独立属性。例如，用户可以在 Windows Environment 中添加一个 WSL UNC 路径，也可以在 WSL 中添加一个映射到 Windows 磁盘的路径。Skill Deck 可以识别和展示这些项目，但不会直接跨文件系统执行受保护写入。

项目访问状态分为以下四种：

| 状态 | 含义 |
|---|---|
| 可以直接管理（`Native`） | 当前 Environment 能够使用目标文件系统的原生路径和文件操作能力，可以执行受保护写入 |
| 跨文件系统（`CrossStorage`） | 当前 Environment 可以读取或观察项目，但项目文件位于 Windows 或其他 WSL 发行版中；需要切换后才能写入 |
| 不支持（`Unsupported`） | 已确认当前 Environment 无法安全访问目标项目 |
| 暂时无法判断（`Unknown`） | 当前还不能确认项目文件所在位置，或者当前 Environment 是否具备访问能力 |

Windows 访问 WSL UNC、WSL 访问 Windows 挂载路径，以及一个 WSL 发行版访问另一个发行版的路径，都可能形成跨文件系统状态。界面会展示项目当前所在位置并引导用户切换；只有当前 Environment 能够原生访问项目文件时，Skill Deck 才会生成安装、更新、移除、复制或管理 Agent 的写入计划。

受保护写入使用目标文件系统原生的路径身份、原子文件操作、lock 和恢复位置。设计取舍见[受保护写入必须使用目标文件系统的原生能力](./adr/0003-protected-writes-follow-storage-owner.md)，写入安全规则见[执行与恢复](./execution-and-recovery.md)。

## 识别和转换项目路径

Skill Deck 会先按照目标 Environment 的路径规则转换用户输入，再保存项目记录：

- Windows 接受 Windows 原生路径、普通 UNC 路径和指向 WSL 的 UNC 路径；
- macOS 和 Linux 接受当前系统的 POSIX 路径；
- WSL 发行版接受自身的 POSIX 路径，以及指向当前发行版的 `\\wsl$` 或 `\\wsl.localhost` 路径；
- Windows 路径进入 WSL 时，Skill Deck 通过 Environment Worker 调用目标发行版提供的 `wslpath`；
- 指向其他 WSL 发行版的 UNC 路径会返回跨发行版错误，无法安全转换的路径会返回不支持映射错误。

保存项目记录时，Skill Deck 按目标 Environment 的路径规则规范化路径文本，并在该 Environment 的项目列表内去重：

- 统一路径分隔符，消除多余的 `.`、`..` 和末尾分隔符；
- Windows 盘符和普通 UNC 路径按大小写不敏感比较；
- `\\wsl$` 与 `\\wsl.localhost` 视为同一种 WSL UNC 形式，发行版名称按大小写不敏感比较，发行版之后的 Linux 路径保留大小写语义；
- macOS、Linux 和 WSL 的项目记录使用 POSIX 路径规则，路径文本按大小写敏感比较。

以下示例说明项目记录的比较结果：

| 路径类型 | 两种输入 | 记录结果 |
|---|---|---|
| Windows 盘符路径 | `c:/Code/Skills/../App/` 与 `C:\code\app` | 共用一条记录；先添加第一种输入时，保存为 `C:\Code\App` |
| Windows 中的 WSL UNC 路径 | `\\wsl$\Ubuntu\home\alice\App` 与 `\\WSL.LOCALHOST\ubuntu\home\alice\App\` | 共用一条记录；发行版之后的 `App` 与 `app` 仍作为不同路径处理 |
| POSIX 路径 | `/work/./Skills/../App/` 与 `/work/App` | 共用一条记录，保存为 `/work/App`；`/work/app` 作为另一条路径处理 |

项目记录的去重依据是路径文本。实际文件操作还会解析符号链接、目录联接等路径别名，并确认目标的物理身份与文件系统能力。macOS 上的项目记录也按上述 POSIX 文本规则比较，实际文件系统的大小写行为由目标卷决定。

添加项目时，后端在写入项目记录前按照目标 Environment 的路径规则比较所选目录与用户主目录。能够解析到用户主目录的路径别名同样会被拒绝；这项限制不要求项目必须位于某个固定父目录，也不改变已有项目记录。目录身份的检查使用目标 Environment 的路径解析能力，路径安全要求见[执行与恢复](./execution-and-recovery.md#路径安全与实际目标)。

Agent 的路径声明和实际路径也分开处理：相对于 Home 或 ConfigHome 的路径使用当前 Environment 中的对应目录，项目相对路径结合已添加项目解析，绝对路径按所在系统解释。解析失败或目标暂时不可用时，原始声明继续保留，运行时状态显示为不可用或待确认。

项目 lock 中的 Local Skill 来源按项目根目录解析并优先保存相对路径，字段格式和跨平台保留规则见[skills CLI 参考与兼容](./skills-cli-reference.md#项目-lock-与-eve-安装位置)。

## Skill 库的数据归属

Skill 库、成员内容和应用关系保存在对应 Environment 用户主目录下的 `~/.skill-deck/skill-libraries`。Windows 和各 WSL 发行版分别使用自己的用户目录。切换 Environment 后，库页面和 `Skills` 页显示目标 Environment 的数据。

库只应用到同一 Environment 的全局 Scope 或已添加项目。目标目录需要由当前 Environment 原生管理，并支持安全创建链接；Windows UNC 路径、WSL 中的 Windows 挂载目录和跨发行版位置不支持库应用。

成员维护见[库成员生命周期](./skill-lifecycle.md#库成员生命周期)，应用顺序和目录中的版本选择见[库应用与版本选举](./skill-lifecycle.md#库应用与版本选举)。存储接口与运行时身份的边界见[系统架构](./architecture.md#数据与状态归属)。

## 把 Skill 复制到其他项目

普通安装和更新都在能够直接管理目标文件的 Windows、macOS、Linux 或 WSL 用户空间中完成。把 Skill 复制到其他项目时，来源 Environment 先保存完整的 Skill 内容快照，目标 Environment 再根据项目路径和访问状态执行受保护写入。

目标项目必须能够由目标 Environment 原生管理。Windows、macOS 和 Linux 均支持在应用所在系统中复制到其他项目；Windows 上还可以选择 WSL 发行版作为来源或目标。目标项目处于跨文件系统状态时，用户需要选择能够原生管理该项目的 Environment。

复制流程使用用户选择的目标 Environment 和项目 ID 定位目标，不依赖界面显示路径。目标 Environment 或项目从可选列表中消失后，用户需要重新选择并重新加载目标项目。复制的业务规则见[Skill 生命周期](./skill-lifecycle.md#复制到项目)，内容传递和写入协议见[执行与恢复](./execution-and-recovery.md)。

## 位置与项目的代码表示

以下类型用于表示 Skill 的管理位置、项目记录和文件资源：

| 代码类型 | 含义 |
|---|---|
| `EnvironmentRef` | `Native` 表示桌面应用所在系统的当前用户空间；`Wsl { distroName }` 表示 Windows 中某个具名 WSL 发行版 |
| `SkillLocation` | 区分全局 Skill 和由稳定项目 ID 标识的项目 Skill |
| `SkillLocationRef` | 将 `EnvironmentRef` 与 `SkillLocation` 组合为一次 Skill 读取或操作的稳定目标 |
| `RegisteredProject` | 保存项目 ID、原生路径和展示信息，不代表项目文件一定可以由当前 Environment 直接写入 |
| `ProjectStorageInfo` | 说明当前 Environment 能否直接管理项目文件，以及需要切换到 Windows 还是某个 WSL 发行版 |
| `ResourceLocator` | 使用 `EnvironmentRef` 和对应系统的原生路径定位文件资源 |

所有平台都使用 `EnvironmentRef::Native` 表示应用所在操作系统的当前用户空间。只有 Windows 还会使用 `EnvironmentRef::Wsl` 表示具名 WSL 发行版，并提供相应的切换入口。平台后端、Windows 与 WSL 之间的路径映射、连接修订号和进程监督由[系统架构](./architecture.md)维护。
