---
status: accepted
---

# WSL 采用最小用户态支持基线

Skill Deck 支持 WSL 2。连接阶段通过最小身份探测取得用户和目录信息，再部署并连接随应用提供的 Environment Worker。Git、`wslpath` 等业务工具在首次执行依赖它们的操作时检查；工具缺失只影响对应操作。

这项取舍使连接能力独立于具体业务工具，避免缺少 Git 等工具时连基础读取和修复入口也无法使用。产品按操作所需能力确定支持范围，不维护发行版名单或通用能力档案。

常见目标发行版无法满足 Worker 或最小身份探测条件，或者产品需要扩大 WSL 支持范围时，重新评估这项边界。当前行为由[Environment、Skill 位置与项目管理](../environments-and-projects.md#在-windows-和-wsl-之间切换)维护，验证要求见[测试与验证规范](../testing.md#unix-shell-与-wsl)。
