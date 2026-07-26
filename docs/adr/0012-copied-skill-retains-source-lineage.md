---
status: accepted
---

# 跨 Environment 复制不改变来源能力

## 背景

跨 Environment 复制会在目标 Project 中产生独立的 Skill 目录，但来源可能是可重新获取的远端仓库，也可能只是本地路径。如果复制统一赋予更新能力，就需要为 Local 路径发明后台同步协议；如果统一丢弃来源关系，又会让远端来源无故失去更新能力。

## 决定

复制保留来源本来具备的能力。Remote、Git 和 Well-known 等可重新获取来源，在目标 lock 中保留后续更新需要的信息，并由目标 Environment 直接重新获取；Local 来源只保留来源凭据和内容基线，复制后仍不可更新。

来源 Environment 只负责在复制前固定完整内容快照。快照固定后，目标执行不再依赖来源 Environment 在线，也不会重新读取原始来源；目标 Environment 仍必须保持可用并负责写入。

## 理由与重新讨论条件

复制应该改变内容所在位置，而不应改变来源的客观能力。固定快照还能把来源连接生命周期与目标写入解耦，避免为普通执行维持跨 Environment 会话。只有 Local 来源获得明确、可验证的同步协议，或者新的来源类型改变“可重新获取”的判断方式时，才重新讨论这项边界。

当前行为由[Skill 生命周期](../skill-lifecycle.md#复制到项目)、[`skills` CLI 兼容](../skills-cli-compatibility.md#安装与更新语义)和[执行与恢复](../execution-and-recovery.md#skill-内容快照)负责说明。
