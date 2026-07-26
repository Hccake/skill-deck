# Skill Deck 文档

Skill Deck 的长期文档说明当前产品、领域规则和系统约束。每项长期规则归入一份主文档，其他文档仅保留理解当前主题所需的摘要，并链接到对应主文档。组件行为、公共类型、命令清单和边界条件以源码与测试为准。

## 文档地图

| 文档 | 主要内容 | 更新时机 |
|---|---|---|
| [产品设计](./product.md) | 用户心智模型、页面能力、用户工作流、反馈规则、Agent 筛选交互和产品限制 | 用户可见能力、交互语义或产品限制变化 |
| [Agent](./agents.md) | Agent 注册表、定义、读取能力、检测、关联 Agent、筛选候选、选择分组和默认目标 | Agent 模型、解析规则或选择语义变化 |
| [Environment 与 Context](./environments-and-contexts.md) | Environment、Context、项目绑定、路径解析条件和存储访问 | 执行位置、管理范围、项目绑定或存储归属变化 |
| [Skill 生命周期](./skill-lifecycle.md) | 来源、发现、安装、读取、更新、来源修复、管理 Agent、复制和移除 | Skill 从来源进入本地后的业务流程变化 |
| [系统架构](./architecture.md) | 系统边界、技术分层、运行时职责、进程通信、窗口、平台实现、快照一致性和系统安全 | 顶层分层、依赖方向、窗口、传输边界或运行时职责变化 |
| [测试与验证规范](./testing.md) | 测试分层、测试夹具、跨平台规则、断言原则和验证证据 | 测试架构、平台测试能力、测试夹具或验证策略变化 |
| [执行与恢复](./execution-and-recovery.md) | 内容快照、预览与执行、变更计划、路径安全、原子写入、取消和恢复 | 写入协议、一致性、安全或恢复规则变化 |
| [skills CLI 兼容](./skills-cli-compatibility.md) | 上游 CLI 基线、共享格式、lock、哈希、稳定差异和同步检查 | CLI 版本或互操作规则变化 |
| [贡献指南](../CONTRIBUTING.md) | 开发命令、前后端协作、验证、CI 和发布流程 | 开发、验证或发布流程变化 |

仓库根目录的 `README.md` 与 `README.zh-CN.md` 面向最终用户，`CHANGELOG.md` 记录版本变化。`AGENTS.md` 提供 Agent 执行规则。

## 按任务阅读

| 任务 | 必读内容 |
|---|---|
| 修改用户可见功能、页面交互或 Agent 筛选 | [产品设计](./product.md)，再按涉及概念进入对应主文档 |
| 修改 Agent 定义、检测、关联 Agent、筛选候选、选择分组或默认目标 | [Agent](./agents.md) |
| 修改 Environment、Context、项目绑定、路径解析或存储访问 | [Environment 与 Context](./environments-and-contexts.md) |
| 修改来源、发现、安装、更新、来源修复、管理 Agent、复制或移除 | [Skill 生命周期](./skill-lifecycle.md)；涉及写入机制时增加[执行与恢复](./execution-and-recovery.md) |
| 修改路由、跨窗口通信、Tauri 命令、WSL 传输或顶层依赖 | [系统架构](./architecture.md) |
| 新增或修改测试、测试夹具、平台适配器验证或 E2E | [测试与验证规范](./testing.md)；修改 CI 命令时增加[贡献指南](../CONTRIBUTING.md) |
| 同步上游 `skills` CLI | [skills CLI 兼容](./skills-cli-compatibility.md)，再检查受影响的领域主文档 |
| 修改构建、bindings、CI、应用更新或发布流程 | [贡献指南](../CONTRIBUTING.md) |

## 信息归属

| 信息 | 主文档或依据 |
|---|---|
| 用户可见能力、交互语义和产品限制 | `product.md` |
| Agent、Environment、Context 和 Skill 的长期领域规则 | 对应领域主文档 |
| 顶层技术边界、职责和跨层一致性 | `architecture.md` |
| 测试分层、测试夹具、跨平台验证和证据范围 | `testing.md` |
| 变更、一致性和恢复保证 | `execution-and-recovery.md` |
| `skills` CLI 共享格式和稳定差异 | 当前上游 CLI 源码与 `skills-cli-compatibility.md` |
| 公共命令和类型的精确形状 | Rust 类型、命令注册和生成的 bindings |
| 软件包、运行时和工具链版本 | `package.json`、lockfile、`Cargo.toml` 和 CI 配置 |
| 边界条件与可执行行为 | 测试和当前实现 |
| 版本变化 | `CHANGELOG.md` |
| 设计过程、实施计划和阶段性评审 | `docs/plans/**` 与 `docs/superpowers/**`，均为本地过程材料 |

文档与实现出现差异时，应在同一变更中确认正确设计并恢复一致性。长期规则仍由原主文档维护，其他文档通过摘要和链接引用。

## 架构决策记录

[`docs/adr/`](./adr/) 记录具有长期价值的架构取舍、理由和重新讨论条件。当前行为仍由文档地图中的主文档、源码和测试说明。

## 写作规则

- 正文先说明当前行为，再补充触发条件、异常结果和必要限制。
- 产品、领域和架构文档使用“谁负责什么、何时发生、结果如何”的陈述句。
- 被放弃的方案和取舍理由集中记录在 ADR，长期主文档说明当前状态。
- 安全禁止、兼容差异和用户需要据此决策的产品限制可以使用直接否定句。
- Agent、Skill、Environment、Context、WSL、CLI、Tauri、Rust 等专业术语可以保留原文；普通叙述使用自然中文。
- 精确代码名称、字段、状态值、命令和路径使用行内代码格式，并只在需要对应实现或协议时出现。
- 每个段落表达一个主要结论；复杂条件使用列表、表格或图示。
- 术语由对应主文档在首次出现时自然定义，其他文档复用同一表达。
- 产品文档说明用户行为和反馈，架构与执行文档说明职责、安全边界和长期保证。
- 源码可以直接提供的完整 Agent、命令、类型、组件、状态模块和制品清单由源码维护。
- 每次变更更新真正拥有该规则的长期文档。
