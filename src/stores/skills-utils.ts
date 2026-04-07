// src/stores/skills-utils.ts
import i18n from '@/i18n';
import type { InstalledSkill, SkillScope, SkillUpdateInfo } from '@/bindings';

/** 按名称排序 skills，保证展示顺序稳定 */
export function sortSkills(skills: InstalledSkill[]): InstalledSkill[] {
  return [...skills].sort((a, b) => a.name.localeCompare(b.name));
}

/** 将 check_updates 结果合并到 skills 列表 */
export function mergeUpdateInfo(skills: InstalledSkill[], updates: SkillUpdateInfo[]): InstalledSkill[] {
  const updateMap = new Map(updates.map((u) => [u.name, u.hasUpdate]));
  return skills.map((s) => ({
    ...s,
    hasUpdate: updateMap.get(s.name) ?? false,
  }));
}

/** 更新检测结果的 scope 级缓存 — 避免频繁切换 scope 时重复网络请求 */
export const updateInfoCache = new Map<string, { results: SkillUpdateInfo[]; checkedAt: number }>();
export const UPDATE_CHECK_TTL = 5 * 60 * 1000; // 5 分钟

/** 清除缓存中指定 skill 的 hasUpdate 标记 — 更新成功后调用，防止 syncSkills 恢复旧标记 */
export function clearUpdateCacheForSkill(skillName: string, scope: SkillScope, projectPath?: string) {
  const cacheKey = scope === 'project' ? projectPath : 'global';
  if (!cacheKey) return;
  const cached = updateInfoCache.get(cacheKey);
  if (cached) {
    cached.results = cached.results.map((r) =>
      r.name === skillName ? { ...r, hasUpdate: false } : r
    );
  }
}

/** i18n t() 的便捷包装 */
export function t(key: string, options?: Record<string, unknown>): string {
  return i18n.t(key, options);
}

export interface DeleteTarget {
  skill: InstalledSkill;
  scope: SkillScope;
  projectPath?: string;
}

export interface AddDialogPrefill {
  source: string;
  skillName: string;
}
