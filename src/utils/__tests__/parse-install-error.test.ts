import { describe, expect, it } from 'vitest';
import type { AppError } from '@/bindings';
import { parseInstallError } from '../parse-install-error';

const t = (key: string, params?: Record<string, unknown>) =>
  `${key}${params ? JSON.stringify(params) : ''}`;

describe('parseInstallError', () => {
  it('includes configured timeout details and settings guidance for git timeouts', () => {
    const error = {
      kind: 'gitTimeout',
      data: { timeoutSecs: 300 },
    } as AppError & { data: { timeoutSecs: number } };

    const result = parseInstallError(
      error,
      t as never
    );

    expect(result.message).toContain('addSkill.error.cloneTimeout');
    expect(result.details).toContain('300');
    expect(result.suggestions).toContain('addSkill.error.suggestion.adjustCloneTimeout');
  });
});
