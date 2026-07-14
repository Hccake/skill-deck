# WSL 与多环境支持重构设计

本文档定义 Skill Deck 对 Windows Host、WSL 多发行版、macOS Host 和 Linux Host 的统一环境模型，并给出当前 WSL 实现从 legacy/v2 双轨迁移到单一正式架构的方案。

本文档是目标设计，不描述兼容旧版 Tauri command 的过渡协议。Skill Deck 前后端随桌面应用一起发布，当前 WSL 版本尚未发布，因此环境敏感 legacy command 将直接移除。用户已有配置和 skills CLI lock 数据仍必须兼容迁移。

---

## 1. 目标与原则

### 1.1 目标

- Windows 用户可以管理 Host 和多个 WSL 发行版中的 Global 与 Project Skill。
- macOS 和 Linux 继续作为单 Host 环境运行，不展示 WSL 专属 UI。
- Windows 没有安装 WSL 或没有发行版时，产品自然退化为单 Host 环境。
- Host、WSL、Settings、Discover 和安装向导使用同一套 context、mutation、lock 与错误语义。
- 保持与 skills CLI 的目录和 lock 格式兼容，不在 lock 中写入 Skill Deck 环境身份。
- 删除 legacy/v2 双轨，避免界面显示环境与实际操作环境不一致。
- 在满足主要用户场景的前提下，拒绝高成本、低价值的兜底设计。

### 1.2 第一性原则

1. 一个操作必须有且只有一个明确的环境与 scope。
2. 环境和项目身份必须在操作开始时冻结，不能在异步执行中重新读取 UI 全局状态。
3. 所有 Skill 写操作必须由 backend 串行化，frontend 禁用只能改善体验，不能承担数据完整性责任。
4. WSL 是外部运行环境，不是 Skill Deck 管理的资源；Skill Deck 只发现、连接和执行，不负责安装、创建、删除或关闭 WSL。
5. skills CLI 是共享数据协议参与者，不是由 Skill Deck 控制的进程；兼容重点是 lock 路径、字段和未知数据保留，而不是构建其他工具不会遵守的私有锁。
6. Host 与 WSL 的业务语义应统一，底层文件和进程实现不必为了形式对称而强行抽象。
7. 能通过清晰交互约束消除的状态分支，不转化为 backend 队列、恢复或自动关联系统。
8. 产品约束不能代替数据正确性；context 冻结、单写保护、迁移和 lock 冲突检测必须由 backend 保证。

---

## 2. 非目标

本次重构不实现：

- WSL 安装、发行版创建、注销、启动、关闭或重启管理。
- 部署或维护 Linux helper。
- 写操作队列。
- 安装步骤恢复或应用重启后的后台续作。
- Windows CLI、WSL CLI 与 Skill Deck 之间的跨进程强锁。
- 任意文件系统变更的完整 rollback transaction。
- 自动合并 Windows 和 WSL 项目注册表中的同一物理项目。
- 自动在 storage owner 环境中创建、关联或选中 ProjectBinding。
- 通过 symlink、realpath 或文件标识判断跨环境的“同一物理项目”。
- 应用运行期间持续监控 WSL 安装、发行版创建或删除；正常变更在下次启动重新发现。
- 为 automount 完全禁用的 WSL 实现复杂的 tar/UNC 双向 staging fallback。
- 为 Host 和 WSL 建立覆盖全部行为的通用 async EnvironmentDriver。

---

## 3. 统一领域模型

### 3.1 环境与 Context

```rust
pub enum EnvironmentRef {
    Host,
    Wsl { distro_name: String },
}

pub enum ContextScope {
    Global,
    Project { project_id: String },
}

pub struct ContextRef {
    pub environment: EnvironmentRef,
    pub scope: ContextScope,
}
```

`ContextRef` 是所有环境敏感读写操作的唯一身份。项目路径不是跨层身份，只是 backend 根据 `environment + project_id` 解析得到的运行数据。

### 3.2 项目绑定

```rust
pub struct ProjectBinding {
    pub id: String,
    pub native_path: String,
    pub display_name: Option<String>,
    pub order: Option<u32>,
    pub suppress_cross_storage_warning: bool,
}
```

Host 项目配置保存于 Skill Deck Host 配置目录的 `projects.json`。每个 WSL 发行版使用其当前默认用户 Home 下的 `~/.skill-deck/projects.json`。不同发行版的项目配置互相隔离。

项目存储归属是运行时信息，不持久化到 `projects.json`：

```rust
pub struct ProjectStorageInfo {
    pub access: StorageAccess,
    pub owner: Option<EnvironmentRef>,
}

pub enum StorageAccess {
    Native,
    CrossStorage,
    Unsupported,
    Unknown,
}

pub struct ProjectInfo {
    pub binding: ProjectBinding,
    pub storage: ProjectStorageInfo,
}

pub struct AddProjectResult {
    pub project: ProjectInfo,
    pub created: bool,
}
```

`ProjectBinding` 是持久化记录；`ProjectInfo` 是 command 返回给 frontend 的运行时视图，附带 backend 计算的 storage access 和 owner。`projects.json` 只保存 `ProjectBinding`，不得持久化 `ProjectStorageInfo`。`AddProjectResult.created=false` 明确表示规范化注册路径已经存在。

### 3.3 解析后的 Context

所有 backend command 先通过 `ContextResolver`：

```text
ContextRef
   -> 校验环境与项目归属
   -> 获取或刷新 WSL session
   -> 解析 Home 与项目路径
   -> 解析 canonical Skill root
   -> 解析 skills CLI-compatible lock locator
   -> ResolvedContext
```

```rust
pub struct ResolvedContext {
    pub context: ContextRef,
    pub project: Option<ProjectBinding>,
    pub home: ResourceLocator,
    pub skill_root: ResourceLocator,
    pub lock: ResourceLocator,
}
```

install、update、remove、copy、agents 和 settings 不得各自重复解释 project ID、Home 或 lock 路径。

---

## 4. 前端状态所有权

### 4.1 WorkspaceContextStore

当前环境和 scope 只保存在一个 store：

```ts
interface WorkspaceContextState {
  selectedContext: ContextRef;
  pendingEnvironment: EnvironmentRef | null;
  contextRevision: number;
  switchEnvironment(environment: EnvironmentRef): Promise<void>;
  selectGlobal(): void;
  selectProject(projectId: string): void;
}
```

`contextRevision` 在任何已提交的 context 变化时增加，包括环境切换成功、`selectGlobal()` 和 `selectProject()`；它不是只用于环境切换的计数器。异步协调逻辑可以用捕获的 revision 判断用户是否已经改变选择。

删除：

- `selectedContext: string`
- `selectedContextRef` 双份表达
- `hasExplicitContext`
- 独立 `selectedEnvironment`
- legacy `projects: string[]`
- `getExplicitContextForScope()`
- 所有 legacy/v2 dispatch

程序每次启动默认进入 Host Global。Windows 会发现 WSL，但不会自动连接或启动任何发行版。

这是有意的产品选择：不恢复上次 WSL context，避免应用启动时隐式唤起发行版。用户选择 WSL 后才建立 session。

### 4.2 环境切换事务

环境切换必须按以下顺序执行：

1. 若 `pendingEnvironment` 非空，拒绝第二次切换；不排队，也不实现 latest-request-wins。
2. 设置 `pendingEnvironment`，保留当前 `selectedContext` 和页面内容。
3. 在 `try` 中执行连接：Host 直接进入加载；WSL 执行连接探测。
4. 加载目标环境项目配置；全部成功后一次性提交目标环境 Global context 并增加 `contextRevision`。
5. 在 `catch` 中保留原 context，只记录目标环境错误并提供重试。
6. 在 `finally` 中无条件清除 `pendingEnvironment`，确保失败后选择器恢复可用。

切换期间所有环境选择器禁用。选择器继续显示已提交环境，并在相邻状态区域显示“正在连接 <目标环境>”，避免选择器与页面内容指向不同环境。不提供取消连接；连接由短 timeout 收口，失败后允许重试。

Settings、Skills、Discover 使用同一环境选择语义。Settings 中切换环境同样会将应用切换到目标环境 Global。

### 4.3 EnvironmentStore 与 ProjectStore

`EnvironmentStore` 只维护发现结果和连接错误，不保存当前选择：

```ts
interface EnvironmentState {
  environments: EnvironmentInfo[];
  discoveryError: AppError | null;
  errorsByEnvironment: Record<EnvironmentKey, AppError | null>;
  discover(): Promise<void>;
  connect(environment: EnvironmentRef): Promise<void>;
}
```

Backend discovery command 返回环境列表与非阻断错误，而不是用空列表吞掉失败：

```rust
pub struct EnvironmentDiscoverySnapshot {
    pub environments: Vec<EnvironmentInfo>,
    pub error: Option<AppError>,
}
```

`Available` 只表示环境已发现且可以选择，不额外引入 `Discovered / Ready` 等用户不可见状态。WSL 是否已建立 session 是 backend 内部信息；只有连接和项目加载成功后，WorkspaceContextStore 才会提交该环境。

WSL 未安装或没有发行版属于正常 Host-only 状态，不产生错误。`wsl.exe` timeout、权限或执行失败属于 discovery failure：Host 仍然可用，界面显示非阻断提示和重试入口。

`ProjectStore` 按环境隔离数据：

```ts
interface ProjectState {
  projectsByEnvironment: Record<EnvironmentKey, ProjectInfo[]>;
  loadStateByEnvironment: Record<EnvironmentKey, LoadState>;
  refresh(environment: EnvironmentRef): Promise<void>;
  add(environment: EnvironmentRef, nativePath: string): Promise<AddProjectResult>;
  remove(environment: EnvironmentRef, projectId: string): Promise<void>;
}
```

异步 action 必须捕获显式环境，不允许在请求完成时重新读取全局 selected context。

添加项目时，在打开系统目录选择器前捕获环境；完成后不自动切换 context。若 `AddProjectResult.created=false`，提示“项目已在当前环境中”。跨存储 banner 和 owner 操作只读取 `ProjectInfo.storage`，frontend 不再根据路径猜测。

移除项目必须由统一 coordinator 完成，而不是由 Skills Sidebar 或 Settings 各自补救：

1. 打开确认框时捕获 `environment + projectId + contextRevision`。
2. 文案明确说明只解除 Skill Deck 注册，不删除磁盘目录。
3. backend 移除成功后，只有当前 `selectedContext` 仍指向该项目且 `contextRevision` 未变化时，才切回该环境 Global。
4. 用户在操作期间已经切换 context 时，不覆盖用户的新选择。
5. Skills Sidebar 与 Settings 复用同一确认和协调逻辑。

### 4.4 Skill 与 Settings 缓存

Skill 数据按 `ContextKey` 隔离：

```ts
interface ContextSkillSnapshot {
  skills: SkillListItem[];
  agents: AgentInfo[];
  loading: boolean;
  error: AppError | null;
  requestId: number;
}
```

规则：

- context 激活时始终刷新，以兼容外部 skills CLI 修改。
- 缓存只用于刷新期间保持画面，不作为长期 authoritative state。
- 请求只写入其启动时的 `ContextKey`。
- 同一 key 只有最新 `requestId` 可以提交结果。
- 当前页面按 `selectedContext` 派生，旧环境请求无法覆盖当前环境。
- mutation 成功后只失效实际受影响的 context。

Global 和 Project 使用两份独立 snapshot。Global 页面只读取 `environment/global`；Project 页面组合展示 `environment/global` 与 `environment/project:<id>`。不得把 Global Skill 复制到 Project cache，也不得用一份可变数组同时表达两个 scope。

默认 Agent 设置按 `EnvironmentKey` 隔离：

```ts
interface AgentDefaultsSnapshot {
  agents: AgentInfo[];
  defaults: DefaultTargetAgents;
  loadState: 'idle' | 'loading' | 'ready' | 'error';
  loadRequestId: number;
  saveRequestId: number;
  saving: boolean;
  error: AppError | null;
}

interface SettingsState {
  agentDefaultsByEnvironment: Record<EnvironmentKey, AgentDefaultsSnapshot>;
  loadAgentDefaults(environment: EnvironmentRef): Promise<void>;
  saveAgentDefaults(
    environment: EnvironmentRef,
    defaults: DefaultTargetAgents,
  ): Promise<void>;
}
```

规则：

- Component 必须把当前环境显式传给 load/save action。
- Action 启动时捕获 `EnvironmentKey + requestId`，完成时不得重新读取 WorkspaceContextStore。
- 每个环境独立执行 latest-request-wins；Host、Ubuntu 和 Debian 的请求互不覆盖。
- save 失败只回滚它捕获的环境和 request，不能恢复另一环境后来加载的数据。
- frontend 在调用 backend 前立即设置对应 snapshot 的 `saving=true`，关闭 mutation event 到达前的重复点击窗口。
- 加载完成前禁止保存；macOS 和 Linux 自然只存在 Host snapshot，不增加平台分支状态。

### 4.5 Dialog 与 Wizard

- Dialog 打开时捕获不可变 `ContextRef`。
- 提交时不重新读取全局 context。
- Copy 捕获 source context 和全部 target context。
- 添加项目在打开目录选择器前捕获目标环境。
- Wizard URL 保存完整 context；主窗口随后切换环境不影响 Wizard。
- Wizard 在确认页持续展示目标环境、Global/Project scope 和项目名。

---

## 5. 正式 Command Surface

所有环境敏感 command 都显式接收 context 或 environment，不再保留版本后缀。

### 5.1 Skill 查询

```text
list_skills(context)
list_agents(context)
get_skill_agent_details(context, skill_name)
fetch_available(context, source)
check_overwrites(context, request)
check_updates(context)
```

### 5.2 Skill 写入

```text
install_skills(context, request)
update_skill(context, skill_name)
update_skills_batch(context, skill_names)
remove_skill(context, request)
manage_skill_agents(context, request)
cleanup_duplicate_agent_copies(context, request)
copy_skill_to_projects(source_context, target_contexts, request)
get_default_target_agents(global_context)
save_default_target_agents(global_context, defaults)
```

### 5.3 环境与项目

```text
list_environments()
connect_environment(environment)
map_environment_path(environment, host_path)
list_environment_projects(environment)
add_environment_project(environment, native_path)
remove_environment_project(environment, project_id)
set_project_cross_storage_warning(environment, project_id, suppressed)
retry_host_project_migration()
```

`list_environments()` 返回 `EnvironmentDiscoverySnapshot`。`list_environment_projects()` 返回 `Vec<ProjectInfo>`；`add_environment_project()` 返回 `AddProjectResult`；`set_project_cross_storage_warning()` 返回更新后的 `ProjectInfo`。`retry_host_project_migration()` 只用于初始化迁移失败后的显式恢复，不向 frontend 暴露 legacy 数据格式。

### 5.4 删除的 legacy API

删除所有使用以下参数模式的环境敏感 command、frontend wrapper、bindings 和测试 mock：

- `scope + projectPath`
- `ListSkillsParams` legacy scope
- `Option<String>` project path
- 无 context 的 install/update/remove/manage/copy/default Agent API
- legacy `add_project(path)` 与 `remove_project(path)`
- 业务 command 的 `_v2` 名称

普通应用配置 command 可以保留，但不得继续承载项目列表或环境敏感 Skill 设置。

---

## 6. Backend 模块边界

不建立通用大而全 driver。保留操作级 service：

```text
Tauri Command
    -> ContextResolver
    -> Operation Service
       - skill_query
       - install
       - update
       - remove
       - manage_agents
       - copy
       - project_registry
    -> Host implementation | WSL implementation
```

Command 只负责参数反序列化、获取 Tauri State 和调用 service。环境分支可以存在于 operation service 内部，但不得绕过统一 context、mutation、lock 和错误模型。

现有 Host 与 WSL 文件执行逻辑优先保留。只有在 legacy 双轨删除并通过测试后，才抽取已经证明存在的重复逻辑。

跨 operation 的基础设施只保留四个窄边界：

```text
ContextResolver             解析环境、项目、Home、Skill root 和 lock locator
SingleMutationController    全局单写、revisioned snapshot、取消信号和状态事件
LockRepository              schema-aware 无损 lock 事务与原子 IO
EnvironmentRegistry         WSL session 缓存、失效和 runtime status event
```

不把这些职责重新包装为通用 `EnvironmentDriver`，也不让 command 自行实现 lock 写入或环境失效恢复。

---

## 7. 全局 Mutation 模型

### 7.1 单写规则

所有会修改 Skill、lock、Agent 目录或项目注册表的操作使用同一个 `SingleMutationController`：

```text
Install
Update
BatchUpdate
Remove
Copy
ManageAgents
DuplicateCleanup
SaveAgentDefaults
AddProject
RemoveProject
UpdateProjectPreference
ProjectMigration
```

- 同一时间只允许一个写操作。
- 不排队，不恢复 step。
- 第二个请求立即返回 `MutationBusy`。
- read、环境切换和浏览不获取 mutation guard。
- mutation 使用启动时捕获的 `ContextRef`。
- 项目注册表 read-modify-write 在 backend 串行执行。

### 7.2 Mutation 状态

```rust
pub struct ActiveMutation {
    pub id: Uuid,
    pub kind: MutationKind,
    pub context: ContextRef,
    pub phase: MutationPhase,
    pub progress: Option<MutationProgress>,
    pub cancelable: bool,
}

pub struct MutationProgress {
    pub subject: Option<String>,
    pub current: Option<u32>,
    pub total: Option<u32>,
}

pub struct MutationSnapshot {
    pub revision: u32,
    pub active: Option<ActiveMutation>,
}
```

`revision` 使用单次进程内的 `u32` 单调序号，以保持 Specta/TypeScript number 绑定无损；frontend 只在同一次应用运行中比较 revision，实际状态变化次数远低于其上限。

backend 在 mutation start、phase update 和 finish 时增加 revision，并通过单一 Tauri event 发布完整 `MutationSnapshot`：

```text
mutation-state-changed
```

`get_active_mutation()` 同样返回 `MutationSnapshot`。frontend 只接受 revision 高于当前值的 event 或查询结果，避免 focus 查询与 finish event 乱序后重新显示已完成操作。每个窗口启动和重新获得焦点时查询一次快照，移除持续 2 秒轮询。短项目操作的状态栏延迟约 300ms 展示，避免闪烁。

backend 不发送用户可见的自然语言状态。frontend 根据 `kind + phase` 使用 i18n 文案渲染状态；Skill 名称、当前数量和总数等动态数据通过 `MutationProgress` 传递。`subject` 只承载名称或标识符，不承载待翻译句子。

### 7.3 取消语义

```text
Preparing      默认不可取消；实现消费 CancellationSignal 时才显式开放
Acquiring      仅真实消费 CancellationSignal 时可取消
Materializing  仅真实消费 CancellationSignal 时可取消
Committing     永不可取消
Finishing      永不可取消
```

- `SingleMutationController::begin()` 必须创建 `Preparing + cancelable=false`，不能让 UI 根据 phase 猜测是否可取消。
- phase、progress 和 cancelable 必须通过一次 transition 原子更新并发布一个 snapshot，不能先切 phase 再单独更新 cancelable。
- Host 与 WSL 只有实际支持取消的 acquisition 才接收并消费 `CancellationSignal`。
- 可取消的 Git 与 WSL 子进程必须被真实终止并清理 staging。
- 进入 `Committing` 前必须在同一次 transition 中设置 `cancelable=false`。
- 尚未实现安全取消的 Host 操作必须显示不可取消，不能提供虚假按钮。
- Wizard 只有观察到更高 revision 且 `active=None` 后，才能在取消流程中关闭。
- 强制结束应用后不恢复 operation，下一次启动重新扫描文件和 lock。

Update All 不是 frontend 对单项 update command 的并发循环。frontend 只发送一次：

```text
update_skills_batch(context, all_names)
```

backend 在一个 mutation guard 内按 source/ref 分组并完成全部更新。批次执行期间第二个写请求仍立即返回 `MutationBusy`，不进入队列。

---

## 8. Lock Repository 与 skills CLI 兼容

### 8.1 路径兼容

- Global lock 使用 skills CLI 约定的 `XDG_STATE_HOME/skills/.skill-lock.json`，未设置时回退 `~/.agents/.skill-lock.json`。
- Project lock 使用 `<project>/skills-lock.json`。
- 读取时保留旧 `<project>/.agents/.skill-lock.json` 的迁移支持。
- Skill Deck 不在 lock 中写入 EnvironmentRef 或 ProjectBinding ID。

### 8.2 三个领域边界

Lock 兼容逻辑只拆为三个小边界，不建立通用 repository framework。

`LosslessLockDocument` 以原始 `serde_json::Value` 持有整个文档，只校验 root 和 `skills` 为 JSON object。它负责保留未知 root 字段、未知 entry 字段和未修改的 Skill entry。

`LockSchemaAdapter` 明确认识 Global v3 和 Project v1 的字段集合，并负责 legacy Global-to-Project 转换。替换 entry 时必须：

1. 从原始 entry clone 当前 object。
2. 删除目标 schema 的全部已知字段。
3. 插入新值中实际存在的已知字段。
4. 保留其余未知字段。

不能使用“replacement 中缺少的字段全部从旧 entry 补回”的通用 merge；该做法会让已经清除的 `pluginName`、`subagents` 或 `remoteHash` 等已知可选字段残留。

`LockRepository` 组合现有 `EnvironmentLockIo` 与 schema adapter：

```rust
pub enum LockSchema {
    Global,
    Project,
}

pub struct LockTarget {
    pub primary: ResourceLocator,
    pub legacy: Option<ResourceLocator>,
    pub schema: LockSchema,
}

impl LockRepository {
    pub async fn begin(
        &self,
        target: LockTarget,
        entries: &[String],
    ) -> Result<LockTransaction, AppError>;
}
```

Transaction 支持替换和删除目标 entry、更新 `defaultTargetAgents` 等明确的 root 字段，并对这些目标分别捕获快照。Host 与 WSL 共用 document、schema 和冲突语义，仅 ResourceLocator 与底层 IO 不同；不向 WSL 部署 helper。

### 8.3 Lossless optimistic commit

Host 与 WSL 使用相同 commit 流程：

1. 按 canonical-first 规则读取并规范化初始文档。
2. 保存目标 Skill entry 或目标 root 字段快照。
3. 准备和 materialize Skill 文件。
4. 提交前按相同规则重新读取并规范化最新文档。
5. 只比较本次触碰的 entry 或 root 字段。
6. 合并其他 Skill、未知字段和不相关 root 字段的外部修改。
7. 目标值被外部修改时返回结构化 `LockConflict`。
8. 使用同目录临时文件原子写入 canonical lock。

即使操作期间外部 CLI 新建了 canonical lock，Repository 也以最新 canonical 文档为基准；目标快照一致时合并，不一致时冲突。install、update、batch update、remove、copy 和 Agent 默认设置都必须通过 Repository 写 lock，command 不得直接读改写完整 JSON。

不实现其他 CLI 不会遵守的跨进程强锁。若外部 CLI 恰好在 materialization 与 lock commit 之间修改相同 Skill，Skill Deck 返回结构化错误，说明文件可能已变化但 lock 未提交，并要求刷新后重试；不增加完整文件回滚或恢复队列。

### 8.4 持久化数据迁移

删除 legacy command 不得删除：

- `config.projects` 到 Host `projects.json` 的一次性迁移。
- project legacy lock 到 `skills-lock.json` 的读取与迁移。
- XDG global lock fallback。
- 未知字段 lossless preservation。

迁移实现是内部数据兼容模块，不对 frontend 暴露 legacy API。迁移时机必须明确：

```rust
pub enum ProjectMigrationState {
    NotNeeded,
    Succeeded,
    Failed { error: AppError },
}
```

- Host `config.projects` 迁移在应用初始化阶段、frontend 开始读取项目之前执行一次。
- 启动时的首次迁移发生在交互式写操作开放之前，不需要获取 mutation guard。
- 初始化结果保存在 backend `ProjectMigrationState`。迁移失败时保留原 config，应用可以继续启动，但 Host 项目查询返回 `ProjectMigrationFailed`，项目区域显示可恢复错误。
- 用户点击重试时调用 `retry_host_project_migration()`；该 command 使用 Host Global `ContextRef`、`SingleMutationController` 和 `MutationKind::ProjectMigration`，成功后更新 migration state 并刷新 Host 项目。
- 普通 query command 不得触发项目注册表写入、备份或 config 改写。
- legacy project lock 在 read 时只作为 fallback，不写入；首次受 mutation guard 保护的相关写操作提交时，再写入 canonical `skills-lock.json`。
- legacy lock 转换必须直接操作无损 JSON：移除旧 schema 已知字段、映射到 Project v1 已知字段，并保留未知 root 与 entry 字段。不得先反序列化为 `SkillLockFile` 再序列化为 `LocalSkillLockFile`。
- canonical 写入成功后 legacy 文件保持不动；Skill Deck 不做双写或主动删除。

---

## 9. WSL 生命周期与 Session

### 9.1 生命周期边界

- Windows 每次程序启动执行一次 `wsl.exe --list --quiet`。
- 没有 WSL 或没有发行版时只返回 Host。
- WSL 未安装和无发行版是正常结果；timeout、权限或执行失败返回独立 discovery error，不伪装成 Host-only。
- 只有用户选择发行版时才连接；连接命令可以自然唤起 stopped distro。
- 不提供 WSL 启停、创建、删除或隐藏按钮。
- 运行中失效的发行版标记为 unavailable，并提供重试。
- 下次程序启动重新发现系统发行版列表。
- 应用运行期间创建或删除发行版，不做持续监控；用户重启应用后刷新列表。

### 9.2 Session

```rust
pub struct WslSession {
    pub distro_name: String,
    pub user: String,
    pub uid: u32,
    pub home: String,
    pub xdg_state_home: Option<String>,
    pub config_home: String,
    pub environment: AgentEnvironment,
    pub git_available: bool,
}
```

每次显式选择 WSL 环境时重新执行轻量探测并更新缓存。普通操作复用 session。只读操作，以及“读取后仅执行一次最终原子写”的操作，遇到业务脚本尚未启动的 session 类错误时，清除缓存并只自动重连一次。install、update、remove、copy 和 Agent 管理等多脚本 mutation 不自动重放整个操作；它们清除失效 session、返回 `EnvironmentUnavailable`，由用户显式重试，避免重复 materialize、删除或 lock 提交。

### 9.3 运行时失效协调

WSL runtime failure 由 backend `EnvironmentRegistry` 统一收口：

```rust
pub struct EnvironmentRuntimeEvent {
    pub environment: EnvironmentRef,
    pub status: EnvironmentStatus,
    pub error: Option<AppError>,
}
```

- 只有重连策略耗尽后的最终 `EnvironmentUnavailable` 才失效 cached session 并发布 unavailable event。
- `WslCommandFailed`、`LockConflict`、参数错误等业务失败不改变发行版状态。
- frontend 只保留一个 App 级 monitor 监听 runtime event，并更新 `EnvironmentStore`；页面和 Dialog 不各自捕获环境失效。
- 当前 context 和已缓存内容继续显示，不自动切换 Host；写操作保持禁用并提供当前发行版的 Retry。
- Retry 只重新连接被选中的发行版。成功后更新状态并刷新当前 context，不扫描或启动其他发行版。
- 不增加 polling，也不把 runtime event 扩展为 WSL 生命周期管理。

---

## 10. WSL Protocol 与路径映射

### 10.1 唯一 Command Runner

各业务模块不得直接创建 `wsl.exe`。统一使用：

```rust
pub struct WslCommandRequest {
    pub session: WslSession,
    pub script: &'static str,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
    pub timeout: Duration,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
    pub cancellation: Option<CancellationSignal>,
}
```

Runner 必须：

- 用户值只作为 positional argument。
- 流式读取 stdout/stderr，并限制输出大小。
- 超时、取消或输出超限时终止 child。
- 所有退出路径都 await 或 abort stdin writer。
- 返回 exit code 与截断后的 stderr。
- 使用版本号和 NUL-delimited records。
- 拒绝未知协议版本。
- 使用 trap 清理 staging 和临时文件。

默认 stdout 上限为 16 MiB，stderr 上限为 1 MiB。确需更大输出的调用显式提高限制，不允许无限收集。

### 10.2 路径映射

系统目录选择器返回的路径按目标管理环境处理：

| 目标管理环境 | 输入路径 | 处理 |
| --- | --- | --- |
| Host | Windows drive / Windows UNC | 规范化后直接保存 |
| Host | 任意有效 WSL UNC | 保留为规范化 Host UNC，标记 CrossStorage |
| WSL Distro A | Distro A 的 WSL UNC | 转换为 Linux native path |
| WSL Distro A | Windows drive path | 通过 Distro A 的 `wslpath -u` 转换，标记 CrossStorage |
| WSL Distro A | 普通 Windows network UNC | 尝试通过 Distro A 的 `wslpath -u` 转换；失败则返回 `StorageMappingUnsupported` |
| WSL Distro A | 其他 Distro 的 WSL UNC | 拒绝，并提示切换对应发行版 |

- Host staging 目录同样通过 `wslpath` 探测。
- 自定义 automount root 由 `wslpath` 自然支持。
- automount 被禁用或 staging 不可访问时返回 `StorageMappingUnsupported`。
- 不使用运行时硬编码 `/mnt/<drive>`。

Backend 根据实际映射结果计算 `ProjectStorageInfo`，frontend 不再使用 `/mnt/c` 或 UNC 正则猜测存储归属。

项目去重只保证同一环境注册表中的规范化注册路径去重：

- Windows Host key 统一 separator、drive letter 和 `\\wsl$` / `\\wsl.localhost` alias。Windows drive 与普通 UNC 按大小写不敏感比较；WSL UNC 仅 alias 与发行版名大小写不敏感，发行版后的 Linux path remainder 保持大小写敏感，避免合并 Linux 中两个不同目录。
- macOS Host、Linux Host 和 WSL key 使用 POSIX lexical normalization，并按大小写敏感比较；不假设底层文件系统一定大小写敏感。
- 不通过 realpath 或 symlink 自动合并，也不跨环境去重。

### 10.3 性能约束

- 启动只发现发行版，不连接全部 WSL。
- 只加载当前环境数据。
- WSL Skill 扫描一次进程批量返回，禁止每个 Skill 启动一个 `wsl.exe`。
- context 切换显示缓存并后台刷新。
- 大量项目通过独立滚动区展示，超过 50 项时启用虚拟化或 `content-visibility: auto`。

---

## 11. 产品交互与错误恢复

### 11.1 环境展示

- macOS、Linux 或 Windows 无 WSL 时不显示环境选择器。
- Windows 检测到 WSL 后平铺 Host 与各发行版。
- discovery failure 时即使只有 Host，也在环境区域显示非阻断错误和重试；正常 Host-only 不显示该区域。
- 环境选择器、Global 和添加项目固定；项目区域独立滚动且不折叠。
- 环境连接期间保留原页面；成功后进入目标 Global，失败后保留原 context。
- 不提供环境删除或隐藏按钮。

### 11.2 添加项目

1. 从当前环境发起目录选择。
2. Backend 将路径映射到目标环境。
3. Host 接受 Windows path 和 WSL UNC；WSL 将自身 UNC 或 Windows path 映射为 native path。
4. 其他发行版 UNC 被拒绝。
5. 重复规范化注册路径返回 `created=false` 和已有 `ProjectInfo`，并提示用户项目已存在。
6. 不解析 symlink，也不承诺物理路径去重。
7. 添加成功不自动选中项目。

### 11.3 跨存储

允许 Windows 管理 WSL 项目，也允许 WSL 管理 Windows 项目。同一物理项目可以分别注册在 Host 与 WSL 项目列表中。

跨存储项目首次进入时显示非阻断 banner：

- 说明当前管理环境和存储 owner。
- 提醒不要同时从两个系统修改。
- owner 环境当前可用时，提供“切换到 <owner>”操作；该操作只切换到 owner Global。
- 支持按 ProjectBinding 关闭后续提醒。

不自动在 owner 环境注册、关联或选中项目。owner 不可用时不显示切换操作，由环境选择器承担重试。跨存储不在每次写操作前重复确认；外部 Windows CLI 与 WSL CLI 的并发风险只提示，不阻止。

### 11.4 写操作体验

- mutation 活跃时立即禁用所有写按钮。
- 允许环境切换、项目浏览和 Skill 阅读。
- 第二个写请求返回 `MutationBusy`，frontend 聚焦现有状态栏。
- 状态栏显示操作实际所属环境、scope 和 phase。
- 只有真实可取消阶段显示取消按钮。

允许浏览是有意的产品选择：Git acquisition 可能持续较长时间，阻塞整个应用会明显降低体验。正确性由冻结的 mutation context、按 context 隔离的缓存和 backend 单写保护保证，不依赖用户停留在原页面。

### 11.5 结构化错误

```text
MutationBusy
MutationCancelled
LockConflict
EnvironmentUnavailable
EnvironmentDiscoveryFailed
ProjectNotFound
ProjectMigrationFailed
StorageMappingUnsupported
```

`LockConflict` 可携带 `filesMayHaveChanged`，表示 Skill 文件可能已经 materialize、但 lock 未提交；不再使用边界重复的 `ExternalWriteDetected`。UI 根据错误类型提供重试、刷新、切换存储 owner 或移除失效 binding。原始 stderr 只写日志，不直接作为主要用户提示。

### 11.6 无障碍

- 项目选择按钮与操作菜单为兄弟元素，不嵌套交互控件。
- Hover 操作同时支持 `focus-within`。
- 环境选择器具有 label。
- 环境连接状态和 discovery error 使用 `aria-live="polite"`；错误出现后“重试”操作必须可通过键盘聚焦。
- mutation 状态使用 `aria-live="polite"`。
- spinner 和转场支持 `prefers-reduced-motion`。
- 删除项目后焦点移动到相邻项目或 Global。
- Skills Sidebar 与 Settings 的项目移除都必须使用同一确认交互。
- 长发行版名、项目名和路径截断并提供 tooltip。
- 移除项目必须明确仅解除注册，不删除磁盘目录。

---

## 12. 实施计划拆分

本设计不作为一个大爆炸计划执行。前五个计划建立 WSL 基础、Context 和 canonical command；实现后的全局审查发现 mutation、Settings、lock 和运行时恢复仍有架构缺口，因此增加第六个架构闭环计划。每个阶段先补失败测试，完成后当前分支必须保持可构建。

### 12.1 Backend 数据安全基础

在保留现有 v2 command 名称的前提下，引入 revisioned MutationSnapshot、ProjectInfo、AddProjectResult、ProjectMigrationState、显式迁移重试、统一 lock transaction、typed error 和 backend 单写入口。同步调整当前 wrapper 和 Store 适配新返回结构，确保计划结束时当前 UI 可构建，但暂不迁移统一 Context。

### 12.2 WSL Runtime 与路径

实现 discovery error 区分、session 重连、唯一 WslCommandRunner、输出限制、完整 child/writer cleanup、`wslpath` 映射、完整目标环境路径矩阵和 storage owner 计算。此计划结束时 Host 与 WSL 的 ProjectInfo 都由 backend 提供可信 storage 信息。

### 12.3 Context 与交互闭环

引入单一 WorkspaceContextStore，拆分 EnvironmentStore 与 ProjectStore，实现 single-flight 环境切换、统一项目移除 coordinator、Global/Project 双 snapshot cache，并迁移 Settings、Skills、Discover、Dialog 和 Wizard。此计划继续调用已有 v2 wrapper，但所有操作已经使用显式 ContextRef 和 ProjectInfo。

### 12.4 Canonical Command Cutover

引入 ContextResolver，将 v2 command 重命名为正式 API。frontend 全部迁移后删除 legacy command、wrapper、bindings、参数类型和 mock，重新生成并 patch bindings；此计划结束时业务代码不得再包含 legacy/v2 dispatch。

### 12.5 产品恢复与真实验证

完成环境 pending 状态、统一项目移除确认、context indicator、跨存储 banner、错误恢复、焦点、键盘和 reduced motion。执行全平台和双发行版真实集成测试，并用 GitNexus 核对最终影响范围。

### 12.6 架构闭环

此计划不与前五个计划重新交叉重构，按以下顺序执行：

1. **基线契约测试**：先复现 Update All 并发、虚假取消、Settings 跨环境回滚、lock 数据损失和 WSL runtime 状态滞留。
2. **Mutation contract**：安全默认值、原子 transition、真实取消能力和单请求 batch update。
3. **Lossless Lock Repository**：先建立 CLI fixture，再按 install、update/batch、remove、copy、Agent defaults 顺序迁移；该阶段单独审查，不混入 Store 修改。
4. **Context-keyed Settings**：按环境 snapshot、request 隔离和保存回滚，并把 Agent defaults 写入接到 Repository。
5. **Runtime failure coordination**：EnvironmentRegistry event 与唯一 App monitor。
6. **遗留清理与验收**：删除 direct lock writes、frontend batch 并发、全局 Settings slot 和页面级环境恢复。

不使用 feature flag、双写或长期兼容层。开发过程中允许逐 command 迁移，但所有 lock consumer 未迁移完成前不得作为可发布版本验收。

---

## 13. 测试矩阵与验收

### 13.1 测试矩阵

| 维度 | 必测场景 |
| --- | --- |
| 平台 | Windows 无 WSL、Windows + WSL、macOS Host、Linux Host |
| 发行版 | 单发行版、两个发行版、stopped distro、无 WSL、无发行版、discovery timeout、连接失败、运行中删除 |
| 存储 | Windows native、macOS/Linux POSIX、WSL native、Host 管理 WSL UNC、WSL 管理 Host、Windows network UNC、其他发行版 UNC 拒绝、DrvFS、自定义 automount、中文和空格 |
| Scope | Global、Project |
| 操作 | list、install、update、batch、remove、copy、Agent 管理、default settings |
| 并发 | 环境切换 single-flight、删除期间切换 context、mutation snapshot 乱序、重复点击、跨窗口写、浏览时写、单请求 batch update、同 entry CLI 冲突、不同 entry CLI 修改、Settings 跨环境乱序与回滚 |
| 取消 | 默认不可取消、真实消费 CancellationSignal 后开放、Preparing、Acquiring、Materializing、Committing、关闭 Wizard、应用重启 |
| 迁移 | 启动期旧 config projects、迁移失败状态与手动重试、read 不写入、首次 mutation 迁移旧 project lock、XDG lock、未知 root/entry 字段、已知可选字段清除 |
| 恢复 | 最终 EnvironmentUnavailable、业务错误不误标 unavailable、保留当前 context、选中发行版 Retry、App 级唯一 monitor |
| UI | 首次启动、Host-only、discovery 失败重试、跨页面切换、失败回滚、统一移除确认、50+ 项目列表、键盘操作、异步状态播报 |

真实 Windows integration 环境至少配置两个普通测试发行版，不使用 `docker-desktop`。发布前必须实际运行 stopped distro、双发行版隔离、DrvFS/UNC 和非 ASCII 路径测试。

### 13.2 验收标准

- 当前 frontend 不调用任何 legacy environment-sensitive command。
- 业务代码不存在 `hasExplicitContext` 和 legacy/v2 dispatch。
- 业务 command 不再使用 `_v2` 命名。
- 所有 Skill 操作显式接收 `ContextRef`。
- 所有写操作受 backend mutation controller 保护。
- mutation 默认不可取消，phase、progress 和 cancelable 通过原子 transition 发布；Update All 只发起一个 backend batch mutation。
- 环境、项目、Skill 和设置缓存不会跨 context 污染。
- Agent defaults 使用按 EnvironmentKey 隔离的 snapshot、request ID 和 rollback。
- 项目 command 返回带 backend storage 信息的 `ProjectInfo`，新增结果通过 `created` 明确区分重复注册。
- Project 页面明确组合当前环境的 Global 与 Project 两份 snapshot。
- 异步旧结果不能覆盖当前页面。
- 环境切换期间不能发起第二次切换，失败后仍停留在原 context。
- 普通 query command 不触发项目配置迁移写入。
- 启动迁移失败后可以通过受 mutation controller 保护的显式 command 重试。
- Host 与 WSL 的项目路径遵守目标环境映射矩阵，不自动解析 symlink 或关联 owner binding。
- mutation 用户可见状态全部由 frontend 根据结构化 kind、phase 和 progress 本地化。
- Host 与 WSL 的取消按钮具有真实语义。
- skills CLI 能继续读取 GUI 写入的 lock，未知字段不丢失。
- 所有 lock 写入通过 schema-aware LockRepository；已知可选字段可以被正确清除，legacy 转换不做 typed round-trip。
- 最终 WSL runtime failure 由 EnvironmentRegistry 发布并由唯一 App monitor 更新；业务错误不改变环境状态。
- Windows 无 WSL、macOS 和 Linux Host 功能不退化。
- 不部署 helper，不管理 WSL 生命周期。
- 不实现队列、恢复安装、跨进程硬锁或完整文件回滚。
- Rust tests、frontend tests、lint、build、Windows MSVC 和真实 WSL integration 全部通过。
- `git diff --check` 通过，GitNexus `detect_changes` 只包含预期模块和执行流。

### 13.3 验证与发布门禁

每个阶段先运行直接相关的 focused tests；架构闭环完成后必须运行完整自动化验证：

```bash
rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
rtk cargo test --manifest-path src-tauri/Cargo.toml
rtk cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
rtk vfox exec nodejs -- pnpm test --run
rtk vfox exec nodejs -- pnpm lint
rtk vfox exec nodejs -- pnpm build
```

bindings 必须重新生成并检查 diff。完成代码审查前执行 GitNexus `detect_changes(scope: "compare", base_ref: "main")`，确认 lock 的 CRITICAL 影响只覆盖预期 command 和执行流。

自动化测试不能替代真实平台验收：

- Windows 无 WSL：应用正常启动，只显示 Host，WSL 不可用不阻断 Host 功能。
- Windows 有多个发行版：验证 stopped distro 首次访问、多发行版隔离、当前发行版失效和 Retry。
- Windows Host 与 WSL：分别验证 install、batch update、remove、copy 和 Agent defaults。
- skills CLI：分别从 Windows 与 WSL CLI 交替读写 GUI 使用的 Global 和 Project lock。
- macOS 与 Linux：验证 Host-only 启动、项目、Skill 和 Agent defaults，不出现 WSL UI 或探测副作用。

在非 Windows 环境中可以完成 Rust/frontend 自动化、Host 行为和 WSL protocol fixture，并标记“代码实现完成”。只有真实 Windows 无 WSL、多发行版和 CLI 互操作验收完成后，才能标记“Windows 发布验收完成”。不能用 mock、Linux WSL 会话或编译成功替代该结论。
