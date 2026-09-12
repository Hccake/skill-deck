// src/stores/skills-utils.ts
import i18n from '@/i18n';
import { contextKey } from '@/lib/context';
import type {
  SkillLocationRef,
  EvidenceFailureReason,
  EvidenceFreshness,
  InstalledSkill,
  InstalledSkillLocation,
  SkillUpdateCheckStatus,
  SkillUpdateInfo,
  SourceUpdateCheckInfo,
  UpdateCheckOutcome,
  UpdateCheckReasonCode,
} from '@/bindings';

export type SkillListItem = InstalledSkill & {
  updateStatus?: SkillUpdateCheckStatus | null;
  updateReason?: string | null;
  updateFreshness?: EvidenceFreshness | null;
  updateEvidence?: SourceUpdateCheckInfo | null;
  skillPath?: string | null;
  /** 最近一次检查尝试的结果；失败时不覆盖上次已确认的比较结论。 */
  updateAttempt?: {
    outcome: UpdateCheckOutcome;
    reason?: string | null;
    attemptedAt?: number;
  } | null;
};

export function hasCommittedUpdateComparison(
  skill: Pick<SkillListItem, 'updateStatus'>,
): boolean {
  return skill.updateStatus === 'upToDate'
    || skill.updateStatus === 'updateAvailable'
    || skill.updateStatus === 'deletedUpstream';
}

export interface UpdateCheckDisplaySnapshot {
  outcome: UpdateCheckOutcome;
  sources: SourceUpdateCheckInfo[];
  skillFreshness: Record<string, EvidenceFreshness>;
  checkedAt: number;
}

export function providerCooldownDeadline(
  sources: readonly SourceUpdateCheckInfo[],
): number | null {
  return sources.reduce<number | null>((latest, source) => {
    const failure = source.lastAttempt?.failure;
    if (!failure?.providerCooldown || !failure.retryAtEpochMs) return latest;
    return latest == null
      ? failure.retryAtEpochMs
      : Math.max(latest, failure.retryAtEpochMs);
  }, null);
}

export type SkillUpdateDisplayStatus =
  | 'acquiring'
  | 'validating'
  | 'updating'
  | 'done'
  | 'failed';

export type SkillUpdateActivePhase = Extract<
  SkillUpdateDisplayStatus,
  'acquiring' | 'validating' | 'updating'
>;

/** 按名称排序 skills，保证展示顺序稳定 */
export function sortSkills(skills: SkillListItem[]): SkillListItem[] {
  return [...skills].sort((a, b) => a.name.localeCompare(b.name));
}

interface MergeUpdateInfoOptions {
  preserveUnmatched?: boolean;
  previousSkills?: SkillListItem[];
  sources?: SourceUpdateCheckInfo[];
}

/** 将 check_updates 结果合并到 skills 列表 */
export function mergeUpdateInfo(
  skills: SkillListItem[],
  updates: SkillUpdateInfo[],
  options: MergeUpdateInfoOptions = {},
): SkillListItem[] {
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

  const previousSkillMap = new Map(
    (options.previousSkills ?? []).map((skill) => [updateIdentityKey(skill), skill]),
  );

  return skills.map((s) => {
    const update = findUpdateForSkill(s, exactUpdateMap, pathlessUpdateMap, nameOnlyUpdateMap);
    const previous = options.preserveUnmatched
      ? previousSkillMap.get(updateIdentityKey(s))
      : undefined;
    const updateEvidence = update
      ? findSourceUpdateInfo(update, options.sources ?? [])
      : undefined;
    const incompleteAttempt = update
      && updateEvidence?.lastAttempt?.failure != null
      && (
        (update.status === 'cannotCheck' && update.reason === 'upstreamUnavailable')
        || update.freshness === 'backingOff'
        || update.freshness === 'coolingDown'
        || update.freshness === 'unavailable'
      );
    const lastConfirmed = previous ?? s;
    const hasConfirmedComparison = hasCommittedUpdateComparison(lastConfirmed);
    return {
      ...s,
      skillPath: update?.skillPath ?? s.skillPath ?? null,
      hasUpdate: incompleteAttempt && hasConfirmedComparison
        ? lastConfirmed.hasUpdate
        : update?.hasUpdate ?? previous?.hasUpdate ?? (options.preserveUnmatched ? s.hasUpdate : false),
      updateStatus: incompleteAttempt && hasConfirmedComparison
        ? lastConfirmed.updateStatus
        : update?.status ?? previous?.updateStatus ?? s.updateStatus ?? null,
      updateReason: incompleteAttempt && hasConfirmedComparison
        ? lastConfirmed.updateReason ?? null
        : update?.reason ?? previous?.updateReason ?? s.updateReason ?? null,
      updateFreshness: update?.freshness ?? previous?.updateFreshness ?? s.updateFreshness ?? null,
      updateEvidence: updateEvidence ?? previous?.updateEvidence ?? s.updateEvidence ?? null,
      updateAttempt: incompleteAttempt
        ? { outcome: 'notCompleted', reason: 'upstreamUnavailable', attemptedAt: Date.now() }
        : update
          ? { outcome: 'completed', reason: null, attemptedAt: Date.now() }
          : previous?.updateAttempt ?? s.updateAttempt ?? null,
    };
  });
}

/** 更新检测结果的 scope 级缓存 — 避免频繁切换 scope 时重复网络请求 */
export const updateInfoCache = new Map<string, {
  results: SkillUpdateInfo[];
  sources: SourceUpdateCheckInfo[];
  checkedAt: number;
  completeness: 'complete' | 'partial';
  outcome: UpdateCheckOutcome;
}>();

/** 清除缓存中指定 skill 的 hasUpdate 标记 — 更新成功后调用，防止 syncSkills 恢复旧标记 */
export function clearUpdateCacheForSkill(
  skillName: string,
  scope: InstalledSkillLocation,
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
  context: SkillLocationRef,
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
  if (result.status === 'deletedUpstream' || result.reason === 'deletedUpstream') {
    return result;
  }
  return { ...result, hasUpdate: false, status: 'upToDate', reason: null };
}

/** i18n t() 的便捷包装 */
export function t(key: string, options?: Record<string, unknown>): string {
  return i18n.t(key, options);
}

const UPDATE_STATUS_I18N_KEYS = {
  updateAvailable: 'skills.updateStatus.updateAvailable',
  upToDate: 'skills.updateStatus.upToDate',
  cannotCheck: 'skills.updateStatus.cannotCheck',
  deletedUpstream: 'skills.updateStatus.deletedUpstream',
} satisfies Record<SkillUpdateCheckStatus, string>;

const UPDATE_REASON_I18N_KEYS = {
  missingRemoteHash: 'skills.updateReason.missingRemoteHash',
  missingSource: 'skills.updateReason.missingSource',
  unsupportedSource: 'skills.updateReason.unsupportedSource',
  upstreamUnavailable: 'skills.updateReason.upstreamUnavailable',
  deletedUpstream: 'skills.updateReason.deletedUpstream',
} satisfies Record<UpdateCheckReasonCode, string>;

const UPDATE_HINT_I18N_KEYS = {
  missingRemoteHash: 'skills.updateHint.missingRemoteHash',
  missingSource: 'skills.updateHint.missingSource',
  unsupportedSource: 'skills.updateHint.unsupportedSource',
  upstreamUnavailable: 'skills.updateHint.upstreamUnavailable',
  deletedUpstream: 'skills.updateHint.deletedUpstream',
} satisfies Record<UpdateCheckReasonCode, string>;

const LEGACY_UPDATE_REASON_I18N_KEYS: Record<string, string> = {
  'missing-skill-path': 'skills.updateReason.missing-skill-path',
  'missing-remote-hash': 'skills.updateReason.missing-remote-hash',
  'unsupported-source-type': 'skills.updateReason.unsupported-source-type',
  'local-source': 'skills.updateReason.local-source',
  'upstream-unavailable': 'skills.updateReason.upstream-unavailable',
  'deleted-upstream': 'skills.updateReason.deleted-upstream',
  'rate-limited': 'skills.updateReason.rate-limited',
  auth: 'skills.updateReason.auth',
  'network-error': 'skills.updateReason.network-error',
};

const LEGACY_UPDATE_HINT_I18N_KEYS: Record<string, string> = {
  'missing-skill-path': 'skills.updateHint.missing-skill-path',
  'missing-remote-hash': 'skills.updateHint.missing-remote-hash',
  'unsupported-source-type': 'skills.updateHint.unsupported-source-type',
  'local-source': 'skills.updateHint.local-source',
  'upstream-unavailable': 'skills.updateHint.upstream-unavailable',
  'deleted-upstream': 'skills.updateHint.deleted-upstream',
  'rate-limited': 'skills.updateHint.rate-limited',
  auth: 'skills.updateHint.auth',
  'network-error': 'skills.updateHint.network-error',
};

const EVIDENCE_FRESHNESS_I18N_KEYS = {
  fresh: 'skills.updateEvidence.freshness.fresh',
  cached: 'skills.updateEvidence.freshness.cached',
  stale: 'skills.updateEvidence.freshness.stale',
  coolingDown: 'skills.updateEvidence.freshness.coolingDown',
  backingOff: 'skills.updateEvidence.freshness.backingOff',
  unavailable: 'skills.updateEvidence.freshness.unavailable',
} satisfies Record<EvidenceFreshness, string>;

const EVIDENCE_FAILURE_I18N_KEYS = {
  rateLimited: 'skills.updateEvidence.failure.rateLimited',
  authenticationRequired: 'skills.updateEvidence.failure.authenticationRequired',
  refNotFound: 'skills.updateEvidence.failure.refNotFound',
  repositoryNotFound: 'skills.updateEvidence.failure.repositoryNotFound',
  notFoundOrUnauthorized: 'skills.updateEvidence.failure.notFoundOrUnauthorized',
  network: 'skills.updateEvidence.failure.network',
  incompleteEvidence: 'skills.updateEvidence.failure.incompleteEvidence',
  sourceUnavailable: 'skills.updateEvidence.failure.sourceUnavailable',
} satisfies Record<EvidenceFailureReason, string>;

const SKILL_UPDATE_PHASE_I18N_KEYS = {
  acquiring: 'skills.updatePhaseAcquiring',
  validating: 'skills.updatePhaseValidating',
  updating: 'skills.updatePhaseUpdating',
} satisfies Record<SkillUpdateActivePhase, string>;

function typedUpdateReasonKey(
  reason: string,
  keys: Record<UpdateCheckReasonCode, string>,
): string | undefined {
  return Object.prototype.hasOwnProperty.call(keys, reason)
    ? keys[reason as UpdateCheckReasonCode]
    : undefined;
}

export function resolveUpdateStatusI18nKey(status: SkillUpdateCheckStatus): string {
  return UPDATE_STATUS_I18N_KEYS[status];
}

export function resolveUpdateReasonI18nKey(reason: string | null | undefined): string | null {
  if (!reason) return null;
  const typedKey = typedUpdateReasonKey(reason, UPDATE_REASON_I18N_KEYS);
  if (typedKey) return typedKey;
  if (reason.startsWith('http-')) return 'skills.updateReason.http-error';
  return LEGACY_UPDATE_REASON_I18N_KEYS[reason] ?? null;
}

export function resolveEvidenceFreshnessI18nKey(freshness: EvidenceFreshness): string {
  return EVIDENCE_FRESHNESS_I18N_KEYS[freshness];
}

export function resolveEvidenceFailureReasonI18nKey(reason: EvidenceFailureReason): string {
  return EVIDENCE_FAILURE_I18N_KEYS[reason];
}

const EVIDENCE_FAILURE_NEXT_STEP_I18N_KEYS = {
  rateLimited: 'skills.updateEvidence.nextStep.configureTokenOrWait',
  authenticationRequired: 'skills.updateEvidence.nextStep.configureToken',
  refNotFound: 'skills.updateEvidence.nextStep.checkSource',
  repositoryNotFound: 'skills.updateEvidence.nextStep.checkSource',
  notFoundOrUnauthorized: 'skills.updateEvidence.nextStep.checkSourceOrToken',
  network: 'skills.updateEvidence.nextStep.retry',
  incompleteEvidence: 'skills.updateEvidence.nextStep.retry',
  sourceUnavailable: 'skills.updateEvidence.nextStep.retry',
} satisfies Record<EvidenceFailureReason, string>;

export function resolveEvidenceFailureNextStepI18nKey(reason: EvidenceFailureReason): string {
  return EVIDENCE_FAILURE_NEXT_STEP_I18N_KEYS[reason];
}

export function hasIncompleteUpdateCheck(
  skill: Pick<SkillListItem, 'updateStatus' | 'updateReason' | 'updateEvidence' | 'updateAttempt'>,
): boolean {
  return skill.updateAttempt?.outcome === 'notCompleted'
    || (skill.updateStatus === 'cannotCheck'
    && skill.updateReason === 'upstreamUnavailable'
    && skill.updateEvidence?.lastAttempt?.failure != null);
}

export function isSkillUpdateActive(
  status: SkillUpdateDisplayStatus | undefined,
): status is SkillUpdateActivePhase {
  return status === 'acquiring' || status === 'validating' || status === 'updating';
}

export function resolveSkillUpdatePhaseI18nKey(phase: SkillUpdateActivePhase): string {
  return SKILL_UPDATE_PHASE_I18N_KEYS[phase];
}

export function resolveUpdateStatusLabelI18nKey(
  skill: Pick<InstalledSkill, 'hasUpdate' | 'canRunUpdate' | 'canCheckForUpdates'> & {
    updateStatus?: SkillUpdateCheckStatus | null;
    updateReason?: string | null;
    updateEvidence?: SourceUpdateCheckInfo | null;
    updateAttempt?: SkillListItem['updateAttempt'];
  }
): string | null {
  if (hasIncompleteUpdateCheck(skill)) {
    return 'skills.updateStatusLabel.checkIncomplete';
  }
  if (skill.hasUpdate === true && skill.canRunUpdate !== false) {
    return 'skills.updateStatusLabel.available';
  }
  if (skill.updateStatus === 'deletedUpstream' || skill.updateReason === 'deletedUpstream') {
    return 'skills.updateStatusLabel.deletedUpstream';
  }
  if (skill.updateReason === 'missing-skill-path') {
    return 'skills.updateStatusLabel.needsSourceInfo';
  }
  if (skill.updateReason === 'missingRemoteHash' || skill.updateReason === 'missing-remote-hash') {
    return 'skills.updateStatusLabel.reinstallRequired';
  }
  if (skill.updateReason === 'local-source') {
    return 'skills.updateStatusLabel.localSource';
  }
  if (skill.updateReason === 'unsupported-source-type') {
    return 'skills.updateStatusLabel.autoCheckUnavailable';
  }
  if (skill.updateReason) {
    return 'skills.updateStatusLabel.checkFailed';
  }
  if (skill.updateStatus === 'cannotCheck' || skill.canCheckForUpdates === false) {
    return 'skills.updateStatusLabel.checkFailed';
  }
  return null;
}

export function resolveUpdateHintI18nKey(reason: string | null | undefined): string | null {
  if (!reason) return null;
  const typedKey = typedUpdateReasonKey(reason, UPDATE_HINT_I18N_KEYS);
  if (typedKey) return typedKey;
  if (reason.startsWith('http-')) return 'skills.updateHint.http-error';
  return LEGACY_UPDATE_HINT_I18N_KEYS[reason] ?? null;
}

export interface DeleteTarget {
  skill: SkillListItem;
  scope: InstalledSkillLocation;
  projectPath?: string;
  context: SkillLocationRef;
}

export interface AddDialogPrefill {
  source: string;
  skillName: string;
  scope?: InstalledSkillLocation;
  projectPath?: string;
  gitRef?: string | null;
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

export function normalizeSourceIdentity(source: string | null | undefined): string | null {
  const value = source?.trim();
  if (!value) return null;
  if (/^[^\s/:]+\/[^\s/]+$/.test(value)) {
    return `github.com/${value.replace(/\.git$/i, '')}`.toLocaleLowerCase('en-US');
  }
  if (!value.includes('://') && value.includes('@') && value.includes(':')) {
    const [, hostAndPath = value] = value.split('@');
    return hostAndPath.replace(':', '/').replace(/\.git$/i, '').toLocaleLowerCase('en-US');
  }
  try {
    const url = new URL(value);
    return `${url.host}${url.pathname}`
      .replace(/^\/+|\/+$/g, '')
      .replace(/\.git$/i, '')
      .toLocaleLowerCase('en-US');
  } catch {
    return value.replace(/^\/+|\/+$/g, '').replace(/\.git$/i, '').toLocaleLowerCase('en-US');
  }
}

function findSourceUpdateInfo(
  update: SkillUpdateInfo,
  sources: SourceUpdateCheckInfo[],
): SourceUpdateCheckInfo | null {
  const identity = normalizeSourceIdentity(update.sourceUrl ?? update.source);
  if (!identity) return null;
  return sources.find((source) => (
    normalizeSourceIdentity(source.source) === identity
    && (source.requestedRef ?? 'HEAD') === (update.gitRef ?? 'HEAD')
  )) ?? null;
}
