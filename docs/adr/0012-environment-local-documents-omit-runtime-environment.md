---
status: accepted
---

# Environment 本地文档不保存运行时 Environment 身份

`EnvironmentRef` 表示当前 Host 如何路由 Native 或 WSL 操作，同一 Linux 用户空间由 Linux Native 和 Windows WSL 访问时会得到不同的运行时表示。Environment 本地文档的归属已经由 Store、Adapter 和存储路径确定，因此只保存业务状态和 Environment 内部键；Repository Interface 显式接收运行时 Context，并在加载后绑定当前 Environment。Host 本地配置需要恢复用户选择或跨 Environment 路由时仍可保存 `EnvironmentRef`。

库应用记录在首次发布前按该规则删除 `target`，旧开发记录中的多余字段由当前 reader 忽略。Recovery marker 仍包含运行时 Environment 和 `ResourceLocator`，在迭代 5 迁移 Recovery 时按同一规则拆分磁盘状态与运行时索引，不在本次兼容修复中提前改写恢复协议。

同一 Environment 本地存储仍采用单写者模型。Linux Native Skill Deck 与 Windows Skill Deck 可以顺序访问同一个 WSL 用户空间，但不能同时写入；正式支持多个跨 Host 写入进程时需要重新讨论 ADR-0010 的协调协议。
