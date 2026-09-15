import { describe, expect, it } from 'vitest';
import { skillStatusPresentation, updateCheckFailureKey } from '../skill-status-presentation';

describe('Skill status presentation', () => {
  it('uses failure-specific feedback without suggesting retries for authentication or unknown errors', () => {
    expect(updateCheckFailureKey({ kind: 'gitNetworkError', data: { message: 'offline' } })).toBe('skills.checkUpdatesNetworkFailed');
    expect(updateCheckFailureKey({ kind: 'gitAuthFailed', data: { message: 'denied' } })).toBe('skills.checkUpdatesAccessFailed');
    expect(updateCheckFailureKey({ kind: 'custom', data: { message: 'unknown' } })).toBe('skills.checkUpdatesFailed');
    expect(updateCheckFailureKey({ skills: [], sources: [] })).toBe('skills.checkUpdatesFailed');
  });

  it('uses a neutral fallback for mixed failures in one batch', () => {
    const source = { source: 'owner/repo', requestedRef: null, resolvedRef: null, refRevision: null, checkedAtEpochMs: null,
      expiresAtEpochMs: null, freshness: 'unavailable' as const, lastAttempt: null };
    expect(updateCheckFailureKey({ skills: [], sources: [
      { ...source, error: { kind: 'gitNetworkError', data: { message: 'offline' } } },
      { ...source, error: { kind: 'gitAuthFailed', data: { message: 'denied' } } },
    ] })).toBe('skills.checkUpdatesFailed');
    expect(updateCheckFailureKey({
      skills: [{ name: 'unknown', source: 'other/repo', sourceKey: 'other', hasUpdate: false, status: 'cannotCheck',
        capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null }, reason: 'upstreamUnavailable',
        gitRef: null, sourceUrl: null, skillPath: null, freshness: 'unavailable' }],
      sources: [{ ...source, sourceKey: 'known', error: { kind: 'gitNetworkError', data: { message: 'offline' } } }],
    })).toBe('skills.checkUpdatesFailed');
  });
  it('identifies local content from its source, not update capability alone', () => {
    expect(skillStatusPresentation({ source: '/work/toolkit', updateReason: 'local-source' }).local).toBe(true);
    expect(skillStatusPresentation({ source: '/work/toolkit', sourceType: 'local' }).local).toBe(true);
    expect(skillStatusPresentation({ source: null, sourceUrl: null }).local).toBe(true);
    expect(skillStatusPresentation({ source: 'owner/repo', canCheckForUpdates: false }).local).toBe(false);
    expect(skillStatusPresentation({ source: null, updateReason: 'missingSource' }).local).toBe(false);
    const invalidMember = skillStatusPresentation({ source: '', sourceType: '' });
    expect(invalidMember.local).toBe(false);
    expect(invalidMember.notice?.labelKey).toBe('skills.card.sourceIncomplete');
  });

  it('keeps a known update visible without displaying a transient check error', () => {
    const presentation = skillStatusPresentation({ source: 'owner/repo', hasUpdate: true, updateStatus: 'updateAvailable',
      updateError: { kind: 'gitNetworkError', data: { message: 'connection failed' } },
    });
    expect(presentation.available).toBe(true);
    expect(presentation.notice).toBeNull();
    expect(skillStatusPresentation({ source: 'owner/repo' }).available).toBe(false);
  });

  it('keeps source problems distinct from ordinary local content and network failures', () => {
    expect(skillStatusPresentation({ updateReason: 'missingSource' }).notice?.labelKey).toBe('skills.card.sourceIncomplete');
    expect(skillStatusPresentation({ source: 'owner/repo', hasUpdate: true, updateStatus: 'deletedUpstream' }).available).toBe(false);
    expect(skillStatusPresentation({ source: 'owner/repo', updateStatus: 'deletedUpstream' }).notice?.labelKey).toBe('skills.card.sourceMissingUpstream');
    expect(skillStatusPresentation({ source: 'owner/repo', updateError: { kind: 'gitAuthFailed', data: { message: 'denied' } } }).notice?.labelKey).toBe('skills.updateEvidence.failure.authenticationRequired');
    expect(skillStatusPresentation({ source: 'owner/repo', updateError: { kind: 'gitRefNotFound', data: { refName: 'main' } } }).notice?.labelKey).toBe('skills.updateEvidence.failure.refNotFound');
  });

  it('explains missing comparison information on demand without another status label', () => {
    const presentation = skillStatusPresentation({ source: 'owner/repo', updateReason: 'missingRemoteHash', canRunUpdate: true });
    expect(presentation.notice).toBeNull();
    expect(presentation.sourceHintKey).toBe('skills.updateHint.missingRemoteHashCanUpdate');
    expect(presentation.available).toBe(false);
  });
});
