---
status: accepted
---

# Environment 本地文档不保存运行时 Environment 身份

`EnvironmentRef` 表示桌面主进程如何路由 Native 或 WSL 操作。同一 Linux 用户空间经由 Linux 桌面应用和 Windows WSL 访问时，运行时表示不同，但持久化数据仍属于同一用户空间。

Environment 本地文档的归属由存储接口、平台适配器和存储路径确定，因此只保存业务状态和 Environment 内部键。存储接口显式接收操作位置，并在加载后绑定当前 Environment。桌面主进程的本地配置需要恢复用户选择或跨 Environment 路由时，仍可保存 `EnvironmentRef`。

库应用记录采用这项规则。恢复标记仍沿用现有兼容格式，包含运行时 Environment 和 `ResourceLocator`；其格式演进需要单独处理已有恢复数据的兼容性。

同一 Environment 本地存储仍采用单写者模型。Linux 桌面应用与 Windows 桌面应用可以顺序访问同一个 WSL 用户空间；正式支持多个主进程同时写入时，需要重新讨论 [ADR-0010](./0010-use-single-writer-library-persistence.md) 中的协调协议。当前数据归属见[系统架构](../architecture.md#数据与状态归属)，恢复格式见[执行与恢复](../execution-and-recovery.md#恢复记录与恢复数据)。
