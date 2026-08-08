---
status: accepted
---

# 受保护写入必须使用目标文件系统的原生能力

受保护写入由能够原生访问目标文件的 Environment 执行。跨 Environment 复制时，来源 Environment 固定完整内容，目标 Environment 随后执行写入并管理恢复数据。

这项边界使文件系统身份、原子操作、lock 和恢复位置遵循同一套文件系统语义。只有系统能够跨 Environment 可靠提供等价的原子操作能力，并能明确恢复数据的归属时，才重新考虑直接跨文件系统写入。当前行为由[Environment、Skill 位置与项目管理](../environments-and-projects.md#访问-windows-和-wsl-中的项目)、[系统架构](../architecture.md#平台实现)和[执行与恢复](../execution-and-recovery.md#路径安全与实际目标)负责说明。
