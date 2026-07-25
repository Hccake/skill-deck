---
status: accepted
---

# Eve placement 兼容 vendored skills CLI

Eve 的 root、具名 subagent 和 multiple placement 采用仓库内 vendored `vercel-skills` CLI 的共享 lock 契约；update/remove 保留已有 placement，新写入使用 CLI 可理解的编码。明确的 `subagents: [""]` 表示 root；当 entry 已由当前 facts 证明属于 Eve 时，缺失字段按 CLI 兼容规则读取为 legacy root，但不自动写回；无法确认 Eve 身份时，缺失字段保持 unknown，并在 preview 中要求确认。没有 Eve target 时不产生 placement，也不把 `subagents: []` 定义成 Skill Deck 专属的“无目标”协议。Skill Deck 只抽取 placement 语义模块并叠加自己的 preview、原子写入和安全校验，不复制 CLI 内部实现，也不把 Eve 收缩成 root-only 特例，以保持外部 lock 的互操作性。
