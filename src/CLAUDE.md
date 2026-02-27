# src/CLAUDE.md — Frontend Rules

## Store Interaction Pattern

三个 Zustand store 有明确的依赖关系：

```
context.ts ← skills.ts 通过 getState() 读取 selectedContext
             settings.ts 独立，持久化到 localStorage + 后端
```

- `stores/context.ts`: 管理当前 scope（`'global'` 或项目路径），项目列表。调用后端 config API
- `stores/skills.ts`: 通过 `useContextStore.getState()` 读取当前 context。NEVER 在 skills store 中使用 `useContextStore` hook（会导致无限循环）
- `stores/settings.ts`: theme/locale 通过 zustand `persist` 中间件持久化到 localStorage（`partialize` 只存 theme + locale）。defaultAgents 通过后端 API 持久化

## Component Conventions

- 页面组件（`pages/`）使用 `useTranslation()` hook
- Feature 组件放在 `components/skills/`，通过 `index.ts` barrel export
- UI 原子组件只用 shadcn/ui（见根 CLAUDE.md 的组件对照表）
- 多步向导模式参考 `components/skills/add-skill/` 的 step 组件结构
- 所有用户可见文本 MUST 使用 `t()` 函数，i18n key 同时维护 en + zh-CN

## Data Flow

```
React Component（UI 层）
  → stores/*.ts（状态管理层，Zustand）
    → hooks/useTauriApi.ts（API 封装层，unwrap Result<T, AppError>）
      → bindings.ts（自动生成，DO NOT edit）
        → Tauri invoke → Rust commands
```

Component 层 NEVER 直接调用 bindings.ts，MUST 通过 useTauriApi.ts 的 wrapper 函数。
