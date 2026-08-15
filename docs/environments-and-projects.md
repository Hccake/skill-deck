# Environment、Skill 位置与项目管理

Skill Deck 是安装在 Windows、macOS 或 Linux 上的桌面应用。它默认使用应用所在操作系统当前用户的目录和文件系统，读取和管理其中的 Skill、项目以及 Agent 状态。

| 桌面系统 | Skill 和项目的管理位置 | 是否可以切换 |
|---|---|---|
| Windows | 默认使用当前 Windows 用户；启用 WSL 支持后还可以选择某个 WSL 发行版的 Linux 用户空间 | 可以在 Windows 与已经发现的 WSL 发行版之间切换 |
| macOS | 使用当前 macOS 用户的目录和文件系统 | 不提供切换功能 |
| Linux | 使用当前 Linux 用户的目录和文件系统 | 不提供切换功能 |

## 管理全局 Skill 和项目 Skill

Skill Deck 管理两类 Skill：

- 全局 Skill 安装在某个 Environment 的全局位置，不属于具体 Project；
- 项目 Skill 安装在具体 Project 中，属于该 Project。

全局和项目描述的是 Skill 的逻辑位置，不是两个文件系统。macOS 和 Linux 始终在应用所在系统中查找这两类 Skill；Windows 未启用 WSL 支持时也使用 Windows 用户空间，启用后则按照用户当前选择的 Windows 用户空间或 WSL 发行版查找。

Agent 能否发现并加载这些 Skill，取决于对应 Agent 的 Skill 读取位置和 Skill 的实际安装目录。

切换 Environment 不会移动或复制 Skill。Windows 用户切换到某个 WSL 发行版后，界面改为展示该发行版中的全局 Skill 和已添加项目；切回 Windows 后，界面重新展示 Windows 用户空间中的内容。

Agent 的 Skill 读取位置和 Agent 检测位置在应用中统一保存。Windows 用户空间与各 WSL 发行版共享这套信息，其中的 Home、ConfigHome、项目相对路径和检测结果会按照当前 Environment 重新解析。完整规则见[Agent 模型](./agent-model.md)。

## 添加和移除项目

添加项目只是让 Skill Deck 记住项目位置，方便用户在后续会话中再次访问。移除已添加项目只会删除这条访问记录，不会删除项目目录、项目文件或其中的 Skill。

macOS 和 Linux 的项目记录保存在应用所在系统中。Windows 未启用 WSL 支持时，项目记录同样保存在 Windows 中；启用 WSL 支持后，Windows 和每个 WSL 发行版分别保存自己的项目列表。同一个项目目录可以通过不同路径多次添加，但每条记录都会保留各自的路径写法和访问状态。

项目记录使用稳定 ID 标识项目。界面显示的路径用于帮助用户确认位置，后端执行操作时会根据项目 ID 重新取得记录并解析实际路径。只有选中具体项目后，项目相对路径才能解析为绝对路径。

项目列表成功加载后会形成一份完整快照。后续刷新期间继续展示已有项目；刷新失败时保留最近一次成功加载的列表，并单独显示本次错误。项目列表是否完整与 WSL 是否连接是两个独立状态；项目列表读取失败不会使已经连接的发行版失效。

## 在 Windows 和 WSL 之间切换

Environment 切换只在 Windows 上出现。WSL 支持默认关闭；用户在通用设置中启用后，Skill Deck 才会发现已经安装的 WSL 发行版，并在发现可用发行版后显示切换入口。Skill Deck 始终作为 Windows 桌面应用运行，切换到 WSL Environment 只会改变后续路径解析和文件操作的位置。

Windows 及其系统工具负责 WSL 和发行版的安装、导入、启动与移除。Skill Deck 负责发现发行版、建立连接，并在用户选中的发行版中执行 Skill 操作。连接一个已经安装但处于停止状态的发行版时，`wsl.exe` 可能按需启动它。

主窗口启动后先展示当前 Windows 用户的全局 Skill。用户选择某个 WSL 发行版后，应用先完成连接并展示该发行版的全局 Skill，再独立加载其中的项目列表。项目列表加载失败不会撤销已经完成的切换，用户仍可使用该发行版中不依赖项目列表的功能。

`Skills`、`Discover` 以及 `Settings` 中与 Agent 和项目有关的页面共同使用主窗口当前的 Environment。独立安装向导在打开时固定本次安装位置；主窗口随后切换到 Windows、其他 WSL 发行版或其他项目，都不会改变已经打开的安装目标。在 Windows 与 WSL 之间复制 Skill 时，复制流程使用用户明确选择的目标，也不会改变主窗口当前显示的位置。

用户关闭 WSL 支持时，应用先切回 Windows，再停止发现和使用 WSL。已经保存在 WSL 中的项目记录、恢复记录和其他持久化数据继续保留；重新启用并连接对应发行版后，Skill Deck 会重新读取这些内容。关闭 WSL 支持不会影响 Windows Environment 中的 Skill、项目和 Agent 工作流。

发行版发现失败时，Windows Environment 和已经可用的其他发行版继续工作。发现过程负责更新可选发行版列表，连接过程负责确认某个发行版当前能否使用；其中一个过程失败，不会清空另一个过程已经确认的信息。WSL 发行版需要提供 Git、POSIX shell 和当前操作使用的基础 GNU 工具，具体执行条件见[系统架构](./architecture.md)。

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
- Windows 路径进入 WSL 时，Skill Deck 调用目标发行版提供的实际映射能力，例如 `wslpath`；
- 指向其他 WSL 发行版的 UNC 路径会返回跨发行版错误，无法安全转换的路径会返回不支持映射错误。

同一位置可能存在多种文本写法。Skill Deck 按对应系统的路径规则规范化并去重：

- 统一路径分隔符，消除多余的 `.`、`..` 和末尾分隔符；
- Windows 盘符和普通 UNC 路径按大小写不敏感比较；
- `\\wsl$` 与 `\\wsl.localhost` 视为同一种 WSL UNC 形式，发行版名称按大小写不敏感比较，发行版之后的 Linux 路径保留大小写语义；
- macOS、Linux 和 WSL 使用 POSIX 路径规则，路径部分按大小写敏感比较。

路径规范化只处理文本形式和平台比较规则。`realpath`、符号链接、junction 和其他物理身份判断会在实际文件操作阶段重新确认。

Agent 的路径声明和实际路径也分开处理：相对于 Home 或 ConfigHome 的路径使用当前 Environment 中的对应目录，项目相对路径结合已添加项目解析，绝对路径按所在系统解释。解析失败或目标暂时不可用时，原始声明继续保留，运行时状态显示为不可用或待确认。

项目 lock 中的 Local Skill 来源也按项目根目录解析。写入时尽可能保存使用 `/` 分隔符的相对路径，使项目目录整体移动后仍能定位相同的项目外相对位置；Windows 项目与来源不在同一盘符或来源使用 UNC 地址时保留绝对路径。

## 把 Skill 复制到其他项目

普通安装、更新和来源修复都在能够直接管理目标文件的 Windows、macOS、Linux 或 WSL 用户空间中完成。把 Skill 复制到其他项目时，来源 Environment 先保存完整的 Skill 内容快照，目标 Environment 再根据项目路径和访问状态执行受保护写入。

目标项目必须能够由用户选择的 Windows 用户空间或 WSL 发行版直接管理。目标项目处于跨文件系统状态时，复制流程会要求用户切换到项目文件所在的 Environment，而不会把当前 Environment 自动视为文件系统归属方。

复制流程使用用户选择的目标 Environment 和项目 ID 定位目标，不依赖界面显示路径。目标 Environment 或项目从可选列表中消失后，用户需要重新选择并重新加载目标项目。复制的业务规则见[Skill 生命周期](./skill-lifecycle.md#复制到项目)，内容传递和写入协议见[执行与恢复](./execution-and-recovery.md)。

## 代码如何表示这些位置

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

## 核心规则

1. Skill Deck 默认使用桌面应用所在操作系统的当前用户目录和文件系统。
2. macOS 和 Linux 不提供 Environment 切换；Windows 只有在用户启用 WSL 支持并发现发行版后才提供切换。
3. 全局 Skill 和项目 Skill 都属于当前使用的 Windows、macOS、Linux 或 WSL 用户空间，切换不会移动文件。
4. 添加项目只保存访问记录，移除记录不会删除项目目录和其中的 Skill。
5. 能够看到项目不代表可以写入；受保护写入只能由能够原生访问目标文件的当前系统或 WSL 发行版执行。
6. Agent 的 Skill 读取位置和 Agent 检测位置由 Windows 用户空间与各 WSL 发行版共享，实际路径和检测结果按照当前 Environment 重新解析。
7. 主窗口切换 Windows 或 WSL 不会改变独立窗口已经固定的操作目标。
8. 关闭 WSL 支持不会删除 WSL 中已经保存的项目、Skill 或恢复数据，也不会影响 Windows Environment 中的功能。
9. 在 Windows 与 WSL 之间复制 Skill 时，来源 Environment 先保存内容快照，再由能够直接管理目标项目的 Environment 执行写入。
