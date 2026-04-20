/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import type { AgentInfo } from '@/bindings';
import type { WizardState } from '../types';
import { OptionsStep } from '../OptionsStep';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const listAgentsMock = vi.fn<() => Promise<AgentInfo[]>>();
const getLastSelectedAgentsMock = vi.fn<() => Promise<string[]>>();

vi.mock('@/hooks/useTauriApi', () => ({
  listAgents: () => listAgentsMock(),
  getLastSelectedAgents: () => getLastSelectedAgentsMock(),
}));

vi.mock('../AgentSelector', () => ({
  AgentSelector: () => <div>agent-selector</div>,
}));

function createState(): WizardState {
  return {
    step: 'options',
    entryPoint: 'skills-panel',
    scope: 'global',
    projectPath: undefined,
    source: 'test/repo',
    fetchStatus: 'success',
    fetchError: null,
    gitRef: null,
    riskPolicy: null,
    riskAcknowledged: false,
    availableSkills: [],
    selectedSkills: ['demo-skill'],
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

function Harness() {
  const [state, setState] = useState<WizardState>(createState());
  return (
    <OptionsStep
      state={state}
      updateState={(updates) => setState((current) => ({ ...current, ...updates }))}
    />
  );
}

describe('OptionsStep', () => {
  it('hides mode radios when only the shared directory is relevant', async () => {
    listAgentsMock.mockResolvedValue([
      {
        id: 'amp',
        name: 'Amp',
        skillsDir: '.agents/skills',
        globalSkillsDir: '~/.config/agents/skills',
        detected: true,
        isUniversal: true,
        showInUniversalList: true,
      },
      {
        id: 'warp',
        name: 'Warp',
        skillsDir: '.agents/skills',
        globalSkillsDir: '~/.agents/skills',
        detected: true,
        isUniversal: true,
        showInUniversalList: true,
      },
    ]);
    getLastSelectedAgentsMock.mockResolvedValue([]);

    render(<Harness />);

    await waitFor(() => {
      expect(screen.getByText('addSkill.mode.singleDirectoryHint')).toBeDefined();
    });

    expect(screen.queryByText('addSkill.mode.title')).toBeNull();
  });
});
