---
status: accepted
---

# Skill 库采用单写者持久化模型

Skill 库的元数据文档和成员目录只由当前应用实例写入。Tauri 单实例机制限制实例数量，`RuntimeAdmissionCoordinator` 统一控制持久化写入许可，库的运行时适配器按 Environment 串行化元数据、成员内容和内部恢复 I/O。

这项取舍省去了 Native 文件锁和 WSL `flock` 协调，同时保留条件提交、原子文档、目录备份和崩溃恢复。库元数据不支持其他进程或用户工具直接写入；全局和项目 Skill 的兼容 lock 仍需处理第三方工具修改，因此在提交前重新读取并检查修订。

正式支持多个应用实例、独立后台写入进程、其他产品写入库，或者多设备共享库目录时，需要重新讨论跨进程协调协议。当前模块边界见[系统架构](../architecture.md#运行时组装与写入许可)，事务见[执行与恢复](../execution-and-recovery.md#库内容事务)，验证见[测试与验证规范](../testing.md#skill-库与目录规划)。
