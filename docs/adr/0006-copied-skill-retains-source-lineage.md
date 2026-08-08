---
status: accepted
---

# 跨 Environment 复制保留来源原有能力

跨 Environment 复制会保留来源原有的能力。对于可以重新获取的来源，目标 lock 继续保存更新所需的信息；对于本地来源，目标 lock 保留来源信息和内容基线，但复制后的 Skill 仍然不具备更新能力。

来源 Environment 只负责在复制前固定完整内容，后续写入由目标 Environment 独立完成。本地来源具备可验证的同步协议，或者新的来源类型改变了“可以重新获取”的判断方式时，再重新评估这项边界。当前行为由[Skill 生命周期](../skill-lifecycle.md#复制到项目)、[skills CLI 参考与兼容](../skills-cli-reference.md#两个工具在安装和更新时的差异)和[执行与恢复](../execution-and-recovery.md#skill-内容快照)负责说明。
