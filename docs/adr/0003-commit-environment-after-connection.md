---
status: accepted
---

# 环境连接成功后独立提交项目加载状态

Skill Deck 在用户切换到某个 Environment 时，只要环境连接成功就提交新的 Environment；项目注册信息随后独立加载，并以加载中、可用或失败状态呈现。这样，项目配置损坏或读取失败不会把用户困在旧 Environment 中，用户仍可以进入目标 Environment 的 Global Context（全局上下文）并处理项目加载问题。我们放弃“连接和项目加载全部成功才切换”的原子语义，因为附属项目数据失败不应阻断环境级恢复入口。
