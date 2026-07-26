---
status: accepted
---

# 受保护写入由存储归属 Environment 执行

## 背景

Host 与 WSL 有时能够互相访问文件，但“可以访问”不能证明当前后端拥有正确的路径身份、大小写语义、原子替换能力、lock 归属和恢复位置。允许跨存储直接写入会引入一套低频但复杂的事务协议。

## 决定

受保护写入只能由目标路径的存储归属 Environment 执行。跨 Environment 复制是内容传递例外：来源 Environment 可以固定完整内容，目标 Environment 仍必须拥有目标存储，并负责实际写入与恢复。

## 理由与重新讨论条件

该边界让物理身份、原子操作、lock 和恢复资源保持在同一文件系统语义下。只有未来能够跨 Environment 可靠证明同一物理目标、等价原子能力和明确恢复归属时，才重新讨论跨存储直写。

当前行为由[Environment 与 Context](../environments-and-contexts.md#执行位置与存储访问)、[系统架构](../architecture.md#平台实现)和[执行与恢复](../execution-and-recovery.md#路径安全与物理身份)负责说明。
