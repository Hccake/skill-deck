---
status: accepted
---

# WSL 以最小用户态契约作为正式支持边界

Skill Deck 正式支持常见 Ubuntu/Debian 等 GNU/Linux 用户态，以及 Git、POSIX shell 和当前操作所需的 GNU coreutils；连接时只执行一次满足或不满足该基线的二元 preflight，失败时返回缺失条件。具体操作可以保留自己的窄 preflight，但项目不把 WSL 发行版兼容性扩展成五项 capability matrix、复杂 shell fallback 或多 distro 的完整 acceptance 契约，因为 Skill Deck 的职责是管理 Skill，而不是维护一层通用 WSL 兼容运行时。
