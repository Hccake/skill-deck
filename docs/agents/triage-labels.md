# Triage 标签

仓库中的工程类 Skill 使用五个固定角色。下表列出这些角色在本仓库中对应的状态值。

| Skill 中的角色 | 本仓库状态值 | 含义 |
| --- | --- | --- |
| `needs-triage` | `needs-triage` | 等待维护者评估 |
| `needs-info` | `needs-info` | 等待报告者补充信息 |
| `ready-for-agent` | `ready-for-agent` | 信息完整，可交由 Agent 实施 |
| `ready-for-human` | `ready-for-human` | 需要人工实施 |
| `wontfix` | `wontfix` | 确认不予处理 |

当 Skill 提到某个 triage 角色时，应在 issue 的 `Status:` 中使用对应状态值。
