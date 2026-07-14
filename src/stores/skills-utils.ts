// src/stores/skills-utils.ts
import i18n from '@/i18n';
import { contextKey } from '@/lib/context';
import type {
  AgentType,
  ContextRef,
  InstalledSkill,
  SkillScope,
  SkillUpdateCheckStatus,
  SkillUpdateInfo,
} from '@/bindings';

export type SkillListItem = InstalledSkill & {
  updateStatus?: SkillUpdateCheckStatus | null;
  updateReason?: string | null;
  skillPath?: string | null;
};

/** 按名称排序 skills，保证展示顺序稳定 */
export function sortSkills(skills: SkillListItem[]): SkillListItem[] {
  return [...skills].sort((a, b) => a.name.localeCompare(b.name));
}

/** 将 check_updates 结果合并到 skills 列表 */
export function mergeUpdateInfo(skills: SkillListItem[], updates: SkillUpdateInfo[]): SkillListItem[] {
  const exactUpdateMap = new Map(updates.map((u) => [updateIdentityKey(u), u]));
  const pathlessUpdateMap = new Map<string, SkillUpdateInfo[]>();
  const nameOnlyUpdateMap = new Map<string, SkillUpdateInfo>();

  for (const update of updates) {
    const pathlessKey = updateIdentityKey(update, { includeSkillPath: false });
    pathlessUpdateMap.set(pathlessKey, [...(pathlessUpdateMap.get(pathlessKey) ?? []), update]);
    if (!hasStableUpdateIdentity(update)) {
      nameOnlyUpdateMap.set(update.name, update);
    }
  }

  return skills.map((s) => {
    const update = findUpdateForSkill(s, exactUpdateMap, pathlessUpdateMap, nameOnlyUpdateMap);
    return {
      ...s,
      skillPath: update?.skillPath ?? s.skillPath ?? null,
      hasUpdate: update?.hasUpdate ?? false,
      updateStatus: update?.status ?? s.updateStatus ?? null,
      updateReason: update?.reason ?? s.updateReason ?? null,
    };
  });
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
        ? clearCachedUpdateResult(r)
        : r
    );
  }
}

export function clearUpdateCacheForContextSkill(
  skillName: string,
  context: ContextRef,
  options: { clearCannotCheck?: boolean } = {},
) {
  const cached = updateInfoCache.get(contextKey(context));
  if (!cached) return;
  cached.results = cached.results.map((result) =>
    result.name === skillName && (result.hasUpdate || options.clearCannotCheck)
      ? clearCachedUpdateResult(result)
      : result
  );
}

function clearCachedUpdateResult(result: SkillUpdateInfo): SkillUpdateInfo {
  if (result.status === 'deleted-upstream' || result.reason === 'deleted-upstream') {
    return result;
  }
  return { ...result, hasUpdate: false, status: 'up-to-date', reason: null };
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
  if (skill.updateStatus === 'deleted-upstream' || skill.updateReason === 'deleted-upstream') {
    return 'skills.updateStatusLabel.deleted-upstream';
  }
  if (skill.updateReason === 'missing-skill-path') {
    return 'skills.updateStatusLabel.needsSourceInfo';
  }
  if (skill.updateReason === 'missing-remote-hash') {
    return 'skills.updateStatusLabel.reinstallRequired';
  }
  if (skill.updateReason === 'unsupported-source-type' || skill.updateReason === 'local-source') {
    return 'skills.updateStatusLabel.autoCheckUnavailable';
  }
  if (skill.updateReason) {
    return 'skills.updateStatusLabel.checkFailed';
  }
  if (skill.updateStatus === 'cannot-check' || skill.canCheckForUpdates === false) {
    return 'skills.updateStatusLabel.checkFailed';
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
  context: ContextRef;
}

export interface AddDialogPrefill {
  source: string;
  skillName: string;
  scope?: SkillScope;
  projectPath?: string;
  gitRef?: string | null;
}

export interface RepairSourceDraft {
  source: string;
  skillName: string;
  scope: SkillScope;
  projectPath?: string;
  gitRef?: string | null;
  agents: AgentType[];
  defaultAvailableAgents?: AgentType[];
  privateAdaptedAgents?: AgentType[];
  privateCopyAgents?: AgentType[];
  context: ContextRef;
}

export function getSkillOperationAgents(
  skill: Pick<InstalledSkill, 'agents' | 'privateAdaptedAgents' | 'privateCopyAgents'>
): AgentType[] {
  const agents = [
    ...(skill.privateAdaptedAgents ?? skill.agents),
    ...(skill.privateCopyAgents ?? []),
  ];
  return agents.filter((agent, index) => agents.indexOf(agent) === index);
}

function normalizeRepairSource(source: string | null | undefined): string | null {
  if (!source) return null;
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(source)) return source;
  if (/^[^\s/]+\/[^\s/]+$/.test(source)) return `https://github.com/${source}`;
  return null;
}

function buildRepairSource(
  skill: Pick<InstalledSkill, 'source' | 'sourceUrl'> & { gitRef?: string | null }
): string | null {
  const baseSource = skill.sourceUrl || normalizeRepairSource(skill.source);
  if (!baseSource) return null;
  if (skill.gitRef && !baseSource.includes('#')) return `${baseSource}#${skill.gitRef}`;
  return baseSource;
}

function canRepairMissingSkillPath(
  skill: Pick<InstalledSkill, 'source' | 'sourceUrl'> & { updateReason?: string | null; gitRef?: string | null }
): boolean {
  return skill.updateReason === 'missing-skill-path' && buildRepairSource(skill) !== null;
}

type SkillMaintenanceAction = 'direct-reinstall' | 'repair-source' | 'none';

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

export function createSkillRepairDraft(
  skill: Pick<
    InstalledSkill,
    | 'name'
    | 'source'
    | 'sourceUrl'
    | 'agents'
    | 'defaultAvailableAgents'
    | 'privateAdaptedAgents'
    | 'privateCopyAgents'
  > & { gitRef?: string | null },
  context: ContextRef,
  projectPath?: string,
): RepairSourceDraft | null {
  const source = buildRepairSource(skill);
  if (!source) return null;
  const scope = context.scope.scope;
  const privateAdaptedAgents = skill.privateAdaptedAgents ?? skill.agents;
  const privateCopyAgents = skill.privateCopyAgents ?? [];
  return {
    source,
    skillName: skill.name,
    scope,
    projectPath: scope === 'project' ? projectPath : undefined,
    gitRef: skill.gitRef ?? null,
    agents: skill.agents,
    defaultAvailableAgents: skill.defaultAvailableAgents ?? [],
    privateAdaptedAgents,
    privateCopyAgents,
    context,
  };
}

interface UpdatePlanItem {
  name: string;
  source?: string | null;
  sourceUrl?: string | null;
  gitRef?: string | null;
  skillPath?: string | null;
  reason?: string | null;
  repairSource?: string | null;
}

interface UpdatePlanGroup {
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
  deletedUpstreamCount?: number;
  groups: UpdatePlanGroup[];
  repairable: UpdatePlanItem[];
  skipped: UpdatePlanItem[];
  deletedUpstream?: UpdatePlanItem[];
}

export function buildUpdatePlan(
  skills: SkillListItem[],
  scope: SkillScope,
  projectPath?: string
): UpdatePlan {
  const groups = new Map<string, UpdatePlanGroup>();
  const repairable: UpdatePlanItem[] = [];
  const skipped: UpdatePlanItem[] = [];
  const deletedUpstream: UpdatePlanItem[] = [];

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
      const operationAgents = getSkillOperationAgents(skill);
      group.skillRows.push({ name: skill.name, agents: operationAgents });
      group.agents = Array.from(new Set([...group.agents, ...operationAgents]));
      groups.set(groupKey, group);
      continue;
    }

    if (skill.updateStatus === 'deleted-upstream' || skill.updateReason === 'deleted-upstream') {
      deletedUpstream.push({
        name: skill.name,
        source: skill.source,
        sourceUrl: skill.sourceUrl,
        gitRef: skill.gitRef,
        skillPath: skill.skillPath,
        reason: skill.updateReason,
        repairSource: buildRepairSource(skill),
      });
      continue;
    }

    if (canRepairMissingSkillPath(skill)) {
      repairable.push({
        name: skill.name,
        source: skill.source,
        sourceUrl: skill.sourceUrl,
        gitRef: skill.gitRef,
        skillPath: skill.skillPath,
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
        skillPath: skill.skillPath,
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
    deletedUpstreamCount: deletedUpstream.length,
    groups: updateGroups,
    repairable,
    skipped,
    deletedUpstream,
  };
}

function updateIdentityKey(
  item: {
    name: string;
    source?: string | null;
    sourceUrl?: string | null;
    gitRef?: string | null;
    skillPath?: string | null;
  },
  options: { includeSkillPath?: boolean } = {}
): string {
  const includeSkillPath = options.includeSkillPath ?? true;
  return [
    item.name,
    item.sourceUrl ?? item.source ?? '',
    item.gitRef ?? '',
    includeSkillPath ? item.skillPath ?? '' : '',
  ].join('::');
}

function hasStableUpdateIdentity(item: {
  source?: string | null;
  sourceUrl?: string | null;
  gitRef?: string | null;
  skillPath?: string | null;
}): boolean {
  return Boolean(item.sourceUrl || item.source || item.gitRef || item.skillPath);
}

function findUpdateForSkill(
  skill: SkillListItem,
  exactUpdateMap: Map<string, SkillUpdateInfo>,
  pathlessUpdateMap: Map<string, SkillUpdateInfo[]>,
  nameOnlyUpdateMap: Map<string, SkillUpdateInfo>
): SkillUpdateInfo | undefined {
  const exact = exactUpdateMap.get(updateIdentityKey(skill));
  if (exact) return exact;

  if (!skill.skillPath) {
    const pathlessMatches = pathlessUpdateMap.get(updateIdentityKey(skill, { includeSkillPath: false })) ?? [];
    if (pathlessMatches.length === 1) return pathlessMatches[0];
  }

  if (!hasStableUpdateIdentity(skill)) {
    return nameOnlyUpdateMap.get(skill.name);
  }

  return undefined;
}
