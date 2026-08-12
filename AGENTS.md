## 项目协作规则

- 与用户沟通和编写长期文档时，默认使用符合中文语境的陈述体。Agent、Skill、Environment、Context、Backend、WSL 等专业术语可以保留英文。
- 开始任务时先阅读[文档地图](./docs/README.md)，按照任务路由找到唯一主文档。`AGENTS.md` 只保留协作规则，不复制产品、架构或领域正文。
- 当前工作区可能包含尚未提交的用户改动。只修改本任务需要的文件，不回退、不覆盖、不顺带整理无关内容。
- 新的 spec、ticket、设计过程和阶段性评审统一保存在 `.scratch/**`。这些过程文件不属于受版本控制的正式文档，不得加入暂存区或提交。
- 代码变更遵循测试先行（test-first），并按[贡献指南](./CONTRIBUTING.md)同步实际受影响的类型绑定（bindings）、窗口权限、国际化文案和长期文档。
- 完成前运行与改动范围相符的验证，并依据最新输出报告结果。
- Agent 完成本轮涉及 Rust/Tauri 的构建、检查或测试，且不再运行 Cargo 命令后，检查 Cargo 实际 `target` 目录（当前为 `src-tauri/target`）的占用。占用达到 30 GiB 时，在交付说明中报告当前容量并提供 `cargo clean --manifest-path src-tauri/Cargo.toml`；清理由用户明确授权后执行。
- Agent 创建的跨平台验证副本和构建输出统一放在仓库外的固定验证目录；每次验证开始前清理旧目录，结束后清理当前目录，不在源码工作区或随机 `/tmp` 路径留下可被后续任务误识别为源码的副本。
- 仓库只保留这一份共享的 Agent 指令文件。不要创建目录级 `AGENTS.md` 或 `CLAUDE.md`；工具专属入口只引用本文件。

## Agent Skill

### Issue 管理

Issue 和 spec 在仓库内以 Local Markdown 文件管理。参见 `docs/agents/issue-tracker.md`。

### Triage 标签

Triage 使用五个默认角色标签。参见 `docs/agents/triage-labels.md`。

### 领域文档

修改领域术语、概念边界或 ADR 前，先按 `docs/agents/domain.md` 读取相应领域资料。
