# Issue 管理：Local Markdown

本仓库使用 `.scratch/` 下的 Markdown 文件管理 issue 和 spec。

## 文件约定

- 每项功能使用一个目录：`.scratch/<feature-slug>/`
- Spec 位于 `.scratch/<feature-slug>/spec.md`
- 实施 issue 分别保存为 `.scratch/<feature-slug>/issues/<NN>-<slug>.md`
- Issue 从 `01` 开始编号，不使用合并的 tickets 文件
- 每个 issue 在文件开头附近使用 `Status:` 记录 triage 状态，状态值参见 `triage-labels.md`
- 评论和讨论记录追加到文件末尾的 `## Comments` 下

## 发布到 issue tracker

当 Skill 要求“publish to the issue tracker”时，在 `.scratch/<feature-slug>/` 下创建对应文件，并按需创建目录。

## 获取相关 ticket

当 Skill 要求“fetch the relevant ticket”时，读取用户指定的文件路径或 issue 编号。

## Wayfinding 操作

`wayfinder` 使用一个 map 文件管理多个子 ticket。

- Map：`.scratch/<effort>/map.md`，保存 Notes、Decisions-so-far 和 Fog
- 子 ticket：`.scratch/<effort>/issues/<NN>-<slug>.md`，从 `01` 开始编号
- `Type:` 记录 ticket 类型：`research`、`prototype`、`grilling` 或 `task`
- `Status:` 记录执行状态：`open`、`claimed` 或 `resolved`；新 ticket 使用 `open`
- `Blocked by: NN, NN` 记录阻塞关系；列出的 ticket 全部变为 `resolved` 后，当前 ticket 才解除阻塞
- Frontier：按编号查找状态为 `open` 且未被阻塞的第一个 ticket
- Claim：开始工作前将 `Status:` 更新为 `claimed` 并保存
- Resolve：在 `## Answer` 下追加结论，将 `Status:` 更新为 `resolved`，然后在 `map.md` 的 Decisions-so-far 中追加上下文链接
