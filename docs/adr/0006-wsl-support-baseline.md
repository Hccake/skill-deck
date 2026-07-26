---
status: accepted
---

# WSL 采用最小用户态支持基线

## 背景

WSL 发行版和用户态工具组合很多。为每项操作维护发行版名单、完整能力矩阵、复杂脚本回退和多发行版发布验收，会让 Skill 管理软件承担通用 WSL 兼容层的成本。

## 决定

WSL 连接检查完成 Skill 操作所需的最小用户态基线：Git、POSIX shell 和当前操作依赖的 GNU 工具行为。全部条件满足后建立会话，缺少条件时返回具体错误。具体操作可以保留窄范围预检，产品不维护通用能力档案。

## 理由

这条基线覆盖常见 GNU/Linux 用户态，并能在缺少条件时给出明确反馈。真实 WSL 是 Windows 上的可选 Environment，三平台常规构建、协议测试和 Linux 脚本测试继续承担发布门禁。

## 重新讨论条件

持续的用户证据表明常见目标发行版普遍无法满足这条基线，或者产品明确扩大 WSL 支持范围时，可以重新评估兼容模型。

当前行为由[Environment 与 Context](../environments-and-contexts.md#wsl-environment-边界)和[测试与验证规范](../testing.md#unix-shell-与-wsl)负责说明。
