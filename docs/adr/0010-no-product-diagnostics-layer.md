---
status: accepted
---

# 不建设独立的 Diagnostics 产品层

Skill Deck 删除自建的持久化 diagnostics recorder、Settings 复制或打开入口、专用 command、bindings 和 ACL，只保留标准本地日志以及业务结果中的 stable error code、parameters、Environment、operation 和 recovery identity。没有真实 support intake 流程时，维护自由文本脱敏、滚动存储和导出协议的成本高于产品价值；technical details 不成为 Frontend 或用户可见契约。
