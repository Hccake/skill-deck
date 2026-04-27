// src/stores/skills-utils.ts
import i18n from '@/i18n';
import type {
  InstalledSkill,
  SkillScope,
  SkillUpdateCheckStatus,
  SkillUpdateInfo,
} from '@/bindings';

export type SkillListItem = InstalledSkill & {
  updateStatus?: SkillUpdateCheckStatus | null;
  updateReason?: string | null;
};

/** 按名称排序 skills，保证展示顺序稳定 */
export function sortSkills(skills: SkillListItem[]): SkillListItem[] {
  return [...skills].sort((a, b) => a.name.localeCompare(b.name));
}

/** 将 check_updates 结果合并到 skills 列表 */
export function mergeUpdateInfo(skills: SkillListItem[], updates: SkillUpdateInfo[]): SkillListItem[] {
  const updateMap = new Map(updates.map((u) => [u.name, u]));
  return skills.map((s) => ({
    ...s,
    hasUpdate: updateMap.get(s.name)?.hasUpdate ?? false,
    updateStatus: updateMap.get(s.name)?.status ?? s.updateStatus ?? null,
    updateReason: updateMap.get(s.name)?.reason ?? null,
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
      r.name === skillName
        ? { ...r, hasUpdate: false, status: 'up-to-date', reason: null }
        : r
    );
  }
}

/** i18n t() 的便捷包装 */
export function t(key: string, options?: Record<string, unknown>): string {
  return i18n.t(key, options);
}

/**
 * 把后端返回的 update reason 映射到 i18n key。
 *
 * 后端会返回:capability 派生的静态 reason (如 `missing-skill-path`),
 * 或 check_updates 时拿到的 GitHub API reason (如 `rate-limited`/`http-404`)。
 * `http-<code>` 是动态值,这里折叠到通用的 `http-error` key,避免 i18n 字典爆炸。
 */
export function resolveUpdateReasonI18nKey(reason: string | null | undefined): string | null {
  if (!reason) return null;
  if (reason.startsWith('http-')) return 'skills.updateReason.http-error';
  return `skills.updateReason.${reason}`;
}

export interface DeleteTarget {
  skill: SkillListItem;
  scope: SkillScope;
  projectPath?: string;
}

export interface AddDialogPrefill {
  source: string;
  skillName: string;
}
