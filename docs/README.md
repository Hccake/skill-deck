# Skill Deck 文档

Skill Deck 的长期文档描述当前产品、领域和系统约束。每项稳定事实只由一份文档负责；其他文档只保留读者理解当前主题所需的摘要，并链接到 owner。源码、公共类型和测试继续承担可执行细节，长期文档不复制组件实现或能够直接从代码获得的完整清单。

## 文档地图

| 文档 | 唯一负责内容 | 发生什么变化时更新 |
|---|---|---|
| [产品设计](./product.md) | 用户心智模型、页面能力、用户工作流、反馈规则和产品限制 | 用户可见能力、交互语义或产品限制变化 |
| [Agent](./agents.md) | Agent Registry、definition、读取能力、Detection、关联 Agent、选择分组和默认目标 | Agent 模型、解析规则或选择语义变化 |
| [Environment 与 Context](./environments-and-contexts.md) | Environment、Context、ProjectBinding、路径解析条件和 storage access | 执行位置、工作范围、Project 绑定或存储归属语义变化 |
| [Skill 生命周期](./skill-lifecycle.md) | Source、Discover、安装、读取、更新、来源修复、Manage Agents、复制和移除 | Skill 从来源到本地状态的业务语义变化 |
| [系统架构](./architecture.md) | 系统边界、技术分层、runtime ownership、IPC、窗口、platform backend 和系统级安全边界 | 顶层分层、依赖方向、窗口、transport 或 runtime ownership 变化 |
| [测试与验证规范](./testing.md) | 测试分层、fixture、跨平台边界、断言规则和验证证据 | 测试架构、平台测试能力、fixture 或验证策略变化 |
| [执行与恢复](./execution-and-recovery.md) | Payload、preview/execute、mutation、路径安全、原子写入、cancellation 和 Recovery | 写入协议、一致性、安全或恢复边界变化 |
| [skills CLI 兼容](./skills-cli-compatibility.md) | vendored CLI 基线、共享格式、lock/hash、稳定差异和同步检查 | CLI 版本或互操作边界变化 |
| [贡献指南](../CONTRIBUTING.md) | 开发命令、Frontend/Backend 协作、验证、CI 和 Release | 开发、验证或发布流程变化 |

仓库根目录的 `README.md` 与 `README.zh-CN.md` 面向最终用户，`CHANGELOG.md` 记录版本变化。`AGENTS.md` 和 `CLAUDE.md` 只提供 Agent 执行规则，不承担产品或架构说明。

## 按任务阅读

| 任务 | 必读内容 |
|---|---|
| 修改用户可见功能或交互 | [产品设计](./product.md)，再按涉及概念进入对应 owner |
| 修改 Agent definition、Detection、关联 Agent、选择分组或默认目标 | [Agent](./agents.md) |
| 修改 Environment、Context、ProjectBinding、路径解析或 storage access | [Environment 与 Context](./environments-and-contexts.md) |
| 修改 Source、Discover、安装、更新、来源修复、Manage Agents、复制或移除 | [Skill 生命周期](./skill-lifecycle.md)；涉及写入机制时增加[执行与恢复](./execution-and-recovery.md) |
| 修改路由、跨窗口通信、Tauri command、WSL transport 或顶层依赖 | [系统架构](./architecture.md) |
| 新增或修改测试、fixture、platform Adapter 验证或 E2E | [测试与验证规范](./testing.md)；修改 CI 命令时增加[贡献指南](../CONTRIBUTING.md) |
| 同步 vendored skills CLI | [skills CLI 兼容](./skills-cli-compatibility.md)，再检查受影响的领域 owner |
| 修改构建、bindings、CI、应用更新或 Release | [贡献指南](../CONTRIBUTING.md) |

## 权威关系

| 信息 | 权威来源 |
|---|---|
| 用户可见能力、交互语义和产品限制 | `product.md` |
| Agent、Environment、Context 和 Skill 的稳定领域语义 | 对应领域 owner |
| 顶层技术边界和 ownership | `architecture.md` |
| 测试分层、fixture、跨平台验证和证据边界 | `testing.md` |
| Mutation、一致性和 Recovery 不变量 | `execution-and-recovery.md` |
| skills CLI 共享格式和稳定差异 | 当前 vendored CLI 源码与 `skills-cli-compatibility.md` |
| 公共 command 和类型的精确形状 | Rust 类型、command manifest 和 generated bindings |
| package、runtime 与 toolchain 版本 | `package.json`、lockfile、`Cargo.toml` 和 CI 配置 |
| 边界条件与可执行行为 | 测试和当前实现 |
| 版本变化 | `CHANGELOG.md` |
| 设计过程、实施计划和阶段性 review | `docs/plans/**` 与 `docs/superpowers/**`，均不作为 tracked authority |

文档与实现发生冲突时，需要判断实现是否偏离已确认设计，或者文档是否已经过期，并在同一变更中恢复一致性。不能用另一个非 owner 文档覆盖冲突。

## 写作规则

- 正文使用符合中文语境的陈述体，专业术语可以保留 English 原文。
- 长期文档只描述当前状态，不记录迁移过程、旧方案或阶段性 UI 取舍。
- 稳定术语由对应 owner 在首次出现时自然定义，其他文档复用同一表达；不维护集中 glossary。
- 非 owner 文档最多保留当前主题需要的一段摘要，不复制状态表、规则清单或算法步骤。
- 产品文档不记录组件结构、布局尺寸、图标、Tooltip 或内部状态组织。
- 架构和执行文档可以记录跨模块契约、安全边界和长期不变量，不维护源码调用链或内部 helper 清单。
- 图示用于表达结构、状态或时序关系，简单事实使用正文或表格。
- 不手工维护完整 Agent、command、type、component、store、module 或 artifact 文件名清单。
- 每次变更只更新真正拥有该事实的长期文档。
