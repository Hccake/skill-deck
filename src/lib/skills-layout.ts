export const SKILLS_LIST_MIN_WIDTH = 240;
export const SKILL_DETAIL_MIN_WIDTH = 360;
export const SKILLS_SIDEBAR_MIN_WIDTH = 192;
export const SKILLS_SIDEBAR_DEFAULT_WIDTH = 208;
export const SKILLS_SIDEBAR_MAX_WIDTH = 320;
export const SKILL_DETAIL_COMFORT_WIDTH = 480;

export function resolveSkillsLayout(width: number, hasDetail: boolean, preferredListWidth: number | null, wasCollapsed: boolean, preferredSidebarWidth = SKILLS_SIDEBAR_DEFAULT_WIDTH) {
  const desiredSidebarWidth = Math.max(SKILLS_SIDEBAR_MIN_WIDTH, Math.min(SKILLS_SIDEBAR_MAX_WIDTH, preferredSidebarWidth));
  const threeColumnMinWidth = desiredSidebarWidth + SKILLS_LIST_MIN_WIDTH + SKILL_DETAIL_COMFORT_WIDTH + 1;
  const navigationCollapsed = hasDetail && width < threeColumnMinWidth + (wasCollapsed ? 24 : 0);
  const sidebarWidth = navigationCollapsed ? 0 : desiredSidebarWidth;
  const sidebarMaxWidth = Math.max(SKILLS_SIDEBAR_MIN_WIDTH, Math.min(SKILLS_SIDEBAR_MAX_WIDTH,
    Math.floor(width - SKILLS_LIST_MIN_WIDTH - (hasDetail ? SKILL_DETAIL_COMFORT_WIDTH + 1 : 0))));
  const availableWidth = width - sidebarWidth - 1;
  const desiredListWidth = preferredListWidth ?? Math.min(320, availableWidth * 0.3);
  const listWidth = hasDetail
    ? Math.max(SKILLS_LIST_MIN_WIDTH, Math.min(desiredListWidth, availableWidth - SKILL_DETAIL_MIN_WIDTH))
    : width - sidebarWidth;
  return { sidebarWidth, sidebarMaxWidth, listWidth, navigationCollapsed };
}
