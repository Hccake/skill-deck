# Issue 评估状态

普通实施 Issue 使用评估与分流状态（triage），说明信息是否充分以及后续由谁处理。工程类 Skill 中的五个 triage 角色对应以下状态值。

| Skill 中的角色 | 本仓库状态值 | 含义 |
| --- | --- | --- |
| `needs-triage` | `needs-triage` | 等待维护者评估 |
| `needs-info` | `needs-info` | 等待报告者补充信息 |
| `ready-for-agent` | `ready-for-agent` | 信息完整，可交由 Agent 实施 |
| `ready-for-human` | `ready-for-human` | 需要人工实施 |
| `wontfix` | `wontfix` | 确认不予处理 |

普通实施 Issue 在 `Status:` 中使用对应状态值。Wayfinding 子任务的 `Status:` 表示执行进度，遵循[本地 Issue 与 Spec](./issue-tracker.md#wayfinding-子任务)中的独立约定。
