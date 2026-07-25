---
status: accepted
---

# Runtime Maintenance 失败通过重新进入恢复

Runtime Maintenance 失败后，Skill Deck 不在进程内维护独立的 retry、backoff 或倒计时状态机；用户处理 Environment 问题后重新连接或重启应用，系统以新的运行时事实重新执行维护。实现必须避免把同一 revision 的旧失败永久复用，否则“重连/重启可恢复”只是文案而不是可达路径。
