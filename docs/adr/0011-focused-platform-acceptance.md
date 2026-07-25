---
status: accepted
---

# 高层验证聚焦主发布平台与单一 WSL 基线

Skill Deck 保留三平台 compile、unit、adapter 和 contract 测试，但不建设三平台完整 GUI E2E 或多 distro WSL acceptance matrix。低频 started-app smoke 聚焦主发布平台的 loader、窗口、plugin、HTTP、外链和 Wizard 关键旅程；真实 WSL acceptance 只使用一个 reference distro，并且只有在 runner、owner 和最近结果都可查时才进入仓库。没有实际执行能力的 workflow 不是质量门禁，应删除而不是长期留作形式入口。
