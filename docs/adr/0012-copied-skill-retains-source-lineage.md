---
status: accepted
---

# 跨 Environment copy 区分来源 lineage 与 Local provenance

跨 Environment copy 在目标 Project 中始终生成独立的 Skill materialization。对 Remote、Git 和 Well-known 等可重新获取的来源，目标 lock 保留 source、ref、Skill path 和内容基线，后续更新直接在目标 storage owner Environment 根据这些 metadata 重新获取，不要求来源 Environment 或来源 Project 仍然存在；对 Local 来源，目标只保留原始路径和内容 hash 作为 provenance，并沿用源 Skill 已有的不可更新能力。Copy 不改变 source capability，也不为 Local path 发明偏离 vendored `vercel-skills` CLI 的后台同步协议。
