/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ScopeStep } from '../ScopeStep';
import type { WizardState } from '../types';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (key === 'addSkill.scopeSelect.globalHint') {
        return `Global hint: ${String(options?.path ?? '')}`;
      }
      return key;
    },
  }),
}));

vi.mock('@/stores/context', () => ({
  useContextStore: (selector: (state: { projects: string[] }) => unknown) =>
    selector({ projects: ['D:/Code/hccake/skill-deck'] }),
}));

function createState(): WizardState {
  return {
    step: 'scope',
    entryPoint: 'skills-panel',
    scope: 'global',
    projectPath: undefined,
    source: '',
    fetchStatus: 'idle',
    fetchError: null,
    gitRef: null,
    riskPolicy: null,
    riskAcknowledged: false,
    availableSkills: [],
    selectedSkills: [],
    skillFilter: null,
    skillSearchQuery: '',
    selectedAgents: [],
    allAgents: [],
    mode: 'symlink',
    otherAgentsExpanded: false,
    otherAgentsSearchQuery: '',
    overwrites: {},
    confirmReady: false,
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    installError: undefined,
    retrySkillName: undefined,
    retryAgents: undefined,
  };
}

describe('ScopeStep', () => {
  it('uses the normalized shared directory path in the global option', () => {
    render(<ScopeStep state={createState()} updateState={vi.fn()} />);

    expect(screen.getByText('Global hint: ~/.agents/skills')).toBeDefined();
    expect(screen.queryByText('Global hint: ~/.agents/skills/')).toBeNull();
  });
});
