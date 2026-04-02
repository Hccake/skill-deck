# UI Reskin Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apply "Skin Swap" visual redesign across the entire Skill Deck app — sharp corners, Manrope+Inter typography, emerald palette, underline navigation — without changing any business logic.

**Architecture:** Pure CSS/className changes organized into 4 phases by dependency layer. Phase 1 (global foundation) unlocks auto-inheritance for all downstream phases. Each phase produces a working, verifiable app state.

**Tech Stack:** Tailwind CSS v4, shadcn/ui CSS variables, `@fontsource/manrope`, `@fontsource/inter`, `lucide-react`

**Design Spec:** `docs/superpowers/specs/2026-04-02-ui-reskin-design.md`

---

## Phase 1: Global Foundation

> CSS variables, fonts, border-radius. After this phase, ~60% of the app auto-inherits the new style.

### Task 1.1: Install Font Packages

**Files:**
- Modify: `package.json`

- [ ] **Step 1: Install @fontsource packages**

```bash
rtk pnpm add @fontsource/manrope @fontsource/inter
```

- [ ] **Step 2: Verify installation**

```bash
ls node_modules/@fontsource/manrope node_modules/@fontsource/inter
```
Expected: Both directories exist

- [ ] **Step 3: Commit**

```bash
rtk git add package.json pnpm-lock.yaml
rtk git commit -m "deps: add @fontsource/manrope and @fontsource/inter"
```

---

### Task 1.2: Import Fonts in Entry Point

**Files:**
- Modify: `src/main.tsx`

- [ ] **Step 1: Add font imports before CSS import**

Add these lines BEFORE the `import './index.css'` line in `src/main.tsx`:

```ts
// Fonts — loaded locally via @fontsource (no CDN)
import '@fontsource/manrope/600.css';
import '@fontsource/manrope/700.css';
import '@fontsource/manrope/800.css';
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
```

- [ ] **Step 2: Verify build**

```bash
rtk pnpm build
```
Expected: Build succeeds with no errors

- [ ] **Step 3: Commit**

```bash
rtk git add src/main.tsx
rtk git commit -m "feat(ui): import Manrope and Inter fonts"
```

---

### Task 1.3: Update CSS Variables + Radius + Font Config

**Files:**
- Modify: `src/index.css`

This is the core change. Replace all CSS variable values in `:root` and `.dark`, update `@theme inline`, and change `--radius`.

- [ ] **Step 1: Update `@theme inline` block — add font variables**

In the `@theme inline {}` block in `src/index.css`, add after the existing radius/color lines:

```css
--font-sans: 'Inter', -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
--font-heading: 'Manrope', sans-serif;
```

- [ ] **Step 2: Update `:root` — radius, font-sans, font-heading**

Change:
```css
--radius: 0.5rem;
```
To:
```css
--radius: 0;
```

Change `--font-sans` to use Inter with system fallbacks:
```css
--font-sans: 'Inter', -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
             "Helvetica Neue", Arial, "Noto Sans", sans-serif;
```

Add `--font-heading` (new variable):
```css
--font-heading: 'Manrope', sans-serif;
```

Note: `--font-mono` remains unchanged. The longer fallback chain for `--font-sans` is intentional (desktop app needs system font fallbacks while web fonts load).

- [ ] **Step 3: Update `:root` — light mode colors**

Replace **every** color value in the `:root` block by copying from design spec Section 2.1 verbatim. The spec is the single source of truth — copy the entire CSS block from there, not this summary.

Variables that change (for quick reference — DO NOT use as sole source):
- `--background`, `--foreground`, `--card`, `--card-foreground`, `--popover-foreground`
- `--primary`, `--secondary`, `--secondary-foreground`, `--muted`, `--muted-foreground`
- `--accent`, `--accent-foreground`, `--border`, `--input`, `--ring`
- `--chart-1`
- All `--sidebar-*` tokens

Variables that stay the same: `--primary-foreground`, `--popover`, `--destructive`, `--success`, `--warning`, `--chart-2` through `--chart-5`

- [ ] **Step 4: Update `.dark` — dark mode colors**

Replace **every** color value in the `.dark` block by copying from design spec Section 2.2 verbatim. Same rule: spec is the single source of truth — copy the entire block.

All variables change in dark mode (for quick reference — DO NOT use as sole source):
- `--primary`, `--primary-foreground`, `--background`, `--foreground`
- `--card`, `--card-foreground`, `--popover`, `--popover-foreground`
- `--secondary`, `--secondary-foreground`, `--muted`, `--muted-foreground`
- `--accent`, `--accent-foreground`, `--destructive`
- `--border`, `--input`, `--ring`
- `--success`, `--warning`
- `--chart-1` through `--chart-5`
- All `--sidebar-*` tokens

- [ ] **Step 5: Update `.skill-prose` — font and rounded**

In the `.skill-prose` class rules:
- `.skill-prose h1` — add `font-family: var(--font-heading);`
- `.skill-prose h2` — add `font-family: var(--font-heading);`
- `.skill-prose h3` — add `font-family: var(--font-heading);`
- `.skill-prose pre` — change `rounded-lg` to remove (will be 0 from global)
- `.skill-prose code:not(pre code)` — change `rounded-sm` to remove
- `.skill-prose img` — change `rounded-md` to remove

- [ ] **Step 6: Run lint + build**

```bash
rtk pnpm lint && rtk pnpm build
```
Expected: Both pass

- [ ] **Step 7: Run tests**

```bash
rtk pnpm test
```
Expected: All tests pass (CSS-only changes should not affect logic tests)

- [ ] **Step 8: Commit**

```bash
rtk git add src/index.css
rtk git commit -m "feat(ui): update global CSS variables to emerald palette, sharp corners, new fonts"
```

---

## Phase 2: Header + ContextSidebar

> The two persistent chrome components visible on every page.

### Task 2.1: Restyle Header — Underline Navigation

**Files:**
- Modify: `src/components/layout/Header.tsx`

- [ ] **Step 1: Update `getNavLinkClass` function**

Replace the existing `getNavLinkClass` function (line ~16-23) with:

```tsx
const getNavLinkClass = ({ isActive }: { isActive: boolean }) =>
  cn(
    'h-full flex items-center gap-1.5 px-1 font-heading font-semibold text-sm tracking-tight transition-all active:scale-95',
    isActive
      ? 'text-primary border-b-2 border-primary font-bold'
      : 'text-muted-foreground hover:text-primary border-b-2 border-transparent'
  );
```

- [ ] **Step 2: Restructure header layout — logo + nav left, tools right**

Replace the `<header>` JSX. Key changes:
- Left side: logo + nav in one flex container with `gap-6`
- Logo icon: change from `rounded-lg bg-gradient-to-br from-teal-500 to-teal-600 shadow-md` to `bg-primary` (no rounded, no gradient, no shadow)
- Brand text: change to `font-heading font-extrabold text-primary tracking-tighter`
- Nav: remove `rounded-full bg-muted p-1` wrapper. Nav links are now direct children with `h-full` for underline alignment
- Right side: simplify language/theme buttons — remove `rounded-full bg-muted` circular wrappers

- [ ] **Step 3: Run lint + build**

```bash
rtk pnpm lint && rtk pnpm build
```
Expected: Both pass

- [ ] **Step 4: Visual verification**

```bash
rtk pnpm dev
```
Open in browser. Verify:
- Logo is sharp square with primary bg
- Nav tabs use underline style (no pills)
- Active tab has bottom border in primary color
- Language/theme buttons are simplified

- [ ] **Step 5: Commit**

```bash
rtk git add src/components/layout/Header.tsx
rtk git commit -m "feat(ui): restyle header with underline nav and sharp logo"
```

---

### Task 2.2: Restyle ContextSidebar

**Files:**
- Modify: `src/components/skills/ContextSidebar.tsx`

- [ ] **Step 1: Add section headers and change active states**

Key changes to `ContextSidebar` component:
1. Add `<h3>` section header before `GlobalContextItem`: `font-heading text-[10px] font-extrabold uppercase tracking-[0.2em] text-muted-foreground` with text from `t('context.global')` label
2. Add `<h3>` section header before projects list: same style with `t('context.projects')` (need to add i18n key)
3. Move Add Project button to bottom with `border-t border-border` separator and `bg-accent` background

- [ ] **Step 2: Update `GlobalContextItem` active state**

Change selected style from:
```
'bg-foreground/[0.06] text-foreground font-medium'
```
To:
```
'border-l-4 border-primary bg-primary/10 text-primary font-heading font-bold'
```
And inactive from:
```
'text-muted-foreground hover:bg-foreground/[0.03] hover:text-foreground'
```
To:
```
'border-l-4 border-transparent text-muted-foreground hover:bg-accent/50 hover:text-foreground'
```
Remove `rounded-md` from both states.

- [ ] **Step 3: Update `ProjectContextItem` active state**

Same pattern as GlobalContextItem. Change selected style to left accent border. Remove `rounded-md`.

Active text: `text-primary font-heading font-bold`

- [ ] **Step 4: Add i18n keys if needed**

If section header labels ("GLOBAL", "PROJECTS") require new i18n keys, add them to both:
- `src/i18n/locales/en.json`
- `src/i18n/locales/zh-CN.json`

Note: Check if existing keys like `context.global` can be reused for section headers. The design spec Section 5 states "No new translation keys required" but this was an oversight — new UI labels for section headers and metadata grid labels do need i18n keys per project convention.

- [ ] **Step 5: Run lint + build + tests**

```bash
rtk pnpm test && rtk pnpm lint && rtk pnpm build
```
Expected: All pass

- [ ] **Step 6: Commit**

```bash
rtk git add src/components/skills/ContextSidebar.tsx src/i18n/locales/en.json src/i18n/locales/zh-CN.json
rtk git commit -m "feat(ui): restyle sidebar with section headers and left accent active state"
```

---

## Phase 3: SkillCard + CompactList + DetailPanel

> The core skill display components on the Skills page.

### Task 3.1: Restyle SkillCard

**Files:**
- Modify: `src/components/skills/SkillCard.tsx`

- [ ] **Step 1: Update skill name styling**

Change `h3` class from `text-sm font-semibold text-foreground` to:
```
text-sm font-heading font-bold tracking-tight text-foreground
```

- [ ] **Step 2: Update scope icon box**

Change from `rounded-lg bg-accent` to `bg-accent` (remove `rounded-lg`).

- [ ] **Step 3: Update agent badges**

Change agent badge class from:
```
rounded-md border border-border/40 bg-accent px-2 sm:px-2.5 py-1.5 text-xs font-medium text-accent-foreground shadow-sm
```
To:
```
border border-primary/15 bg-primary/[0.08] px-2 sm:px-2.5 py-1.5 text-xs font-medium text-accent-foreground
```
(Remove `rounded-md` and `shadow-sm`)

- [ ] **Step 4: Update progress bar bottom corners**

Remove `rounded-b-xl` from the update progress bar containers (3 places: updating, done, failed states).

- [ ] **Step 5: Run lint + build + tests**

```bash
rtk pnpm test && rtk pnpm lint && rtk pnpm build
```
Expected: All pass

- [ ] **Step 6: Commit**

```bash
rtk git add src/components/skills/SkillCard.tsx
rtk git commit -m "feat(ui): restyle SkillCard with heading font and sharp corners"
```

---

### Task 3.2: Restyle CompactSkillItem + CompactSkillList

**Files:**
- Modify: `src/components/skills/CompactSkillItem.tsx`
- Modify: `src/components/skills/CompactSkillList.tsx`

- [ ] **Step 1: Update CompactSkillItem selected state**

Replace selected class from `bg-primary/10` to `bg-primary/[0.06] border-y border-primary/15`.

Remove the selected left bar div (`<div className="absolute left-0 top-1.5 bottom-1.5 w-[3px] bg-primary rounded-r-md" />`).

Update selected name class to `font-heading font-bold text-primary`.
Update selected description to `text-primary/60`.

Update normal name to `font-heading font-semibold text-foreground`.
Update normal description to `text-muted-foreground`.

Remove `rounded-md` and `border border-transparent` from container.

Change padding from `px-3 py-2` to `px-4 py-2.5`.

- [ ] **Step 2: Update CompactSkillList section headers**

Change section header class from:
```
text-[11px] font-semibold text-muted-foreground uppercase tracking-wider
```
To:
```
font-heading text-[10px] font-extrabold text-muted-foreground uppercase tracking-[0.2em]
```

Remove count suffix from section header text. Change `{projectTitle} · {projectSkills.length}` to just `{projectTitle}`, and `{t('skills.globalSkills')} · {globalSkills.length}` to just `{t('skills.globalSkills')}`.

- [ ] **Step 3: Run lint + build + tests**

```bash
rtk pnpm test && rtk pnpm lint && rtk pnpm build
```
Expected: All pass (check CompactSkillList tests specifically)

- [ ] **Step 4: Commit**

```bash
rtk git add src/components/skills/CompactSkillItem.tsx src/components/skills/CompactSkillList.tsx
rtk git commit -m "feat(ui): restyle compact skill list with new selection style and heading font"
```

---

### Task 3.3: Restyle SkillDetailPanel

**Files:**
- Modify: `src/components/skills/SkillDetailPanel.tsx`

- [ ] **Step 1: Add hero title at top of scroll content**

After the `<div className="px-4 py-4 sm:px-6 sm:py-5 w-full space-y-4">` (change to `px-6 py-6 sm:px-8 sm:py-6`), add before the meta properties section:

```tsx
{/* Hero title */}
<h2 className="text-2xl sm:text-3xl font-heading font-extrabold tracking-tight text-foreground">
  {skill.name}
</h2>
```

- [ ] **Step 2: Add source link below title**

If `skill.source && skill.sourceUrl`, render a dedicated source link line (import `Link2` from lucide-react):

```tsx
{skill.source && skill.sourceUrl ? (
  <a
    href={skill.sourceUrl}
    target="_blank"
    rel="noopener noreferrer"
    className="inline-flex items-center gap-1.5 text-sm text-primary font-medium hover:underline"
  >
    <Link2 className="h-3.5 w-3.5" />
    {skill.source}
  </a>
) : null}
```

- [ ] **Step 3: Restructure metadata into 3-column grid**

Replace the existing inline meta properties section with a grid layout:

```tsx
<div className="grid grid-cols-2 md:grid-cols-3 gap-4 pb-4 border-b border-border">
  {skill.installedAt ? (
    <div className="flex flex-col">
      <span className="font-heading text-[10px] uppercase font-bold text-muted-foreground tracking-[0.2em]">
        {t('skills.detail.installed')}
      </span>
      <span className="text-sm font-semibold text-accent-foreground mt-1">
        {formatTime(skill.installedAt, i18n.language)}
      </span>
    </div>
  ) : null}
  {skill.updatedAt ? (
    <div className="flex flex-col">
      <span className="font-heading text-[10px] uppercase font-bold text-muted-foreground tracking-[0.2em]">
        {t('skills.detail.updated')}
      </span>
      <span className="text-sm font-semibold text-accent-foreground mt-1">
        {formatTime(skill.updatedAt, i18n.language)}
      </span>
    </div>
  ) : null}
  <div className="flex flex-col col-span-2 md:col-span-1">
    <span className="font-heading text-[10px] uppercase font-bold text-muted-foreground tracking-[0.2em]">
      {t('skills.detail.installPath')}
    </span>
    <div className="flex items-center gap-1 mt-1">
      <code className="text-sm font-mono text-accent-foreground bg-sidebar px-2 py-1 truncate">
        {skill.canonicalPath}
      </code>
      {/* Copy button — keep existing handleCopyPath logic */}
    </div>
  </div>
</div>
```

- [ ] **Step 4: Restructure agents into dedicated row**

Below the metadata grid, add an agents row:

```tsx
{skill.agents.length > 0 ? (
  <div className="flex flex-wrap items-center gap-3">
    <span className="font-heading text-[10px] uppercase font-bold text-muted-foreground tracking-[0.2em]">
      {t('skills.detail.agents')}
    </span>
    <div className="flex flex-wrap gap-2">
      {skill.agents.map((agentId) => (
        <span
          key={agentId}
          className="inline-flex items-center gap-1.5 rounded-full bg-primary/10 px-3 py-1 text-[11px] font-bold text-primary"
        >
          {agentDisplayNames.get(agentId) ?? agentId}
        </span>
      ))}
    </div>
  </div>
) : null}
```

- [ ] **Step 5: Add i18n keys if needed**

Check and add any new keys (e.g., `skills.detail.installPath`, `skills.detail.agents`) to both locale files.

- [ ] **Step 6: Run lint + build + tests**

```bash
rtk pnpm test && rtk pnpm lint && rtk pnpm build
```
Expected: All pass

- [ ] **Step 7: Commit**

```bash
rtk git add src/components/skills/SkillDetailPanel.tsx src/i18n/locales/en.json src/i18n/locales/zh-CN.json
rtk git commit -m "feat(ui): restyle detail panel with hero title, metadata grid, agent badges row"
```

---

### Task 3.4: Restyle SkillsSection + EmptyStates

**Files:**
- Modify: `src/components/skills/SkillsSection.tsx`
- Modify: `src/components/skills/EmptyStates.tsx`

- [ ] **Step 1: Update SkillsSection title**

Find the section title element and add `font-heading font-extrabold` class.

- [ ] **Step 2: Update EmptyStates**

In `GlobalEmptyState`:
- Change icon container from `rounded-2xl bg-gradient-to-br from-blue-500 to-indigo-600 shadow-xl` to `bg-primary`
- Remove blur/glow effects (`blur-2xl`, `blur-lg`, `opacity-30` divs). Replace with simple `bg-primary/5` background
- Icon inside: keep `Package` Lucide icon, change to `text-primary-foreground`
- Heading: add `font-heading font-bold`

In `ProjectEmptyState`:
- `rounded-xl` auto-removed by global radius. No manual change needed for that
- Heading: add `font-heading font-bold`

- [ ] **Step 3: Run lint + build + tests**

```bash
rtk pnpm test && rtk pnpm lint && rtk pnpm build
```
Expected: All pass

- [ ] **Step 4: Commit**

```bash
rtk git add src/components/skills/SkillsSection.tsx src/components/skills/EmptyStates.tsx
rtk git commit -m "feat(ui): restyle section titles and empty states"
```

---

## Phase 4: Discover + Settings + Remaining Polish

> Pages that auto-inherit most changes, plus targeted tweaks.

### Task 4.1: Restyle Discover Page (SkillSearch)

**Files:**
- Modify: `src/components/skills/skill-search/SkillSearch.tsx`

- [ ] **Step 1: Update SearchResultItem name font**

Change the result name div class from `font-medium text-sm` to:
```
font-heading font-semibold text-sm tracking-tight
```

- [ ] **Step 2: Consider Install button variant**

Change the Install button from `variant="outline"` to `variant="default"` for a stronger CTA:
```tsx
<Button variant="default" size="sm" className="h-7 text-xs" onClick={() => onInstall(skill)}>
```

- [ ] **Step 3: Run test + lint + build**

```bash
rtk pnpm test && rtk pnpm lint && rtk pnpm build
```
Expected: All pass

- [ ] **Step 4: Commit**

```bash
rtk git add src/components/skills/skill-search/SkillSearch.tsx
rtk git commit -m "feat(ui): restyle discover page with heading font and primary CTA"
```

---

### Task 4.2: Restyle Settings Page — Underline Tabs

**Files:**
- Modify: `src/pages/SettingsPage.tsx`

- [ ] **Step 1: Override TabsList to underline style**

The shadcn `TabsList` uses a pill/muted background by default. Override with custom className:

```tsx
<TabsList className="mb-5 bg-transparent border-b border-border rounded-none h-auto p-0 gap-0">
  <TabsTrigger
    value="general"
    className="rounded-none border-b-2 border-transparent data-[state=active]:border-primary data-[state=active]:text-primary data-[state=active]:bg-transparent data-[state=active]:shadow-none font-heading font-semibold px-4 py-2"
  >
    {t('settings.tabs.general')}
  </TabsTrigger>
  {/* Same pattern for 'projects' and 'about' triggers */}
</TabsList>
```

- [ ] **Step 2: Update section headings — text and icon container**

Find section heading `h2` elements and add `font-heading font-bold`.

Section heading icon containers (`rounded-lg bg-accent`) — `rounded-lg` is auto-removed by global `--radius: 0`. The `bg-accent` auto-inherits the new color (#E0E3E5). No manual change needed.

- [ ] **Step 3: Run lint + build + tests**

```bash
rtk pnpm test && rtk pnpm lint && rtk pnpm build
```
Expected: All pass

- [ ] **Step 4: Commit**

```bash
rtk git add src/pages/SettingsPage.tsx
rtk git commit -m "feat(ui): restyle settings page with underline tabs and heading fonts"
```

---

### Task 4.3: Dark Mode Verification + Polish

**Files:**
- Possibly modify: `src/index.css` (if contrast tweaks needed)

- [ ] **Step 1: Visual verification in dark mode**

```bash
rtk pnpm dev
```
Toggle to dark mode. Check each page:
- Skills page: sidebar, skill cards, compact list, detail panel
- Discover page: search, results
- Settings page: tabs, cards, checkboxes
- Verify text contrast on all backgrounds

- [ ] **Step 2: Fix any contrast issues**

If `--muted-foreground` (#8A9B90) on `--card` (#111C18) is too low contrast, lighten to #95A69C or similar.

Test with a contrast checker: minimum 4.5:1 for normal text.

- [ ] **Step 3: Run full verification**

```bash
rtk pnpm test && rtk pnpm lint && rtk pnpm build
```
Expected: All pass

- [ ] **Step 4: Commit if changes were made**

```bash
rtk git add src/index.css
rtk git commit -m "fix(ui): adjust dark mode contrast for WCAG AA compliance"
```

---

### Task 4.4: Final Verification

- [ ] **Step 1: Full test suite**

```bash
rtk pnpm test
```
Expected: All tests pass

- [ ] **Step 2: Lint**

```bash
rtk pnpm lint
```
Expected: No errors

- [ ] **Step 3: Production build**

```bash
rtk pnpm build
```
Expected: Build succeeds

- [ ] **Step 4: Wizard step titles — add font-heading**

`font-heading` is a new utility class and does NOT auto-inherit via CSS variables — it requires manually adding the class. Add `font-heading` to step title elements in:
- `src/pages/WizardPage.tsx` (main "Add Skill" title)
- Individual step components that have heading text

- [ ] **Step 5: Visual walkthrough**

Start dev server and verify all pages in both light and dark mode:
- [ ] Header: sharp logo, underline nav, simplified controls
- [ ] Sidebar: section headers, left accent active state, bottom Add button
- [ ] SkillCard: heading font, sharp corners, green-tinted agent badges
- [ ] CompactList: border-y selection, heading font section labels
- [ ] DetailPanel: hero title, metadata grid, agent pills
- [ ] Discover: heading font on results, primary Install button
- [ ] Settings: underline tabs, heading font on sections
- [ ] Dark mode: all above with correct colors and contrast
- [ ] Empty states: simplified, sharp corners, heading fonts
