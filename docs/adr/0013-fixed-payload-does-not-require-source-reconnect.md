---
status: accepted
---

# 跨 Environment Copy 在 payload 固定后不依赖来源重连

跨 Environment Copy 有独立的来源 Environment 和目标 Environment。来源 payload 已经固定后，Execute 只需验证 payload lease、目标 storage owner、目标 revision 和当前 authority；来源 Environment 断开不会使快照失效，也不触发自动重连或重新获取，但目标 Environment 仍必须在线。来源在 payload 固定前不可用，仍按普通获取失败处理。

Install、Update、Repair 没有独立的来源 Environment：当前 Environment 同时负责获取内容和执行写入。它们遵循的共同规则是 payload 固定后不重新读取原始来源，但当前 Environment 断开时操作仍然无法继续。这样既保留跨环境复制的必要解耦，也不为普通操作虚构 source reconnect 语义。
