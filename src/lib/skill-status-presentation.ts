import type { AppError, CheckUpdateCapability, SkillUpdateInfo, UpdateCheckResponse } from '@/bindings';
import type { SkillListItem } from '@/stores/skills-utils';
import { resolveEvidenceFailureNextStepI18nKey, resolveEvidenceFailureReasonI18nKey } from '@/stores/skills-utils';

type SkillStatusInput = Partial<Pick<SkillListItem,
  'source' | 'sourceUrl' | 'hasUpdate' | 'canRunUpdate' | 'canCheckForUpdates' | 'updateReason' | 'updateStatus' | 'updateError' | 'updateEvidence'
>> & { sourceType?: string; updateCapability?: CheckUpdateCapability | null };

export interface SkillSourceNotice {
  labelKey: string;
  hintKey?: string;
  error?: AppError;
}

function reasonFeedbackKey(reason: string): string {
  if (reason === 'network' || reason === 'timeout') return 'skills.checkUpdatesNetworkFailed';
  if (reason === 'authenticationRequired') return 'skills.checkUpdatesAccessFailed';
  return 'skills.checkUpdatesFailed';
}

function errorFeedbackKey(error: AppError): string {
  if (error.kind === 'gitNetworkError' || error.kind === 'gitTimeout') return 'skills.checkUpdatesNetworkFailed';
  if (error.kind === 'gitAuthFailed') return 'skills.checkUpdatesAccessFailed';
  if (error.kind === 'wellKnownSourceFailed' || error.kind === 'directDownloadFailed') return reasonFeedbackKey(error.data.reason);
  if (error.kind === 'sourceAcquisitionFailed') {
    const first = reasonFeedbackKey(error.data.wellKnownReason);
    return first === reasonFeedbackKey(error.data.downloadReason) ? first : 'skills.checkUpdatesFailed';
  }
  return 'skills.checkUpdatesFailed';
}

export function updateCheckFailureKey(result: AppError | Pick<UpdateCheckResponse, 'skills' | 'sources'>): string {
  if ('kind' in result) return errorFeedbackKey(result);
  if (result.skills.some(skill => skill.status === 'cannotCheck' && !skill.error
    && !result.sources.some(source => skill.sourceKey && source.sourceKey === skill.sourceKey
      && (source.error || source.lastAttempt?.failure)))) return 'skills.checkUpdatesFailed';
  const keys = new Set([
    ...result.skills.flatMap(skill => skill.error ? [errorFeedbackKey(skill.error)]
      : skill.status === 'cannotCheck' && skill.reason !== 'upstreamUnavailable' ? ['skills.checkUpdatesFailed'] : []),
    ...result.sources.flatMap(source => source.lastAttempt?.failure ? [reasonFeedbackKey(source.lastAttempt.failure.reason)]
      : source.error ? [errorFeedbackKey(source.error)] : []),
  ]);
  return keys.size === 1 ? [...keys][0] : 'skills.checkUpdatesFailed';
}

function isTransientCheckError(error: AppError): boolean {
  return errorFeedbackKey(error) === 'skills.checkUpdatesNetworkFailed';
}

export function skillStatusPresentation(skill: SkillStatusInput, check?: SkillUpdateInfo) {
  const reason = check?.reason ?? skill.updateReason ?? skill.updateCapability?.reason;
  const status = check?.status ?? skill.updateStatus;
  const sourceLabel = skill.source?.trim() || skill.sourceUrl?.trim() || null;
  const incomplete = reason === 'missingSource' || reason === 'missing-skill-path'
    || (!sourceLabel && skill.sourceType !== undefined && skill.sourceType !== 'local');
  const local = !incomplete && (reason === 'local-source' || skill.sourceType === 'local'
    || (!sourceLabel && !reason && !skill.updateError && !check?.error));
  const deleted = status === 'deletedUpstream' || reason === 'deletedUpstream' || reason === 'deleted-upstream';
  const available = !deleted && (check?.hasUpdate ?? skill.hasUpdate ?? false);
  const error = check?.error ?? skill.updateError ?? skill.updateEvidence?.error;
  const failure = skill.updateEvidence?.lastAttempt?.failure;
  let notice: SkillSourceNotice | null = null;

  if (incomplete) {
    notice = { labelKey: 'skills.card.sourceIncomplete', hintKey: 'skills.updateHint.missingSource' };
  } else if (deleted) {
    notice = { labelKey: 'skills.card.sourceMissingUpstream', hintKey: 'skills.updateHint.deletedUpstream' };
  } else if (!local) {
    if (failure && ['authenticationRequired', 'refNotFound', 'repositoryNotFound', 'notFoundOrUnauthorized'].includes(failure.reason)) {
      notice = { labelKey: resolveEvidenceFailureReasonI18nKey(failure.reason), hintKey: resolveEvidenceFailureNextStepI18nKey(failure.reason) };
    } else if (error && !isTransientCheckError(error)) {
      const labelKey = error.kind === 'gitAuthFailed' ? 'skills.updateEvidence.failure.authenticationRequired'
        : error.kind === 'gitRefNotFound' ? 'skills.updateEvidence.failure.refNotFound'
        : error.kind === 'gitRepoNotFound' ? 'skills.updateEvidence.failure.repositoryNotFound'
        : 'skills.card.sourceNeedsAttention';
      notice = { labelKey, error };
    } else if (reason === 'auth') {
      notice = { labelKey: 'skills.updateEvidence.failure.authenticationRequired', hintKey: 'skills.updateHint.auth' };
    }
  }

  const canRunUpdate = check?.capability.canRunUpdate ?? skill.canRunUpdate ?? skill.updateCapability?.canRunUpdate;
  const sourceHintKey = local ? null
    : reason === 'missingRemoteHash' || reason === 'missing-remote-hash'
      ? canRunUpdate ? 'skills.updateHint.missingRemoteHashCanUpdate' : 'skills.updateHint.missingRemoteHash'
    : reason === 'unsupportedSource' || reason === 'unsupported-source-type' ? 'skills.updateHint.unsupportedSource'
    : null;
  return { local, sourceLabel, available, notice, sourceHintKey };
}
