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
      capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
      reason: null,
      gitRef: null,
      sourceUrl: null,
      skillPath: null,
      freshness: 'fresh',
    } satisfies SkillUpdateInfo,
  ]));
}

const line = (statuses: SkillUpdateCheckStatus[], hasError = false) => (
  formatLibraryUpdateSummary(summarizeLibraryUpdates(checks(...statuses), hasError), t)
);

describe('library update summary', () => {
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
      summarizeLibraryUpdates(checks('upToDate'), false),
      t,
    )).toEqual([
      { text: 'libraries.updateSummary.allUpToDate', tone: 'neutral' },
    ]);

    expect(formatLibraryUpdateSummaryItems(
      summarizeLibraryUpdates(checks('updateAvailable', 'deletedUpstream'), true),
      t,
    )).toEqual([
      { text: 'libraries.updateSummary.updateAvailable(1)', tone: 'accent' },
      { text: 'libraries.updateSummary.deletedUpstream(1)', tone: 'warning' },
      { text: 'libraries.updateSummary.incomplete', tone: 'warning' },
    ]);
  });
});
