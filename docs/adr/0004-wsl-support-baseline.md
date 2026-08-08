---
status: accepted
---

# WSL 采用最小用户态支持基线

WSL 连接只检查 Skill 操作所需的最低用户态条件：Git、POSIX shell，以及当前操作依赖的 GNU 工具行为。产品不维护受支持发行版名单，也不为每个发行版维护通用的能力档案。

这项取舍可以覆盖常见的 GNU/Linux 用户态，同时控制 WSL 兼容工作的范围。持续的用户反馈表明常见目标发行版无法满足这些条件，或者产品明确扩大支持范围时，再重新评估。当前行为由[Environment、Skill 位置与项目管理](../environments-and-projects.md#在-windows-和-wsl-之间切换)和[测试与验证规范](../testing.md#unix-shell-与-wsl)负责说明。
