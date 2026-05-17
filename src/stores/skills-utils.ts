// src/stores/skills-utils.ts
import i18n from '@/i18n';
import type {
  AgentType,
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
    updateReason: updateMap.get(s.name)?.reason ?? s.updateReason ?? null,
  }));
}

/** 更新检测结果的 scope 级缓存 — 避免频繁切换 scope 时重复网络请求 */
export const updateInfoCache = new Map<string, { results: SkillUpdateInfo[]; checkedAt: number }>();
export const UPDATE_CHECK_TTL = 5 * 60 * 1000; // 5 分钟

/** 清除缓存中指定 skill 的 hasUpdate 标记 — 更新成功后调用，防止 syncSkills 恢复旧标记 */
export function clearUpdateCacheForSkill(
  skillName: string,
  scope: SkillScope,
  projectPath?: string,
  options: { clearCannotCheck?: boolean } = {},
) {
  const cacheKey = scope === 'project' ? projectPath : 'global';
  if (!cacheKey) return;
  const cached = updateInfoCache.get(cacheKey);
  if (cached) {
    cached.results = cached.results.map((r) =>
      r.name === skillName && (r.hasUpdate || options.clearCannotCheck)
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

export function resolveUpdateStatusLabelI18nKey(
  skill: Pick<InstalledSkill, 'hasUpdate' | 'canRunUpdate' | 'canCheckForUpdates'> & {
    updateStatus?: SkillUpdateCheckStatus | null;
    updateReason?: string | null;
  }
): string | null {
  if (skill.hasUpdate === true && skill.canRunUpdate !== false) {
    return 'skills.updateStatusLabel.available';
  }
  if (skill.updateReason === 'missing-skill-path') {
    return 'skills.updateStatusLabel.needsSourceInfo';
  }
  if (skill.updateReason === 'missing-remote-hash') {
    return 'skills.updateStatusLabel.versionUnknown';
  }
  if (skill.updateReason === 'local-source') {
    return 'skills.updateStatusLabel.localSource';
  }
  if (skill.updateReason === 'upstream-unavailable' || skill.updateReason === 'network-error') {
    return 'skills.updateStatusLabel.upstreamUnavailable';
  }
  if (skill.updateStatus === 'cannot-check' || skill.canCheckForUpdates === false) {
    return 'skills.updateStatusLabel.versionUnknown';
  }
  return null;
}

export function resolveUpdateHintI18nKey(reason: string | null | undefined): string | null {
  if (!reason) return null;
  if (reason.startsWith('http-')) return 'skills.updateHint.http-error';
  return `skills.updateHint.${reason}`;
}

export interface DeleteTarget {
  skill: SkillListItem;
  scope: SkillScope;
  projectPath?: string;
}

export interface AddDialogPrefill {
  source: string;
  skillName: string;
  scope?: SkillScope;
  projectPath?: string;
  gitRef?: string | null;
}

function normalizeRepairSource(source: string | null | undefined): string | null {
  if (!source) return null;
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(source)) return source;
  if (/^[^\s/]+\/[^\s/]+$/.test(source)) return `https://github.com/${source}`;
  return null;
}

export function buildRepairSource(
  skill: Pick<InstalledSkill, 'source' | 'sourceUrl'> & { gitRef?: string | null }
): string | null {
  const baseSource = skill.sourceUrl || normalizeRepairSource(skill.source);
  if (!baseSource) return null;
  if (skill.gitRef && !baseSource.includes('#')) return `${baseSource}#${skill.gitRef}`;
  return baseSource;
}

export function canRepairMissingSkillPath(
  skill: Pick<InstalledSkill, 'source' | 'sourceUrl'> & { updateReason?: string | null; gitRef?: string | null }
): boolean {
  return skill.updateReason === 'missing-skill-path' && buildRepairSource(skill) !== null;
}

export type SkillMaintenanceAction = 'direct-reinstall' | 'repair-source' | 'none';

export function resolveSkillMaintenanceAction(
  skill: Pick<InstalledSkill, 'source' | 'sourceUrl' | 'canRunUpdate'> & {
    updateReason?: string | null;
    gitRef?: string | null;
  }
): SkillMaintenanceAction {
  if (skill.updateReason === 'missing-remote-hash' && skill.canRunUpdate !== false) {
    return 'direct-reinstall';
  }
  if (canRepairMissingSkillPath(skill)) {
    return 'repair-source';
  }
  return 'none';
}

export function createSkillRepairPrefill(
  skill: Pick<InstalledSkill, 'name' | 'source' | 'sourceUrl'> & { gitRef?: string | null },
  scope: SkillScope,
  projectPath?: string
): AddDialogPrefill | null {
  const source = buildRepairSource(skill);
  if (!source) return null;
  return {
    source,
    skillName: skill.name,
    scope,
    projectPath: scope === 'project' ? projectPath : undefined,
    gitRef: skill.gitRef ?? null,
  };
}

export interface UpdatePlanItem {
  name: string;
  source?: string | null;
  sourceUrl?: string | null;
  gitRef?: string | null;
  reason?: string | null;
  repairSource?: string | null;
}

export interface UpdatePlanGroup {
  id: string;
  source: string;
  sourceUrl?: string | null;
  gitRef?: string | null;
  skillNames: string[];
  agents: AgentType[];
  skillRows: Array<{
    name: string;
    agents: AgentType[];
  }>;
}

export interface UpdatePlan {
  scope: SkillScope;
  projectPath?: string;
  total: number;
  updatableCount: number;
  repairableCount: number;
  skippedCount: number;
  groups: UpdatePlanGroup[];
  repairable: UpdatePlanItem[];
  skipped: UpdatePlanItem[];
}

export function buildUpdatePlan(
  skills: SkillListItem[],
  scope: SkillScope,
  projectPath?: string
): UpdatePlan {
  const groups = new Map<string, UpdatePlanGroup>();
  const repairable: UpdatePlanItem[] = [];
  const skipped: UpdatePlanItem[] = [];

  for (const skill of skills) {
    const source = skill.sourceUrl ?? skill.source ?? 'manual';
    const groupKey = `${source}::${skill.gitRef ?? ''}`;
    const isUpdatable = skill.hasUpdate === true && skill.canRunUpdate !== false;

    if (isUpdatable) {
      const group = groups.get(groupKey) ?? {
        id: groupKey,
        source: skill.source ?? source,
        sourceUrl: skill.sourceUrl,
        gitRef: skill.gitRef,
        skillNames: [],
        agents: [],
        skillRows: [],
      };
      group.skillNames.push(skill.name);
      group.skillRows.push({ name: skill.name, agents: skill.agents });
      group.agents = Array.from(new Set([...group.agents, ...skill.agents]));
      groups.set(groupKey, group);
      continue;
    }

    if (canRepairMissingSkillPath(skill)) {
      repairable.push({
        name: skill.name,
        source: skill.source,
        sourceUrl: skill.sourceUrl,
        gitRef: skill.gitRef,
        reason: skill.updateReason,
        repairSource: buildRepairSource(skill),
      });
      continue;
    }

    if (skill.updateStatus === 'cannot-check' || skill.canCheckForUpdates === false) {
      skipped.push({
        name: skill.name,
        source: skill.source,
        sourceUrl: skill.sourceUrl,
        gitRef: skill.gitRef,
        reason: skill.updateReason,
      });
    }
  }

  const updateGroups = Array.from(groups.values());
  return {
    scope,
    projectPath: scope === 'project' ? projectPath : undefined,
    total: skills.length,
    updatableCount: updateGroups.reduce((total, group) => total + group.skillNames.length, 0),
    repairableCount: repairable.length,
    skippedCount: skipped.length,
    groups: updateGroups,
    repairable,
    skipped,
  };
}
