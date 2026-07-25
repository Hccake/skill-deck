---
status: accepted
---

# 受保护写入跟随 storage owner

Skill Deck 不在当前执行 Environment 与实际 storage owner 不一致时执行受保护写入；安装、更新、删除和 Manage Agents 都要求用户位于目标 storage owner Environment。Copy 是有意保留的跨 Environment 例外：来源 Environment 可以固定并传递完整 Skill 内容，但每个目标 Project 必须由目标 Environment（且该 Environment 必须是目标 storage owner）执行自己的受保护写入。跨 storage 访问仍可用于读取事实、显示风险和引导切换，但不通过 capability fallback 扩展为写入模式。这样既支持合法的跨 Environment 内容复制，也保持 physical identity、lock、rename 和 Recovery ownership 的边界稳定，避免为跨存储直写维护一套低频事务协议。
