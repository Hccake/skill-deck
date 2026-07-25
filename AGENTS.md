<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **skill-deck** (7498 symbols, 22008 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> Index stale? Run `node .gitnexus/run.cjs analyze` from the project root — it auto-selects an available runner. No `.gitnexus/run.cjs` yet? `npx gitnexus analyze` (npm 11 crash → `npm i -g gitnexus`; #1939).

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows. For regression review, compare against the default branch: `detect_changes({scope: "compare", base_ref: "main"})`.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `rename` which understands the call graph.
- NEVER commit changes without running `detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/skill-deck/context` | Codebase overview, check index freshness |
| `gitnexus://repo/skill-deck/clusters` | All functional areas |
| `gitnexus://repo/skill-deck/processes` | All execution flows |
| `gitnexus://repo/skill-deck/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->

## 项目协作规则

- 与用户沟通和长期维护文档默认使用符合中文语境的陈述体。Agent、Skill、Environment、Context、Backend、WSL 等专业术语可以保留 English 原文。
- 开始任务时先阅读[文档地图](./docs/README.md)，按其中的任务路由进入唯一 owner 文档。不要在 `AGENTS.md` 复制产品、架构或领域正文。
- 当前工作区可能包含尚未提交的用户改动。只修改本任务需要的文件，不回退、不覆盖、不顺带整理无关内容。
- `docs/plans/**` 与 `docs/superpowers/**` 保存本地设计、计划和 review 过程，不属于 tracked authority，不得 stage 或 commit。
- 代码变更遵循 test-first，并按[贡献指南](./CONTRIBUTING.md)同步 bindings、ACL、i18n、长期文档和验证。
- 完成前运行与改动范围相符的验证，并依据最新输出报告结果。
- 仓库只保留这一份共享 Agent instruction。不要创建目录级 `AGENTS.md` 或 `CLAUDE.md`；工具专属入口只引用本文件。
