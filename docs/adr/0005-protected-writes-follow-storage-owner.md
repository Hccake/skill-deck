---
status: accepted
---

# 受保护写入由存储归属 Environment 执行

## 背景

Host 与 WSL 有时能够互相访问文件，但“可以访问”不能证明当前后端拥有正确的路径身份、大小写语义、原子替换能力、lock 归属和恢复位置。允许跨存储直接写入会引入一套低频但复杂的事务协议。

## 决定

目标路径的存储归属 Environment 负责受保护写入。跨 Environment 复制由来源 Environment 固定完整内容，再由拥有目标存储的 Environment 执行写入并管理恢复资源。

## 理由

该边界让物理身份、原子操作、lock 和恢复资源保持在同一套文件系统语义下，也让目标后端承担完整的写入责任。

## 重新讨论条件

系统能够跨 Environment 可靠证明同一物理目标、等价原子能力和明确恢复归属时，可以重新评估跨存储直接写入。

当前行为由[Environment 与 Context](../environments-and-contexts.md#执行位置与存储访问)、[系统架构](../architecture.md#平台实现)和[执行与恢复](../execution-and-recovery.md#路径安全与物理身份)负责说明。
