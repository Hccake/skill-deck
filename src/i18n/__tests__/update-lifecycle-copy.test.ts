import { describe, expect, it } from 'vitest';
import type {
  EvidenceFailureReason,
  EvidenceFreshness,
  SkillUpdateCheckStatus,
  UpdateCheckReasonCode,
} from '@/bindings';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

const updateStatuses: SkillUpdateCheckStatus[] = [
  'updateAvailable', 'upToDate', 'cannotCheck', 'deletedUpstream',
];
const updateReasons: UpdateCheckReasonCode[] = [
  'missingRemoteHash', 'missingSource', 'unsupportedSource',
  'upstreamUnavailable', 'deletedUpstream',
];
const evidenceFreshness: EvidenceFreshness[] = [
  'fresh', 'cached', 'stale', 'coolingDown', 'backingOff', 'unavailable',
];
const evidenceFailures: EvidenceFailureReason[] = [
  'rateLimited', 'authenticationRequired', 'refNotFound', 'repositoryNotFound',
  'notFoundOrUnauthorized', 'network', 'incompleteEvidence', 'sourceUnavailable',
];

describe('update lifecycle copy', () => {
  it('defines source and outcome labels in both supported locales', () => {
    for (const locale of [en, zhCN]) {
      expect(locale.skills.updatePlan.resultOutcome).toEqual({
        succeeded: expect.any(String),
        partial: expect.any(String),
        failed: expect.any(String),
        cancelled: expect.any(String),
      });
      expect(locale.mutation.result.status.acquired).toEqual(expect.any(String));
      expect(locale.mutation.phase.acquiring).toEqual(expect.any(String));
      expect(locale.mutation.phase.validating).toEqual(expect.any(String));
      expect(locale.mutation.phase.updating).toEqual(expect.any(String));
      expect(locale.skills.updatePlan.cleanCopyCount).toEqual(expect.any(String));
      expect(locale.skills.updatePlan.cleanCopyCountForSkill).toEqual(expect.any(String));
    }
  });

  it('defines every Backend update status, reason, and evidence value in both locales', () => {
    for (const locale of [en, zhCN]) {
      const skills = locale.skills as unknown as {
        updateStatus: Record<string, string>;
        updateReason: Record<string, string>;
        updateHint: Record<string, string>;
        updateEvidence: {
          freshness: Record<string, string>;
          failure: Record<string, string>;
        };
      };
      for (const status of updateStatuses) {
        expect(skills.updateStatus[status], status).toEqual(expect.any(String));
      }
      for (const reason of updateReasons) {
        expect(skills.updateReason[reason], reason).toEqual(expect.any(String));
        expect(skills.updateHint[reason], reason).toEqual(expect.any(String));
      }
      for (const freshness of evidenceFreshness) {
        expect(skills.updateEvidence.freshness[freshness], freshness).toEqual(expect.any(String));
      }
      for (const failure of evidenceFailures) {
        expect(skills.updateEvidence.failure[failure], failure).toEqual(expect.any(String));
      }
    }
  });
});
