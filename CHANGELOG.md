# Changelog

All notable changes to Skill Deck will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **WSL 集成改为按需启用** — Windows 上的 WSL 集成现在默认关闭，应用只有在用户启用后才会发现和使用 WSL Environment；从 `1.7.0-beta.1` 或 `1.7.0-beta.2` 升级的用户需要在 `Settings > 通用` 中重新启用，已保存的 WSL 项目和设置保持不变。
- **安装向导窗口协作** — 安装向导打开期间，主窗口保持业务只读并提供返回向导入口；未知 Agent 作为不可用目标保留，用户关闭向导并在 Settings 创建定义后重新发起安装。
- **Agent 管理与安装目标** — Agent 编辑器改为独立子页面；自定义 Agent 只保留创建、编辑和删除，卡片直接提供具名的编辑与删除图标，ID 冲突记录明确说明当前定义未生效；Settings 不再提供单独的默认安装目标页面，每次安装仍可在向导中确认目标。
- **Skill 工作台信息层级** — Global 与 Project 汇总区域随列表吸顶；卡片紧凑展示两行简介、来源元数据、维护诊断和全部关联 Agent，详情按当前目标、上游来源、本地安装、使用关系及安全维护分组。

### Fixed

- **更新检查触发边界** — Automatic 检查按 Context 会话去重，窗口重新获得焦点、页面重挂载和 Context 往返不再重复访问来源；新增或来源身份变化只触发定向检查。
- **更新检查失败抑制** — 需要用户处理的失败不再由 Automatic 反复重试，瞬时失败使用分级退避，GitHub 限流按明确的冷却截止时间处理，Force 不再绕过服务商限流冷却期；清理状态写入失败时会同时保留原有内存与磁盘状态。
- **更新状态反馈** — 未知、未完成、最近有效结论和限流状态分开表达，摘要布局保持稳定，并提供最近尝试、重试时间和可访问的动态反馈。
- **安装向导搜索与恢复** — 安装向导可以在限域权限内搜索 `skills.sh`；主窗口在事件监听失败后仍核对 Backend 状态，并可在重新获得焦点或用户重试时恢复监听。
- **Agent 管理可靠性** — 保存校验不再被后台校验竞态取消；新建 Agent 的并发 ID 冲突显示字段错误并保留草稿；删除预览使用当前 Context，并明确显示无法解析的 Project 影响。
- **Skill 复制来源边界** — 无来源或来源记录异常的 Skill 可以直接复制，不再要求先修复来源；复制到多个项目时，单个目标项目的 lock 或路径异常不会阻断其他项目。

## [1.7.0-beta.2] - 2026-07-27

### Added

- **GitHub 访问凭据** — Git 设置支持验证并保存 Token，用于提高更新检查额度和访问授权仓库。

### Changed

- **Agent 筛选体验** — 支持搜索 Agent、查看匹配数量，并改进组合筛选和空状态。
- **更新检查反馈** — 保留最近一次有效结果，并提供更明确的失败原因和处理入口。
- **未完成操作反馈** — 统一写操作未完成时的提示、重试和文件检查入口。

### Fixed

- **Environment 稳定性** — 刷新或重连失败时保留当前列表和 Context；Windows 后台任务不再弹出终端窗口。
- **跨 Environment 复制** — Skill 准备完成后不再依赖来源 Environment。
- **Eve 目标一致性** — 安装和更新会保持原先选择的 root 或 subagent。
- **安装向导操作** — 修复部分检查、安装和停止操作被错误拒绝的问题。

## [1.7.0-beta.1] - 2026-07-24

### Added

- **新增 Custom Agent 管理功能** — `Settings > Agents` 支持创建、编辑、复制和删除 Custom Agent，并可将其用于安装、默认目标、Detection 和 Skill 管理。
- **新增 Windows/WSL Environment 支持** — Windows 可以在 Host 与多个 WSL 发行版之间切换，并分别管理 Global/Project Context、Project、Skill 和 Agent 状态。
- **新增 Recovery Center** — 写操作无法自动恢复时保留恢复数据，并提供查看、刷新和清理入口。

### Changed

- **重构 Skill 生命周期管理代码** — 安装、更新、移除、Manage Agents 和项目复制改为先生成变更预览，再按 Skill 或项目返回执行结果。
- **调整 Skill 目录内容处理方式** — 安装、更新和复制会保留 `SKILL.md`、scripts、references、assets、metadata、嵌套目录及其他有效文件。
- **调整更新检查和来源维护流程** — 更新检查区分有更新、已是最新、检查失败、来源不可达、信息不足和上游已删除，并支持重新选择来源。
- **调整更新时的 copy 处理方式** — link 和未修改的 copy 随更新同步；存在本地修改的独立 copy 由用户决定是否覆盖。
- **调整 Project Skill 复制流程** — 复制时支持选择目标 Environment，并在该 Environment 中选择目标项目。
- **调整 Agent 解析和分组规则** — 安装、默认目标和 Manage Agents 按实际目录分组；Skill 页面只展示实际能够读取该 Skill 的 Agent。
- **调整安装确认流程** — 确认页固定已选择的 Skill 内容并生成变更预览，准备失败时保留当前步骤并显示原因。
- **调整 Skill 删除流程** — 从 Skill Card 删除时展示通用 Skill 目录和全部 Agent 软连接或副本，确认后完整删除；只调整部分 Agent 时继续使用 Manage Agents，删除范围变化或执行失败时可以重新确认或重试。
- **调整应用更新失败处理** — 更新检查或安装失败后在更新对话框中显示错误，并提供重试入口。

### Fixed

- **修复安装目标状态丢失问题** — 切换安装步骤、重新准备或完成 Custom Agent 配置后，保留已选择的 Agent、adapter target 和安装方式。
- **修复启动偏好恢复时机** — 主窗口和安装向导在首次渲染前应用已保存的主题和语言。

## [1.6.2] - 2026-07-07

### Added

- **Eve 项目支持** — 可识别 Eve 项目，并支持将 Skill 安装到 Eve root agent 或指定 subagent。
- **项目内目标选择** — 安装项目级 Skill 时，可以选择具体的项目内目标，例如 Eve root 或 subagent；确认页会展示实际写入位置。
- **安装目录提示** — 当 Skill 名称包含不适合作为目录名的字符时，确认页会显示实际安装目录，避免安装后目录名与预期不一致。
- **复制 Skill 时保留更新来源** — 将远程安装的 Skill 复制到其他项目时，会尽量保留自动检查更新所需的来源信息；源 Skill 缺少相关信息时，会在复制前给出轻量提示。

### Changed

- **安装确认更贴近实际写入结果** — 覆盖提示、目标分组和完成页文案改为围绕“目标目录”表达，减少 Agent 检测状态和 Skill 写入状态之间的歧义。
- **Eve Skill 的更新和删除范围更明确** — 对安装到 Eve root 或 subagent 的 Skill，更新和删除会按实际目标处理，用户可以更清楚地控制影响范围。
- **Skill 卡片展示完整 Agent 信息** — SkillCard 不再折叠 Agent 标签，会直接展示当前 Skill 关联的全部 Agent。

## [1.6.1] - 2026-06-24

### Added

- **Agent 可用性分组** — 安装和管理 Skill 时，按“可直接使用”“需要单独接入”“额外保留”区分 Agent，不再把所有 Agent 都视为需要写入独立目录的目标。
- **额外保留项清理** — 当 Agent 既能读取通用 Skill 目录，又在自己的目录中保留同名链接或副本时，Skill Deck 会标记该状态，并可在“管理 Agents”中清理对应目录项。
- **Agent 支持扩展** — 新增并校准 Antigravity CLI、AstrBot、Autohand Code CLI、inference.sh、Jazz、Lingma、Loaf、Moxby、Ona、PromptScript、Qoder CN、Reasonix、Zenflow、Terramind、Tinycloud 等 Agent 的目录识别。

### Changed

- **安装目标选择** — 可直接读取通用 Skill 目录的 Agent 不再进入普通接入选择；需要写入独立目录的 Agent 仍由用户显式选择。
- **安装确认与结果展示** — 确认页改为展示安装计划，完成页按“可直接使用”“已单独接入”“额外保留”“已跳过”“失败”分类展示结果。
- **默认接入设置迁移** — 已经可直接使用的 Agent 会从默认接入配置中移除，避免后续安装时重复写入独立目录。
- **项目复制目标处理** — 复制项目级 Skill 时，会根据目标项目中的 Agent 可用性决定写入通用 Skill 目录、独立 Agent 目录或跳过，并返回被跳过的 Agent。

### Fixed

- **重复目录项覆盖** — 对已经存在额外保留项或仅存在独立目录副本的 Agent，管理入口不再直接覆盖原有目录内容。
- **删除范围提示** — 删除通用 Skill 目录中的 Skill 时，如果 Agent 自己目录中仍有链接或副本，会明确提示这些目录项不会随通用目录一起删除。
- **部分 Agent 目录识别** — 校准 Cline、Hermes、Kimi Code CLI 等 Agent 的目录配置和检测逻辑。

## [1.6.0] - 2026-06-02

### Added

- **Zed 支持** — 可识别并管理 Zed 的 Skill 目录，Zed 用户可以和其他编辑器一样安装、同步和维护 Skill。
- **上游已删除状态** — 当已安装 Skill 在原仓库中不存在时，应用会明确标记，并提供删除本地副本、修复来源或继续保留的选择。

### Changed

- **多层级目录 Skill 发现** — 添加 Skill 时支持发现常见集合目录下更深一层的 Skill，适配更多仓库组织方式。
- **项目 Skill 去重** — 浏览项目 Agent Skill 目录时，已安装并记录的 Skill 不再重复出现在待安装列表中。
- **更新状态更清晰** — 更新检查会区分“有更新”“已是最新”“无法检查”和“上游已删除”，状态判断更直观。

### Fixed

- **私有仓库来源保真** — 通过 SSH 或 private git 来源安装的 Skill 会保留原始来源地址，后续更新和重新安装不会丢失访问方式。
- **同仓库 Skill 更新更准确** — 同一仓库存在多个 Skill 时，更新会优先使用安装时记录的位置，避免更新到错误内容。
- **上游删除处理更安全** — 远端 Skill 已删除时，不再误触发普通更新，也不会在检查阶段自动删除本地文件。

## [1.5.0] 2026-05-23

### Added

- **默认安装目标** — 可分别设置全局和项目 Skill 默认安装到哪些 Agent。
- **修复来源** — 对于缺少更新检查信息的 Skill，可直接修复来源并恢复版本检查，无需重新走完整安装流程。

### Changed

- **安装流程优化** — 安装 Skill 时，安装范围、来源填写、Skill 选择、Agent 选择和确认步骤更清晰。
- **Agent 选择更直观** — 自动可用的 Agent 与需要额外安装的 Agent 分开展示，并显示对应路径。
- **设置页重组** — 设置页新增侧边导航，外观、安装、Git、项目和关于信息分区更清楚。
- **Git 设置位置调整** — 远程拉取超时设置移动到 `Settings > Git`。

### Fixed

- **更新检查更可靠** — 旧版本安装的 Skill 即使缺少部分来源信息，也能更稳定地展示状态，并在可修复时提供明确入口。
- **更新目标更准确** — 更新 Skill 时更准确地保留原来的安装范围和 Agent 目标。
- **路径显示更一致** — 统一不同页面中的 Agent 路径展示。
- **窗口尺寸更稳定** — 避免窗口过小或过大导致页面显示异常。

## [1.4.0] - 2026-04-27

### Added

- **远程拉取超时设置** — 在 `Settings > General` 新增 Git 仓库拉取超时配置，可选 1 / 2 / 5 / 10 分钟预设或在 30–3600 秒内自定义，安装与更新流程统一读取该值
- **风险来源安装确认** — 安装来自 OpenClaw 等高风险来源的 skill 时，在确认页要求显式勾选确认后才能继续
- **「无法检查更新」状态展示** — 来源不支持远端检查（本地路径、缺失 skill 路径等）的 skill 在卡片和详情面板上明确标注，不再误显示为「已是最新」

### Changed

- **对齐 skills CLI v1.5.1** — 同步安装/更新流程的语义：批量更新按 (来源 + 分支) 分组，避免同仓库不同分支共用错误的 clone；克隆失败时错误提示包含实际超时秒数；克隆过程跳过 LFS smudge 加快速度

### Fixed

- **更新按钮显示语义** — 仅在检测到可用更新时显示 Skill 更新按钮；更新成功后会在列表卡片和详情面板中同步隐藏
- **批量更新来源更准确** — 使用 Update All 更新来自仓库子目录的 Skill 时，会沿用安装时记录的原始目录，避免同一仓库存在同名 Skill 或非默认目录结构时更新到错误内容

## [1.3.0] - 2026-04-17

### Changed

- **Manage Agents 弹窗重设计** — 新增 Agent 的安装方式选项改为 card-style radio（整行点击、选中态高亮），字号体系统一 AgentSelector（13px/11px），通过顶部分隔线与 Agent 列表区分层级；始终渲染且无新增 Agent 时置灰，消除选中/取消 Agent 时弹窗的高度抖动
- **AgentSelector 中文文案本地化优化** — 调整若干中文术语使其更贴合用户心智：通用目录区标题「基准目录」→「通用目录」、Badge「默认支持的 Agent」→「自动支持」、独立目录区标题「独立目录的 Agent」→「独立目录」；在独立目录标题旁以小字统一解释"已检测"含义，避免多处 Tooltip 堆积
- **「已检测」Badge 语义明确化** — 独立目录 Agent 的检测标识由「已安装」/"Installed" 改为「已检测」/"Detected"，避免与"skill 已安装到该 Agent"产生歧义（检测仅根据 Agent skills 目录是否存在反推）
- **GeneralTab 空状态判定修正** — 默认 Agent 设置的空状态判定由 `hasNonUniversalAgents` 改为 `hasAgents`，只检测到 Universal Agent 时不再误显示空状态

### Fixed

- **AgentSelector 路径标签渲染** — `scope` 为 undefined 时不再显示默认路径字符串，避免非 global/project 场景下渲染错位路径

### Removed

- 清理 5 个不再使用的 i18n key：`addSkill.agents.detectedSection` / `otherSection` / `otherAgentsTitle` / `expand` / `collapse`

## [1.2.0] - 2026-04-16

### Added

- **GitNexus 项目指引** — 新增 `AGENTS.md`，并在 `CLAUDE.md` 中加入 GitNexus 代码智能工具的使用规范、风险检查流程和索引刷新说明；`.gitignore` 忽略 `.gitnexus` 本地索引目录
- **Manage Agents 安装模式选择** — 管理已安装 Skill 的 Agent 支持时，可为新增 Agent 选择 Symlink 或 Copy 模式，并在前端弹窗中明确展示两种投放方式

### Fixed

- **Copy 模式保留 canonical 目录** — `copy` 安装不再跳过 `.agents/skills/<skill>`，而是先写入 canonical，再复制到目标 Agent 目录，避免后续管理 Agent 时找不到源目录
- **Manage Agents 不再静默降级** — 通过 Manage Agents 添加 Agent 时，用户选择 `symlink` 后若链接创建失败会返回明确错误，不再自动 fallback 成 copy
- **更新流程保留 per-agent 模式** — 单个更新和批量更新改为按 Agent 独立检测并应用原有模式，避免用第一个 Agent 的模式覆盖所有 Agent

## [1.1.0] - 2026-04-07

### Added

- **Agent 管理** — 为已安装的 Skill 添加或移除 Agent 支持，无需重新安装；SkillCard 和详情面板均提供入口
- **跨项目复制 Skill** — 一键将项目级 Skill 复制到其他项目，自动标注目标项目中已存在的 Skill 并提示覆盖
- **单实例运行** — 集成 `tauri-plugin-single-instance`，防止同时打开多个应用进程；重复启动时自动聚焦已有窗口
- **Discover 双栏详情面板** — Discover 页支持在可调节双栏布局中浏览榜单与搜索结果，右侧详情展示 overview、`SKILL.md` 正文、安全审计、Agent 安装量与 CLI 安装命令；侧栏展示本机安装位置（Global 及各项目），安装按钮始终可用，支持将 Skill 安装到不同位置

### Changed

- **Discover 对齐 skills.sh 语义** — 榜单切换调整为 All Time / Trending / Hot，搜索结果保留 live API 顺序，official creators 改为内部 metadata 判断，详情解析按页面分区提取真实内容
- **前端状态管理重构** — Skills 状态管理按职责拆分为数据层、详情面板、对话框三个独立模块，降低模块间耦合；Skill 更新完成后列表刷新不再阻塞 UI 交互
- **设置页重构** — General、Projects、About 三个标签页拆分为独立组件，提升页面加载效率和可维护性

### Fixed

- **Discover 搜索结果截断** — skills.sh search 请求上限从 50 提升到 100，减少热门关键词搜索时结果过早截断
- **About 页技术栈版本标注** — React 版本从 18 修正为 19

## [1.0.0] - 2026-04-03

### Added

- **Skill 内容详情面板** — Skills 页面支持在可调节宽度的双栏布局中查看已安装 skill 的 `SKILL.md` 正文；自动剥离 frontmatter 并以 Markdown/GFM 渲染，同时展示来源、安装时间、更新时间、适用 Agents 和安装路径，支持复制路径、重试加载、面板内直接更新/删除

### Changed

- **整体视觉重设计** — 引入 Manrope / Inter 字体，更新为 emerald 主色与分层中性色面板体系，统一更利落的圆角、边框、滚动条和文档排版风格
- **导航与品牌焕新** — Header 改为胶囊式导航，刷新应用 Logo，并同步更新 Tauri 桌面端图标资源
- **Skills 工作台重构** — 选中 skill 后切换为“紧凑列表 + 沉浸式详情面板”的工作台布局；SkillCard、Compact List、Empty States 和详情阅读区整体重做
- **Context Sidebar 重做** — 左侧上下文切换区调整为 Workspace / Global / Projects 分区，强化选中态、项目路径信息和底部 Add Project 入口
- **Discover / Wizard / Settings 统一改版** — 搜索安装流程、Discover 页面和 Settings 三个标签页统一为新的卡片化界面；About 区新增品牌展示、外链入口和更新操作聚合区

### Fixed

- **Sidebar 设计稿对齐问题** — 修复 Add Project 按钮、Workspace 标题、GLOBAL 分区标题和项目列表细节与设计稿不一致的问题
- **详情面板阅读干扰** — 移除详情区 sticky header，并将更新/删除等操作收纳到 Hero 区域，减少滚动阅读时的视觉干扰
- **紧凑列表细节打磨** — 调整列表计数、间距和选中态表现，改善双栏模式下的浏览与定位体验

## [0.11.0] - 2026-04-02

### Changed

- **对齐 vercel-skills CLI v1.4.7** — 完成与上游 23 个 commit（`7022ad3..HEAD`）的兼容性适配
- **Well-Known 路径迁移** — 优先探测 `.well-known/agent-skills`，fallback 到旧的 `.well-known/skills`；`build_index_urls()` 为每个 well-known 路径生成候选 URL
- **Discovery 搜索路径清理** — 移除已废弃的 `.agent/skills`（单数）搜索路径，仅保留 `.agents/skills`

### Added

- **Branch ref `#fragment` 语法** — source 输入支持 `owner/repo#branch`、`owner/repo#branch@skill-name` 格式；source parser 新增 `parse_fragment_ref()` + `looks_like_git_source()` 判定逻辑；含 `/` 的分支名、tag、`github:`/`gitlab:` 前缀递归附加等场景全覆盖（10 个新测试）
- **Lock 文件 `ref` 字段** — `SkillLockEntry` 和 `LocalSkillLockEntry` 新增 `ref_name: Option<String>`（serde rename `ref`），install/update 命令全链路传递；更新检测按 `(source, ref)` 分组，同仓库不同分支互不干扰
- **新增 Agent：Bob (IBM) 和 Firebender** — agent 总数 43 → 45；Bob 使用 `.bob/skills` 目录，Firebender 使用 `.agents/skills` + `~/.firebender/skills`
- **前端 ref badge** — SourceStep 输入框下方展示 branch/skill-filter Badge；SkillCard 已安装 skill 卡片在 source 信息行展示分支标签；新增 en/zh-CN 双语 i18n key

### Fixed

- **macOS 外部链接无法打开** — 更新弹窗中「前往下载」按钮使用 `window.open()` 在 Tauri webview 中无效，改用 `tauri-plugin-opener` 的 `openUrl()` 通过系统浏览器打开；同时 opener 插件自动拦截页面中所有 `<a target="_blank">` 链接，修复 SettingsPage、SkillCard、SkillDetailDialog 等处的外部链接
- **Discover 模块 TypeScript 严格模式错误** — 修复 `parseLeaderboardHtml` 返回值含 null 的类型不匹配、`DiscoverSkillSummary` 上不存在的 `repoUrl` 引用、`relevanceScore` 可能 undefined 的排序比较
- **SkillCard ref badge 尾部分隔符** — 当 `gitRef` 存在但 `updatedAt` 为空时不再渲染多余的 `·` 分隔符
- **Discover 模块 regex 性能** — `parseLeaderboardHtml` 循环内的 3 个 regex literal 提升为模块级常量（`js-hoist-regexp`）

## [0.10.0] - 2026-03-12

### Changed

- **批量更新检测优化** — `check_updates` 使用 `fetch_skill_folder_hashes_batch` 批量查询同源 skills 的 hash，同源 N 个 skills 从 N 次 GitHub Trees API 降为 1 次
- **Update All 并行分组** — `updateAllInSection` 按 source 分组后调用 `updateSkillsBatch` 批量 API，不同源组并行执行（`Promise.all`），同组共享单次 clone
- **SkillCard 进度条性能优化** — 更新进度 phase 改用 `useRef` + DOM 操作替代 `useState`，避免 Tauri 事件驱动的高频 re-render；条件渲染统一为三元表达式
- **刷新按钮交互优化** — Refresh 按钮增加最小 300ms spin 保持时间 + ✓ 完成态闪现（800ms），解决操作过快时用户无法感知点击生效的问题；Check 按钮检测完成后短暂显示 ✓ 图标（有更新时跳过，已有 "X updates" 信号）

### Added

- **`update_skills_batch` 命令** — 新增批量更新后端命令，按 source 分组后每组只 clone 一次仓库，从同一 clone 中安装所有同源 skills
- **`fetch_skill_folder_hashes_batch` API** — 批量获取同源多个 skill 文件夹的 hash，单次 GitHub Trees API 请求即可比对所有 skills
- **SkillCard 更新状态 Badge** — 新增 done/failed 独立 Badge 标识（`updateDone`/`updateFailed` i18n key），替代底部色条的单一信号

### Fixed

- **更新缓存标记残留** — 更新成功后清除 `updateInfoCache` 中对应 skill 的 `hasUpdate` 标记，防止 `syncSkills` 恢复旧标记导致更新按钮重新出现

## [0.9.0] - 2026-03-09

### Changed

- **对齐 skills CLI v1.4.4** — 完成与 vercel-labs/skills CLI v1.4.2 → v1.4.4 的全量同步
- **移除 `SourceType::DirectUrl`** — `direct-url` 类型统一为 `well-known`；自定义 serde `Deserialize` 实现确保旧 lock 文件中 `"direct-url"` 值可正确反序列化为 `WellKnown`
- **更新检测范围扩展** — `check_updates` 不再限制 `sourceType == "github"`，改为检查 `skillFolderHash` 和 `skillPath` 字段是否存在，支持更多来源类型的更新检测

### Added

- **Well-Known Skills 支持** — 实现 RFC 8615 `/.well-known/skills/` 协议，支持从任意 HTTP 站点发现和安装 skills（如 `https://mintlify.com/docs`）；新增 `core/wellknown.rs` 模块处理 index.json 获取、文件下载和临时目录管理；`fetch_available` 和 `install_skills` 命令完整接入 WellKnown 来源；lock 文件使用 hostname 作为 source identifier（对齐 CLI WellKnownProvider）
- **`github:`/`gitlab:` 前缀简写** — source 输入支持 `github:owner/repo` 和 `gitlab:owner/repo` 前缀格式，分别复用 GitHub shorthand 和 GitLab URL 解析逻辑（对齐 CLI v1.4.4）
- **SSH URL owner/repo 提取** — `get_owner_repo()` 新增对 `git@host:owner/repo.git` 格式的解析，支持 GitHub、GitLab、自定义 host 和多级 subgroup 路径
- **Subpath 路径遍历防护** — 双层防护：解析层 `sanitize_subpath()` 拒绝含 `..` 段的 subpath，执行层 `is_subpath_safe()` 验证 resolved path 不逃逸 base 目录
- **27 个新增 Rust 测试** — 覆盖 serde 兼容层（3）、前缀简写（5）、SSH URL 解析（6）、路径遍历防护（11）、更新检测（1）、现有测试修改（1）

## [0.8.0] - 2026-03-02

### Changed

- **更新交互重构** — 用独立 Dialog 替代 Toast 通知：发现新版本时弹出 Dialog 展示 Release Notes（Markdown 渲染），用户确认后再下载；下载中展示进度条且不可关闭；下载完成后提供「立即重启/稍后」选项；macOS 跳转 GitHub 下载
- **Updater Store 重写** — 新增并发保护（仅 idle/error 可触发检查）、下载中止（dismiss 设置 abortFlag）、错误退避（失败后 4h 重试 vs 正常 24h 间隔）、Release Notes 和 lastCheckTime 字段
- **Settings 更新状态完善** — 覆盖全部 7 种状态（idle/checking/available/downloading/ready/error/idle+lastCheckTime），idle 状态展示相对时间「上次检查：5 分钟前」
- 移除自动下载行为，改为用户在 Dialog 中确认后再开始下载
- **update_skill 结构化响应** — `update_skill` 命令返回 `UpdateSkillResponse`（含 per-skill 状态 success/partial/failed/skipped、per-agent 结果、warnings、耗时），前端根据状态展示差异化 toast（成功/部分成功/跳过/失败 + 告警）
- **Lock 文件原子写入** — `skill_lock` 和 `local_lock` 的写入改用 `tempfile::persist` 原子操作，避免写入中断导致文件损坏；统一追加尾部换行符
- **Uninstaller 简化** — 提取 `resolve_agents_to_remove` 辅助函数，移除冗余的 `detect_installed` 中间回退逻辑
- **CompleteStep 重构** — 统一为 skill 分组卡片展示，显示 agent 覆盖率统计（如 2/3 agents），失败明细可折叠展开
- **安装重试行为分离** — 提取 `InstallBehavior` 结构体，重试模式下跳过 Universal Agent 自动填充和 agent 持久化
- **Install/Update 共享核心** — 提取 `install_skill_to_agents()` 共享函数，install 和 update 命令复用同一安装逻辑；`PerAgentInstallResult` 携带完整 path/canonical_path/mode 数据
- **Update 文件系统检测** — 更新命令通过 `detect_installed_agents_for_skill()` 扫描文件系统确定目标 agents（非 lock 元数据），通过 `detect_install_mode()` 检测 symlink/junction vs copy 模式
- **Skills Store 状态重设计** — `updatingSkill: string | null` 升级为 `updatingSkills: Map<string, status>` 支持批量并行状态追踪；新增 `checkingUpdateScopes: Set<string>` 实现 per-scope 独立检测状态
- **更新检测缓存** — 新增 scope 级 TTL 缓存（5 分钟），切换 scope 时避免重复网络请求；stale context guard 防止异步写入过期数据
- **SkillsPanel selector 优化** — `checkingUpdateScopes` 从整个 Set 订阅改为派生 boolean selector（`rerender-derived-state` 规则），减少无关重渲染

### Added

- **UpdateDialog 组件** — 三态更新弹窗（available/downloading/ready），react-markdown 懒加载渲染 Release Notes，下载中禁止关闭
- **formatRelativeTime 工具函数** — 将时间戳转换为 i18n 相对时间 key（刚刚/N 分钟前/N 小时前/昨天/N 天前），含 5 个单元测试
- **Updater Store 测试** — 16 个单元测试覆盖并发保护、状态转换、错误退避、dismiss 重置
- **逐 Skill 重试** — CompleteStep 新增「重试该 Skill」按钮，仅对失败的 skill + 失败的 agents 重新安装（通过 `retrySkillName`/`retryAgents` 状态传递）；后端 `InstallParams` 新增 `retry` 标志
- **UpdateSkillResponse 类型体系** — 新增 `models/update.rs`：`UpdateSkillResponse`、`UpdateSkillItemResult`、`UpdateSkillAgentResult`、`UpdateSkillSummary`、`UpdateSkillStatus`、`UpdateSkillAgentStatus`
- **11 个新增测试** — 6 个 Rust 测试（derive_skill_status 边界、summarize_results、InstallBehavior、serde 序列化）+ 2 个 CompleteStep 组件测试 + 2 个 useTauriApi 测试 + 1 个 skills store 测试
- **Section 级 Update All** — SkillsSection 标题栏新增「全部更新」按钮，支持批量串行更新（queued → updating → done/failed），进度计数器和取消按钮
- **SkillCard 内联进度条** — 更新时展示 phase-based 进度条（cloning 35% → installing 70% → writing_lock 90%），监听 `update-progress` Tauri 事件
- **手动检查更新** — 每个 Section 新增 Check 按钮，调用 `forceCheckUpdates()` 强制刷新指定 scope 的更新检测（绕过 TTL 缓存）
- **Update 进度事件** — 后端 `update_skill` 在 clone/install/lock-write 阶段发送 `update-progress` 事件，前端 SkillCard 响应并展示阶段标签

### Removed

- 移除 `update-toast.tsx`（Toast 更新通知），由 UpdateDialog 替代

## [0.7.0] - 2026-02-27

### Added

- **智能删除对话框** — 删除 skill 时展示 agent 安装详情，支持选择仅从部分 Agent 中移除 skill（保留源文件），或完全删除
- **get_skill_agent_details 命令** — 新增后端命令，查询 skill 的 universal / independent agent 分组安装详情，为智能删除对话框提供数据
- **SkillAgentDetails / IndependentAgentInfo 类型** — 新增数据模型，描述 skill 在各 agent 中的安装状态（路径、是否 symlink）
- **Plugin 分组支持** — 解析 `.claude-plugin/marketplace.json` 和 `.claude-plugin/plugin.json`，自动识别 skill 所属 plugin 并在 UI 中分组展示（对齐 skills CLI v1.4.2）
- **plugin_manifest 模块** — 新增 `src-tauri/src/core/plugin_manifest.rs`，支持多 plugin manifest 解析、路径安全校验（防目录穿越）和路径归一化
- **pluginName 字段贯穿数据链路** — `DiscoveredSkill` → `AvailableSkill` → `InstalledSkill` → `SkillLockEntry` / `LocalSkillLockEntry` 全链路传递 `pluginName`
- **分层 CLAUDE.md** — 新增 `src/CLAUDE.md`（前端 Store 交互模式、组件约定）和 `src-tauri/CLAUDE.md`（Rust 命令添加流程、模块职责表），根 CLAUDE.md 新增 Business Rules、Change Dependencies、Verification 段落
- **Vitest 测试基础设施** — 配置 Vitest + jsdom + @testing-library/react，包含 Tauri invoke mock 和 i18n mock 的全局 test-utils
- **29 个单元测试** — 覆盖 useTauriApi unwrap 逻辑（5）、context store（10）、skills store（6）、settings store（8）
- **Pre-commit hooks** — husky + lint-staged，提交前自动 eslint --fix
- **CI pipeline** — GitHub Actions 工作流：lint → test → build → cargo check

### Changed

- **Header 导航栏优化** — 导航标签改为 pill 圆角胶囊样式，放大 logo 和品牌名，导航图标始终可见（移除 `sm:hidden`），主题/语言按钮增大触控区域
- **ContextSidebar 侧边栏精简** — 移除标题栏、分区标题和底部「在设置中管理」按钮；去掉图标外层包裹容器；选中/悬停状态改为更柔和的 `foreground` 透明度样式；全局上下文新增副标题说明
- **remove_skill 命令增强** — 新增 `agents` 和 `full_removal` 参数，支持完全删除和部分移除两种模式；部分移除时仅删除指定 agent 的 symlink，不清理 canonical 目录和 lock file
- **DeleteSkillDialog 重构** — 从简单的 AlertDialog 升级为完整的 Dialog，包含 Skill 信息横幅、共享目录区（含级联全选和警告提示）、独立安装区（Checkbox 逐项选择）、加载骨架屏
- **Cline Agent 路径迁移** — skill 目录从 `.cline/skills` 迁移到 `.agents/skills`（对齐 skills CLI v1.4.2）
- **SkillsStep 安装向导** — 当 skill 来源包含 plugin 时，按 plugin 分组展示可选 skill 列表
- **ConfirmStep 确认页面** — 选中的 skills 按 plugin 分组展示，未归属 plugin 的归入「通用」分组
- **SkillCard 卡片** — 当 skill 属于某个 plugin 时，显示 plugin 名称 Badge

## [0.6.0] - 2026-02-26

### Changed

- **重构 ConfirmStep 确认页面** — 移除冗余的 Scope 信息卡片和重复的路径前缀、agent badges、mode label，新增集中覆盖警告条与行内 Tooltip 标记，新增安装信息区展示安装方式和安装目录列表
- **优化安装进度展示** — 安装过程新增细粒度进度状态反馈，提升安装体验
- 搜索安装 skill 时窗口自适应高度
- 移除安装步骤中内容区域多余的 padding top
- 优化 ConfirmStep 布局层级和交互体验
- ESLint 校验范围限定为 `src` 目录

### Fixed

- 修复 CompleteStep 中 `useMemo` 在 early return 之后调用导致违反 Rules of Hooks 的问题
- 修复 OptionsStep 中渲染期间直接写入 `ref.current` 的问题，改为通过 `useEffect` 同步
- 修复 InstallingStep 中 `useEffect` 缺失 `t` 和 `state.availableSkills` 依赖的问题，通过 ref 捕获
- ESLint 配置新增 `argsIgnorePattern: '^_'`，支持下划线前缀的未使用参数惯例

## [0.5.0] - 2026-02-24

### Fixed

- 修复设置页「检查更新」按钮在首次检查后永久消失的问题，改为始终显示刷新按钮允许手动重新检查
- 修复检查更新失败时无任何错误提示的问题，新增错误状态展示和重试按钮
- 更新检查 UI 改为由 store 状态驱动，移除对 `localStorage` 的直接依赖

## [0.4.0] - 2026-02-24

### Added

- **Cortex & Universal Agent 支持** — 新增 Cortex（Snowflake）和 Universal（`.agents/skills`）两种 Agent 类型
- **项目级 Local Lock** — 新增 `skills-lock.json`，使用 SHA-256 哈希追踪项目级 skill 状态，兼容 skills CLI v1.4.1
- **安全审计 API** — 调用 `add-skill.vercel.sh/audit` 接口获取 skill 风险等级，3 秒超时优雅降级
- **RiskBadge 组件** — 在 SkillCard 和安装确认步骤展示 skill 安全风险等级（safe / low / medium / high / critical / unknown）
- **Source 别名** — 支持源地址别名解析（如 `coinbase/agentWallet` → `coinbase/agentic-wallet-skills`）

### Changed

- 安装流程不再排除 `README.md`（仅排除 `metadata.json`）
- 项目级 skill 的安装/卸载/更新/列表全部切换到 Local Lock
- 更新检测支持从 Local Lock 读取 `remoteHash` 进行比对
- Replit Agent 检测标识从 `.agents` 改为 `.replit`
- Cursor 项目级 skill 目录从 `.cursor/skills` 改为 `.agents/skills`，成为 Universal Agent
- OpenClaw 全局目录三路径均不存在时默认回退到 `.openclaw/skills`（对齐 CLI v1.4.1）

### Fixed

- 卸载 skill 时增加安全检查，避免误删被多个 Agent 共享的 canonical 目录
- 移除 Antigravity 的 `cwd/.agent` 检测，减少误判（对齐 CLI v1.4.1）
- 移除 GitHub Copilot 的 `cwd/.github` 检测，`.github` 是仓库标记而非 Copilot 安装标记（对齐 CLI v1.4.1）
- Git 克隆时设置 `GIT_TERMINAL_PROMPT=0`，防止私有仓库弹出凭据提示导致进程挂起

## [0.3.2] - 2026-02-24

### Changed

- 版本号统一由 `package.json` 管理，构建时自动同步到 `Cargo.toml` 和 `tauri.conf.json`

## [0.3.1] - 2026-02-24

### Fixed

- 修复 macOS 和 Ubuntu 编译失败问题

## [0.3.0] - 2026-02-24

### Added

- **发现页** — 通过 skills.sh 搜索在线 skill 并一键安装
- **更新检测** — 支持检测已安装 skill 的新版本并一键更新
- 安装 skill 弹窗优化为独立窗口

### Changed

- 使用 tauri-specta 替代手动类型桥接，Rust 类型自动生成 TypeScript 绑定
- React 代码全面优化，消除潜在隐患

## [0.2.0] - 2026-02-11

### Added

- **安装错误页** — 安装 skill 报错时展示详细错误信息和修复建议

### Fixed

- 修复 Windows 环境执行 git 命令时弹出控制台窗口的问题
- 修复命令行解析安装时 source 传递错误的问题
- 修复 TypeScript 类型错误

## [0.1.0] - 2026-02-11

### Added

- **首个发布版本**
- Skill 管理核心功能：安装、卸载、更新
- 支持 38+ AI Agent（Claude Code、Cursor、Windsurf、Copilot 等）
- 多来源支持：GitHub shorthand、URL、本地路径、安装命令解析
- 安装模式：符号链接（推荐）和复制
- Global / Project 双层 scope 管理
- Agent 过滤和 Display Name 展示
- 国际化支持（English / 简体中文）
- 深色/浅色主题切换
- GitHub Actions CI/CD 构建流水线（Windows / macOS / Ubuntu）

[1.1.0]: https://github.com/hccake/skill-deck/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/hccake/skill-deck/compare/v0.11.0...v1.0.0
[0.11.0]: https://github.com/hccake/skill-deck/compare/v0.10.0...0.11.0
[0.10.0]: https://github.com/hccake/skill-deck/compare/v0.9.0...0.10.0
[0.9.0]: https://github.com/hccake/skill-deck/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/hccake/skill-deck/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/hccake/skill-deck/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/hccake/skill-deck/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/hccake/skill-deck/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/hccake/skill-deck/compare/v0.3.2...v0.4.0
[0.3.2]: https://github.com/hccake/skill-deck/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/hccake/skill-deck/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/hccake/skill-deck/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/hccake/skill-deck/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/hccake/skill-deck/releases/tag/v0.1.0
