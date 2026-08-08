---
status: accepted
---

# Environment 连接成功后独立加载项目

Environment 连接成功后，应用立即完成切换并展示目标 Environment 的全局 Skill。已添加项目随后独立加载；即使项目加载失败，已经完成的 Environment 切换仍然有效。

这项边界确保项目信息损坏时，用户仍可使用目标 Environment 的基础能力和修复入口。只有全部 Environment 能力都必须依赖完整项目列表时，才重新考虑把连接与项目加载合并为一个事务。当前行为由[Environment、Skill 位置与项目管理](../environments-and-projects.md#在-windows-和-wsl-之间切换)和[产品行为与交互](../product.md#浏览全局-skill-和已添加项目)负责说明。
