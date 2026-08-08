---
status: accepted
---

# Skill Deck 独立实现 skills CLI 兼容能力

Skill Deck 直接读写与 `skills` CLI 兼容的 Skill 目录和 lock 数据，并自行实现来源获取、Agent 解析和 Skill 工作流。应用运行时不调用该 CLI，也不依赖 Node.js。

这项边界使桌面端的多 Environment 路由、预览、执行和恢复遵循 Skill Deck 自身的契约，不受第三方 CLI 版本、输出格式和进程生命周期的直接影响。该 CLI 提供能够覆盖这些能力的稳定嵌入接口，或者维护独立实现的成本已经超过数据兼容带来的价值时，再重新评估。当前系统边界由[系统架构](../architecture.md#系统边界)说明，兼容范围和参考版本由[skills CLI 参考与兼容](../skills-cli-reference.md#skill-deck-与-skills-cli-的关系)维护。
