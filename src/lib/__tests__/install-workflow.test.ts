import { describe, expect, it } from 'vitest';
import { hasFailedMutationUnits } from '../install-workflow';

describe('install workflow model', () => {
  it('treats every non-succeeded mutation unit as an incomplete install', () => {
    expect(hasFailedMutationUnits({ units: [], warnings: [] })).toBe(false);
    expect(hasFailedMutationUnits({
      units: [{ status: 'notRun' } as never],
      warnings: [],
    })).toBe(true);
  });
});
