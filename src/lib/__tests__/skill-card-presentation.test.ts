import { describe, expect, it } from 'vitest';
import { formatSkillCardDate } from '../skill-card-presentation';

describe('formatSkillCardDate', () => {
  const now = new Date(2026, 7, 13, 18, 0);

  it('shows only the time for a date from today', () => {
    const date = new Date(2026, 7, 13, 14, 30);
    const result = formatSkillCardDate(date.toISOString(), 'zh-CN', now);

    expect(result.short).toBe(new Intl.DateTimeFormat('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    }).format(date));
    expect(result.full).toBe(new Intl.DateTimeFormat('zh-CN', {
      dateStyle: 'long',
      timeStyle: 'short',
    }).format(date));
  });

  it('shows month and day without the year for another date this year', () => {
    const result = formatSkillCardDate(new Date(2026, 6, 2, 14, 30).toISOString(), 'zh-CN', now);

    expect(result.short).toContain('7');
    expect(result.short).toContain('2');
    expect(result.short).not.toContain('2026');
  });

  it('includes the year for a date from an earlier year', () => {
    const result = formatSkillCardDate(new Date(2025, 11, 18, 14, 30).toISOString(), 'zh-CN', now);

    expect(result.short).toContain('2025');
  });

  it('uses the requested English date ordering', () => {
    const date = new Date(2026, 6, 2, 14, 30);
    const result = formatSkillCardDate(date.toISOString(), 'en-US', now);

    expect(result.short).toBe(new Intl.DateTimeFormat('en-US', {
      month: 'short',
      day: 'numeric',
    }).format(date));
    expect(result.full).toBe(new Intl.DateTimeFormat('en-US', {
      dateStyle: 'long',
      timeStyle: 'short',
    }).format(date));
  });

  it('preserves an invalid source value instead of inventing a date', () => {
    expect(formatSkillCardDate('not-a-date', 'en-US', now)).toEqual({
      short: 'not-a-date',
      full: 'not-a-date',
    });
  });
});
