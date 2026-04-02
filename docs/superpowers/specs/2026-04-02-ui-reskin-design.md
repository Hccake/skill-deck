# UI Reskin Design Spec

**Date:** 2026-04-02
**Approach:** Skin Swap — keep all existing layouts, component structures, and business logic intact. Apply new visual DNA as a theme layer across the entire app.

**Reference:** `ui/screen.png` + `ui/code.html` (AI-generated design mockup, covers Skills page in split-view state only)

---

## 1. Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Border radius | 0px globally (sharp corners) | Match design mockup. Exception: `rounded-full` for avatar/pill shapes |
| Color system | Keep shadcn/ui CSS variable system | Minimal migration cost, all components auto-inherit |
| Color palette | Emerald (#006C4B primary) | Shift from teal (#0D9488) to match mockup's deeper green |
| Typography | Manrope (headings) + Inter (body) | Match mockup. Load via `@fontsource` packages (local, no CDN) |
| Icons | Lucide SVG only | Already in use. **No emoji icons anywhere** |
| Navigation | Underline tabs (left-aligned) | Replace center-aligned pill tabs to match mockup |
| Information density | Preserve existing | SkillCard keeps all fields (scope, name, risk, agents, etc.) |
| Uncovered pages | Extend style DNA | Discover, Settings, Wizard, Dark Mode — derive from approved style |

---

## 2. Global Foundation

### 2.1 CSS Variables — Light Mode

```css
:root {
  /* Font */
  --font-sans: 'Inter', sans-serif;
  --font-heading: 'Manrope', sans-serif;  /* NEW */
  --font-mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Monaco, Consolas, monospace;

  /* Radius */
  --radius: 0;  /* was: 0.5rem */

  /* Colors */
  --background: #F7F9FB;           /* was: #FFFFFF */
  --foreground: #191C1E;           /* was: #0F172A */
  --card: #FFFFFF;                 /* was: #F8FAFC */
  --card-foreground: #191C1E;      /* was: #0F172A */
  --popover: #FFFFFF;
  --popover-foreground: #191C1E;
  --primary: #006C4B;              /* was: #0D9488 */
  --primary-foreground: #FFFFFF;
  --secondary: #ECEEF0;            /* was: #F1F5F9 */
  --secondary-foreground: #3C4A42; /* was: #475569 */
  --muted: #ECEEF0;                /* was: #F1F5F9 */
  --muted-foreground: #6C7A71;     /* was: #64748B */
  --accent: #E0E3E5;               /* was: #F0FDFA */
  --accent-foreground: #2D3A32;    /* was: #115E59 — darkened for WCAG AA on #E0E3E5 (5.2:1) */
  --destructive: #EF4444;
  --border: #E6E8EA;               /* was: #E2E8F0 */
  --input: #E6E8EA;                /* was: #E2E8F0 */
  --ring: #006C4B;                 /* was: #0D9488 */

  /* Status — unchanged */
  --success: #22C55E;
  --warning: #F59E0B;

  /* Chart colors — shift to emerald palette */
  --chart-1: #006C4B;              /* was: #0D9488 */
  --chart-2: #22C55E;              /* unchanged */
  --chart-3: #F59E0B;              /* unchanged */
  --chart-4: #EF4444;              /* unchanged */
  --chart-5: #8B5CF6;              /* unchanged */

  /* Sidebar */
  --sidebar: #F2F4F6;              /* was: #F8FAFC */
  --sidebar-foreground: #191C1E;
  --sidebar-primary: #006C4B;      /* was: #0D9488 */
  --sidebar-primary-foreground: #FFFFFF;
  --sidebar-accent: #E0E3E5;       /* was: #F0FDFA */
  --sidebar-accent-foreground: #3C4A42;
  --sidebar-border: #E6E8EA;
  --sidebar-ring: #006C4B;
}
```

### 2.2 CSS Variables — Dark Mode

```css
.dark {
  --primary: #45DFA4;              /* was: #2DD4BF — mockup's inverse-primary */
  --primary-foreground: #002114;   /* was: #0F172A */
  --background: #0A1210;           /* was: #020617 — green undertone */
  --foreground: #F8FAFC;
  --card: #111C18;                 /* was: #0F172A — emerald card */
  --card-foreground: #F8FAFC;
  --popover: #111C18;
  --popover-foreground: #F8FAFC;
  --secondary: #1A2520;            /* was: #1E293B */
  --secondary-foreground: #D0D8D4; /* was: #E2E8F0 */
  --muted: #1A2520;                /* was: #1E293B */
  --muted-foreground: #8A9B90;     /* was: #94A3B8 — green-tinted */
  --accent: #0A2820;               /* was: #042F2E */
  --accent-foreground: #68FCBF;    /* was: #5EEAD4 */
  --destructive: #F87171;
  --border: rgba(255, 255, 255, 0.1);
  --input: rgba(255, 255, 255, 0.15);
  --ring: #45DFA4;                 /* was: #2DD4BF */

  --success: #4ADE80;
  --warning: #FBBF24;

  /* Chart colors — dark mode */
  --chart-1: #45DFA4;              /* was: #2DD4BF */
  --chart-2: #4ADE80;              /* unchanged */
  --chart-3: #FBBF24;              /* unchanged */
  --chart-4: #F87171;              /* unchanged */
  --chart-5: #A78BFA;              /* unchanged */

  --sidebar: #111C18;
  --sidebar-foreground: #F8FAFC;
  --sidebar-primary: #45DFA4;
  --sidebar-primary-foreground: #002114;
  --sidebar-accent: #0A2820;
  --sidebar-accent-foreground: #68FCBF;
  --sidebar-border: rgba(255, 255, 255, 0.1);
  --sidebar-ring: #45DFA4;
}
```

### 2.3 Typography Setup

**Install packages:**
```bash
pnpm add @fontsource/manrope @fontsource/inter
```

**Import in `main.tsx`:**
```ts
// Manrope — only weights actually used (600=semibold, 700=bold, 800=extrabold)
import '@fontsource/manrope/600.css';
import '@fontsource/manrope/700.css';
import '@fontsource/manrope/800.css';
// Inter — body text weights
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
```

**Usage convention:**
- Headings, nav labels, brand text: `font-heading` (Manrope) + `font-bold` or `font-extrabold` + `tracking-tight` or `-0.5px`
- Body text, descriptions, labels: `font-sans` (Inter, default) — no extra class needed
- Monospace (paths, code): `font-mono` — unchanged

### 2.4 Border Radius

Change `--radius: 0.5rem` to `--radius: 0` in `:root`.

All derived radius tokens (`--radius-sm`, `--radius-md`, etc.) compute to 0 or negative (clamped to 0).

**Exception:** `rounded-full` (9999px) is NOT affected by `--radius` — it stays. Used for:
- Agent badge pills in Detail Panel
- Theme/language button circles (if kept)
- Status dot indicators

### 2.5 Tailwind v4 `@theme inline` Config

The project uses Tailwind CSS v4 with `@theme inline {}` block in `index.css` (not a separate config file). Add font variables inside the existing `@theme inline {}` block:

```css
@theme inline {
  /* Existing radius/color vars... */
  --font-sans: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
  --font-heading: 'Manrope', sans-serif;  /* NEW */
}
```

This enables the `font-heading` utility class. Usage: `className="font-heading font-bold tracking-tight"`

**Note:** `--font-sans` override in `@theme inline` takes priority over the `:root` `--font-sans` declaration. Both must be updated.

---

## 3. Component Changes

### 3.1 Header (`components/layout/Header.tsx`)

**Layout change:** Logo + nav left-aligned inline; tools right-aligned.

| Element | Current | New |
|---------|---------|-----|
| Container | `justify-between` (3-column: logo / center-nav / tools) | `justify-between` (2-column: logo+nav / tools) |
| Logo icon | `rounded-lg bg-gradient-to-br from-teal-500 to-teal-600 shadow-md` | Sharp square, `bg-primary`, no gradient/shadow |
| Brand text | `font-medium text-foreground` | `font-heading font-extrabold text-primary tracking-tighter` |
| Nav container | `rounded-full bg-muted p-1` (pill wrapper) | Remove wrapper entirely |
| Nav links | `rounded-full`, active: `bg-foreground text-background shadow-sm` | `h-full border-b-2`, active: `border-primary text-primary font-heading font-bold`, inactive: `border-transparent text-muted-foreground` |
| Language button | `rounded-full bg-muted` circle | Simpler text button, no circle bg |
| Theme button | `rounded-full bg-muted` circle | Ghost button, no circle bg |

### 3.2 ContextSidebar (`components/skills/ContextSidebar.tsx`)

**Add section headers. Change active state to left border accent.**

| Element | Current | New |
|---------|---------|-----|
| Section headers | None | Add "GLOBAL" and "PROJECTS" labels: `font-heading text-[10px] font-extrabold uppercase tracking-[0.2em] text-muted-foreground` |
| Global item | `rounded-md bg-foreground/[0.06]` (selected) | `border-l-4 border-primary bg-primary/10` (left accent) |
| Project items | `rounded-md bg-foreground/[0.06]` (selected) | Same left accent pattern. Inactive: `border-l-4 border-transparent` |
| Active text | `text-foreground font-medium` | `text-primary font-heading font-bold` |
| Path subtitle | `text-muted-foreground/60` | Same position, adjust to new muted token |
| Add button | Inline with project list | Move to bottom with `border-t border-border` separator. `bg-accent` background |
| Hover actions | Ghost buttons for open/delete | Keep as-is, inherit new color tokens |

### 3.3 SkillCard (`components/skills/SkillCard.tsx`)

**CSS-only changes. All props, logic, and information density preserved.**

| Element | Current | New |
|---------|---------|-----|
| Card | `rounded-*` (from Card component) | Sharp (global --radius: 0) |
| Scope icon box | `rounded-lg bg-accent` | Sharp, `bg-accent` (new #E0E3E5) |
| Skill name | `font-semibold` | `font-heading font-bold tracking-tight` |
| Risk badge | Uses Badge `variant="outline"` | Auto-inherits sharp corners via global `--radius: 0`. No manual change needed |
| Plugin badge | `rounded` variant secondary | Sharp, secondary bg |
| Agent badges | `rounded-md border-border/40 bg-accent` | Sharp, `bg-primary/[0.08] border-primary/15`. Keep green dot |
| Git ref badge | `rounded` | Sharp |
| Progress bar | `rounded-b-xl` | Sharp (bottom corners 0) |
| Update left border | `border-l-2 border-l-warning` | Keep as-is |

### 3.4 CompactSkillItem (`components/skills/CompactSkillItem.tsx`)

**Match design mockup's compact list style.**

| Element | Current | New |
|---------|---------|-----|
| Container | `rounded-md`, border transparent | No rounded. Remove border |
| Selected state | `bg-primary/10` + left 3px bar with `rounded-r-md` | `bg-primary/[0.06] border-y border-primary/15` |
| Selected left bar | `w-[3px] bg-primary rounded-r-md` | Remove entirely (border-y replaces it) |
| Selected name | `text-primary` | `font-heading font-bold text-primary` |
| Selected description | `text-foreground/60` | `text-primary/60` |
| Normal name | `text-foreground/80 font-medium` | `font-heading font-semibold text-foreground` |
| Normal description | `text-foreground/60` | `text-muted-foreground` |
| Padding | `px-3 py-2` | `px-4 py-2.5` (slightly more generous) |

### 3.5 CompactSkillList (`components/skills/CompactSkillList.tsx`)

| Element | Current | New |
|---------|---------|-----|
| Section headers | `text-[11px] font-semibold uppercase tracking-wider` | `font-heading text-[10px] font-extrabold uppercase tracking-[0.2em]` (match sidebar) |
| Count suffix | `"{title} · {count}"` | Remove count — just section name |

### 3.6 SkillDetailPanel (`components/skills/SkillDetailPanel.tsx`)

**Add large title, metadata grid, and agents row. Keep all existing data fields.**

| Element | Current | New |
|---------|---------|-----|
| Sticky header | Small name + actions | Keep as-is, inherit new colors |
| **NEW: Hero title** | N/A | Add `font-heading text-2xl sm:text-3xl font-extrabold tracking-tight` using `skill.name` at top of scroll area |
| **NEW: Source link** | Inline in meta bar | Dedicated line below title: Link icon + primary color + font-medium |
| **NEW: Metadata grid** | Single row of inline items | 3-column grid: "INSTALLED" / "LAST UPDATED" / "INSTALL PATH" with uppercase labels (same style as sidebar section headers) |
| Agents | Inline badges in meta bar | Dedicated row with "AGENTS" uppercase label. Badges use `rounded-full bg-primary/10 text-primary font-bold` (pill exception) |
| Path display | `rounded` mono code | Sharp, `bg-sidebar px-2 py-1` |
| Padding | `px-4 py-4 sm:px-6 sm:py-5` | `px-6 py-6 sm:px-8 sm:py-6` (more spacious) |
| Git ref | `rounded` inline | Sharp, inline with source |
| Markdown `.skill-prose` headings | System font | `font-heading` for h1, h2, h3 |
| Markdown `.skill-prose pre` | `rounded-lg` | Sharp (remove rounded-lg) |
| Markdown `.skill-prose code` | `rounded-sm` | Sharp |
| Markdown `.skill-prose img` | `rounded-md` | Sharp |

### 3.7 SkillsToolbar + SkillsSection + EmptyStates

| Element | Change |
|---------|--------|
| Search input | Auto-inherit sharp corners + new bg from global tokens |
| Section title in SkillsSection | Add `font-heading font-extrabold` |
| Add/Sync/Update buttons | Auto-inherit from Button token changes |

**EmptyStates.tsx** (`components/skills/EmptyStates.tsx`):

| Element | Current | New |
|---------|---------|-----|
| GlobalEmptyState icon container | `rounded-2xl bg-gradient-to-br from-blue-500 to-indigo-600` | Sharp square, `bg-primary` (remove gradient). Keep Lucide `Package` icon |
| GlobalEmptyState glow effects | `blur-2xl rounded-full`, `blur-lg` | Remove blur/glow effects. Use flat `bg-primary/5` background instead |
| GlobalEmptyState sparkle badge | `rounded-full bg-amber-400` | Keep `rounded-full` (circle exception) |
| ProjectEmptyState container | `rounded-xl border-dashed` | Sharp (`rounded-xl` removed by global). Keep `border-dashed` |
| ProjectEmptyState icon | `rounded-full bg-muted` | Keep `rounded-full` (circle exception) |
| Heading text | `font-semibold` | `font-heading font-bold` |

### 3.8 Discover Page (`pages/DiscoverPage.tsx` + `SkillSearch.tsx`)

| Element | Change |
|---------|--------|
| Search input | Auto-inherit sharp corners |
| Result item name | Add `font-heading font-semibold tracking-tight` |
| Install button | Consider `variant="default"` (primary bg) instead of `variant="outline"` for stronger CTA |
| Installed badge | Auto-inherit from Badge secondary token |
| Skeleton loader | Auto-inherit sharp corners |

### 3.9 Settings Page (`pages/SettingsPage.tsx`)

| Element | Change |
|---------|--------|
| **TabsList** | Override from pill to underline style. Active: `border-b-2 border-primary text-primary bg-transparent`. Remove `rounded-*` and bg on TabsTrigger |
| Section heading icon | `rounded-lg` → sharp (global). New accent bg |
| Section heading text | Add `font-heading font-bold` |
| Cards | Auto-inherit sharp corners + border |
| Empty states | `rounded-xl` → sharp. Keep `rounded-full` on icon circle |
| Checkbox | Auto-inherit primary color |

### 3.10 Wizard (`pages/WizardPage.tsx` + step components)

All wizard step components (`ScopeStep`, `SourceStep`, `SkillsStep`, `OptionsStep`, `ConfirmStep`, `InstallingStep`, `CompleteStep`, `ErrorStep`) use shadcn primitives exclusively and auto-inherit global token changes. Specific items:

| Element | Change |
|---------|--------|
| All shadcn components (Input, Checkbox, RadioGroup, Card, Button, Progress) | Auto-inherit via global token changes |
| Step titles in each step | Add `font-heading` for consistency |
| `StepIndicator` | Auto-inherit primary color for active dot/line |
| `ScopeBadge` | Auto-inherit sharp corners + new colors |
| Agent badges in `ConfirmStep` | Same restyling as SkillCard agents: `bg-primary/[0.08] border-primary/15`, sharp |
| `AgentSelector` (shared with Settings) | Checkbox auto-inherits primary color |

No wizard files need manual edits — all changes flow through global tokens.

---

## 4. Files to Modify

### Must change (manual edits required):

| File | Scope of change |
|------|-----------------|
| `src/index.css` | All CSS variable values (light + dark), `--radius: 0`, add `--font-heading`, update `.skill-prose` |
| `src/main.tsx` | Add `@fontsource/manrope` and `@fontsource/inter` imports |
| `src/components/layout/Header.tsx` | Restructure to left-aligned nav with underline tabs |
| `src/components/skills/ContextSidebar.tsx` | Add section headers, change active state, move Add button |
| `src/components/skills/SkillCard.tsx` | Add font-heading to name, restyle agent badges |
| `src/components/skills/CompactSkillItem.tsx` | New selected state (border-y), font changes |
| `src/components/skills/CompactSkillList.tsx` | Section header style, remove count |
| `src/components/skills/SkillDetailPanel.tsx` | Add hero title, metadata grid, agents row, increase padding |
| `src/components/skills/SkillsSection.tsx` | Section title font |
| `src/components/skills/skill-search/SkillSearch.tsx` | Result item font, install button variant |
| `src/pages/SettingsPage.tsx` | TabsList underline override, section heading fonts |
| `src/components/skills/EmptyStates.tsx` | Remove `rounded-xl`, `rounded-2xl`; keep `rounded-full` on icon circles |

### Auto-inherit (no changes needed):

All shadcn/ui components in `src/components/ui/` — they read from CSS variables. Specifically:
- Button, Badge, Card, Input, Checkbox, Dialog, AlertDialog, DropdownMenu, Select, Progress, Skeleton, Tabs (partially), Tooltip, ScrollArea, Separator, Switch, Toggle

### New files:

| File | Purpose |
|------|---------|
| None | No new component files. Font packages added via pnpm |

### New dependencies:

```bash
pnpm add @fontsource/manrope @fontsource/inter
```

---

## 5. Constraints

- **No logic changes.** No store modifications, no new state, no API changes
- **No new shadcn components needed.** Existing 23 components are sufficient
- **No emoji icons.** All icons must use Lucide SVG (`lucide-react`)
- **i18n unchanged.** No new translation keys required
- **`bindings.ts` untouched.** No Rust changes
- **Dark mode must work.** Derive from emerald palette, verify all pages
- **Verification after implementation:** `pnpm test && pnpm lint && pnpm build`

---

## 6. Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Contrast issues in dark mode | Medium | Verify WCAG 4.5:1 for: `--muted-foreground` (#8A9B90) on `--card` (#111C18) ≈ 4.7:1 (borderline AA). Adjust if needed |
| TabsList underline override conflicts with shadcn updates | Low | Use CSS class override, not fork the component |
| `--radius: 0` breaks some component visuals | Low | Test all components; `rounded-full` is unaffected |
| Font loading delay (FOUT) | Low | `@fontsource` bundles are local, not CDN — near-zero latency in Tauri |
