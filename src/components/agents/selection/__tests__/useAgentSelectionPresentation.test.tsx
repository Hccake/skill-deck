/* @vitest-environment jsdom */

import { renderHook } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { useAgentSelectionPresentation } from '../useAgentSelectionPresentation';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

describe('useAgentSelectionPresentation', () => {
  it.each([
    ['install', 'install'],
    ['manage', 'manage'],
    ['copyToProject', 'copyToProject'],
    ['libraryApplication', 'libraryApplication'],
  ] as const)('provides complete %s copy without exposing the usage to the view', (usage, suffix) => {
    const { result } = renderHook(() => useAgentSelectionPresentation(usage));

    expect(result.current.automatic).toEqual({
      title: `agentSelection.automatic.${suffix}.title`,
      help: `agentSelection.automatic.${suffix}.help`,
    });
    expect(result.current.selectable).toEqual({
      title: 'agentSelection.selectable.title',
      help: 'agentSelection.selectable.help',
    });
    expect(result.current.ownDirectory.title).toBe('agentSelection.ownDirectory.title');
    expect(result.current.ownDirectory.description).toBe(
      `agentSelection.ownDirectory.${suffix}.description`,
    );
    expect(result.current.ownDirectory.selectedCount(2)).toBe(
      'agentSelection.ownDirectory.selectedCount:{"count":2}',
    );
  });
});
