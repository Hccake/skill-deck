---
status: accepted
---

# Skill Library 采用单写者持久化模型

Skill Library catalog 和成员目录只由当前 Skill Deck 进程写入。Tauri 单实例机制限制正常运行时的应用实例数量，`RuntimeAdmissionCoordinator` 统一控制持久化写入许可，Runtime Library Adapter 再按 Environment 串行化 catalog、成员内容和内部恢复 I/O。其他进程、命令行工具和用户直接修改 catalog 不属于受支持的写入方式。

这项取舍删除了 Native 操作系统文件锁和 WSL `flock` 守卫协议，同时保留成员条件提交、catalog 原子写入、目录备份和崩溃恢复。全局和项目 Skill 的兼容 lock 仍可能被 `skills` CLI 或用户工具修改，因此继续在提交前重新读取并检查修订。

正式支持多个 Skill Deck 实例、增加独立后台写入进程、允许其他产品写入 Library，或者让多个设备共享同一 Library 数据目录时，必须重新讨论跨进程协调协议。当前行为由[系统架构](../architecture.md#应用内部结构)、[执行与恢复](../execution-and-recovery.md#原子写入与-lock-提交)和[测试与验证规范](../testing.md#http时间与外部进程)负责说明。
