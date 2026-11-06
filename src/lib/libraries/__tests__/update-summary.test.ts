import { describe, expect, it } from 'vitest';
import type { SkillUpdateCheckStatus, SkillUpdateInfo } from '@/bindings';
import {
  formatLibraryUpdateSummary,
  formatLibraryUpdateSummaryItems,
  summarizeLibraryUpdates,
} from '../update-summary';

const t = (key: string, options?: Record<string, unknown>) => (
  options ? `${key}(${options.count})` : key
);

function checks(...statuses: SkillUpdateCheckStatus[]): Record<string, SkillUpdateInfo> {
  return Object.fromEntries(statuses.map((status, index) => [
    `skill-${index}`,
    {
      name: `skill-${index}`,
      source: 'https://example.com/repo.git',
      hasUpdate: status === 'updateAvailable',
      status,
      capability: { canRunUpdate: true, canCheckForUpdates: true, canCheckAutomatically: true, reason: null },
      reason: null,
      gitRef: null,
      sourceUrl: null,
      skillPath: null,
      freshness: 'fresh',
      sourceKey: 'source-1',
    } satisfies SkillUpdateInfo,
  ]));
}

const sources = [{ sourceKey: 'source-1', expiresAtEpochMs: Date.now() + 60_000 }];
const membersFor = (result: Record<string, SkillUpdateInfo>) => Object.values(result).map(check => ({
  name: check.name, sourceType: 'git', comparisonFingerprint: check.comparisonFingerprint, updateCapability: check.capability,
}));
const summary = (result: Record<string, SkillUpdateInfo>, hasError: boolean) => summarizeLibraryUpdates(result, hasError, membersFor(result), sources);

const line = (statuses: SkillUpdateCheckStatus[], hasError = false) => (
  formatLibraryUpdateSummary(summary(checks(...statuses), hasError), t)
);

describe('library update summary', () => {
  it('does not call the Library up to date when a new maintainable member has no result', () => {
    const result = checks('upToDate');
    const members = ['skill-0', 'new-download'].map(name => ({ name, sourceType: 'download', comparisonFingerprint: null, updateCapability: { canRunUpdate: true, canCheckForUpdates: true, canCheckAutomatically: false, reason: null } }));
    expect(formatLibraryUpdateSummary(summarizeLibraryUpdates(result, false, members, sources), t)).toBe('');
  });

  it('ignores normally excluded members and rejects stale or different-baseline comparisons', () => {
    const result = checks('upToDate');
    const members = [{ name: 'skill-0', sourceType: 'download', comparisonFingerprint: 'current', updateCapability: { canRunUpdate: true, canCheckForUpdates: true, canCheckAutomatically: false, reason: null } },
      { name: 'one-time', sourceType: 'download', comparisonFingerprint: null, updateCapability: { canRunUpdate: false, canCheckForUpdates: false, canCheckAutomatically: false, reason: 'unsupportedSource' as const } }];
    expect(formatLibraryUpdateSummary(summarizeLibraryUpdates(result, false, members, sources), t)).toBe('');
    result['skill-0'].comparisonFingerprint = 'current';
    expect(formatLibraryUpdateSummary(summarizeLibraryUpdates(result, false, members, sources), t)).toBe('libraries.updateSummary.allUpToDate');
    result['skill-0'].freshness = 'stale';
    expect(formatLibraryUpdateSummary(summarizeLibraryUpdates(result, false, members, sources), t)).toBe('');
    result['skill-0'].freshness = 'fresh';
    expect(formatLibraryUpdateSummary(summarizeLibraryUpdates(result, true, members, sources), t)).toBe('libraries.updateSummary.incomplete');
  });

  it('requires unexpired evidence associated with each comparison', () => {
    const result = checks('upToDate');
    result['skill-0'].sourceKey = 'download-a';
    const sources = [{ sourceKey: 'download-a', expiresAtEpochMs: Date.now() + 60_000 }];
    expect(summarizeLibraryUpdates(result, false, membersFor(result), sources).coverageComplete).toBe(true);
    expect(summarizeLibraryUpdates(result, false, membersFor(result), [{ ...sources[0], expiresAtEpochMs: Date.now() - 1 }]).coverageComplete).toBe(false);
    delete result['skill-0'].sourceKey;
    expect(summarizeLibraryUpdates(result, false, membersFor(result), [{ ...sources[0], sourceKey: undefined }]).coverageComplete).toBe(false);
  });

  it('says nothing has been checked yet before the first result', () => {
    // 尚未检查不占位，摘要为空。
    expect(line([])).toBe('');
  });

  it('shows up-to-date only when it is the sole category', () => {
    expect(line(['upToDate', 'upToDate'])).toBe('libraries.updateSummary.allUpToDate');
    // 有需要处理的类别时，"已是最新"是默认背景，不占位。
    expect(line(['upToDate', 'updateAvailable']))
      .toBe('libraries.updateSummary.updateAvailable(1)');
  });

  it('orders categories by how much they need attention', () => {
    expect(line(['cannotCheck', 'deletedUpstream', 'updateAvailable'])).toBe(
      'libraries.updateSummary.updateAvailable(1) · libraries.updateSummary.deletedUpstream(1)'
      + ' · libraries.updateSummary.moreAttention(1)',
    );
  });

  it('keeps the previous conclusion and says this check did not finish', () => {
    expect(line(['updateAvailable'], true)).toBe(
      'libraries.updateSummary.updateAvailable(1) · libraries.updateSummary.incomplete',
    );
    expect(line([], true)).toBe('libraries.updateSummary.incomplete');
  });

  it('preserves the visual meaning of normal, actionable, and warning states', () => {
    expect(formatLibraryUpdateSummaryItems(
      summary(checks('upToDate'), false),
      t,
    )).toEqual([
      { text: 'libraries.updateSummary.allUpToDate', tone: 'neutral' },
    ]);

    expect(formatLibraryUpdateSummaryItems(
      summary(checks('updateAvailable', 'deletedUpstream'), true),
      t,
    )).toEqual([
      { text: 'libraries.updateSummary.updateAvailable(1)', tone: 'accent' },
      { text: 'libraries.updateSummary.deletedUpstream(1)', tone: 'warning' },
      { text: 'libraries.updateSummary.incomplete', tone: 'warning' },
    ]);
  });
});
