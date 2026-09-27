export const SKILLS_LIST_MIN_WIDTH = 240;
export const SKILL_DETAIL_MIN_WIDTH = 360;
const SIDEBAR_MIN_WIDTH = 192;
const SIDEBAR_WIDTH = 208;
const THREE_COLUMN_MIN_WIDTH = 794;

export function resolveSkillsLayout(width: number, hasDetail: boolean, preferredListWidth: number, wasCollapsed: boolean) {
  const navigationCollapsed = hasDetail && width < THREE_COLUMN_MIN_WIDTH + (wasCollapsed ? 24 : 0);
  const sidebarWidth = navigationCollapsed ? 0 : hasDetail
    ? Math.max(SIDEBAR_MIN_WIDTH, Math.min(SIDEBAR_WIDTH, width - SKILLS_LIST_MIN_WIDTH - SKILL_DETAIL_MIN_WIDTH - 1))
    : SIDEBAR_WIDTH;
  const listWidth = hasDetail
    ? Math.max(SKILLS_LIST_MIN_WIDTH, Math.min(preferredListWidth, width - sidebarWidth - SKILL_DETAIL_MIN_WIDTH - 1))
    : width - sidebarWidth;
  return { sidebarWidth, listWidth, navigationCollapsed };
}
