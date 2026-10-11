# 本地 Issue 与 Spec

本仓库使用 `.scratch/` 下的 Markdown 文件管理设计方案（spec）和实施事项（Issue）。普通实施 Issue 使用评估状态，Wayfinding 流程中由任务地图管理的子任务使用执行状态。

## 文件约定

- 每项功能使用一个目录：`.scratch/<feature-slug>/`
- Spec 位于 `.scratch/<feature-slug>/spec.md`
- 实施 issue 分别保存为 `.scratch/<feature-slug>/issues/<NN>-<slug>.md`
- Issue 从 `01` 开始编号，每项单独保存
- 每个 Issue 在文件开头附近使用 `Status:` 记录所属任务类型的状态
- 评论和讨论记录追加到文件末尾的 `## Comments` 下

## 状态约定

| 任务类型 | `Status:` 的含义 | 状态依据 |
|---|---|---|
| 普通实施 Issue | 评估与分流结果（triage），说明信息完整程度及后续处理方 | [Issue 评估状态](./triage-labels.md) |
| Wayfinding 子任务 | 任务的执行进度 | 本文的 [Wayfinding 子任务](#wayfinding-子任务)约定 |

普通实施 Issue 的评估状态与执行结果分别记录，实施情况写入 `## Comments`。由 `map.md` 管理的 Wayfinding 子任务遵循下文的领取和完成规则。

## 发布到 issue tracker

当 Skill 要求“publish to the issue tracker”时，在 `.scratch/<feature-slug>/` 下创建对应文件，并按需创建目录。

## 获取相关 ticket

当 Skill 要求“fetch the relevant ticket”时，读取用户指定的文件路径或 issue 编号。

## Wayfinding 子任务

Wayfinding 流程由 `wayfinder` Skill 使用任务地图（`map.md`）管理多个子任务，记录它们的结论与依赖关系。下列 `Status:` 表示执行进度，不使用普通实施 Issue 的 triage 状态。

- 任务地图：`.scratch/<effort>/map.md`，保存任务笔记（Notes）、已有决定（Decisions-so-far）和待澄清事项（Fog）
- 子任务：`.scratch/<effort>/issues/<NN>-<slug>.md`，从 `01` 开始编号
- `Type:` 记录 ticket 类型：`research`、`prototype`、`grilling` 或 `task`
- `Status:` 记录执行状态：`open`、`claimed` 或 `resolved`；新 ticket 使用 `open`
- `Blocked by: NN, NN` 记录阻塞关系；列出的 ticket 全部变为 `resolved` 后，当前 ticket 才解除阻塞
- 选择下一项（Frontier）：按编号查找状态为 `open` 且未被阻塞的第一个子任务
- 领取任务（Claim）：开始工作前将 `Status:` 更新为 `claimed` 并保存
- 完成任务（Resolve）：在 `## Answer` 下追加结论，将 `Status:` 更新为 `resolved`，然后在 `map.md` 的 Decisions-so-far 中追加上下文链接
