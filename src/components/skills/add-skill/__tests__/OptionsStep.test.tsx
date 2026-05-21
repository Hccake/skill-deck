/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
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
const getDefaultTargetAgentsMock = vi.fn();
const getLastSelectedAgentsMock = vi.fn<() => Promise<string[]>>();

vi.mock('@/hooks/useTauriApi', () => ({
  listAgents: () => listAgentsMock(),
  getDefaultTargetAgents: () => getDefaultTargetAgentsMock(),
  getLastSelectedAgents: () => getLastSelectedAgentsMock(),
}));

vi.mock('../AgentSelector', () => ({
  AgentSelector: ({ selectedAgents, scope }: { selectedAgents: string[]; scope: string }) => (
    <div>
      agent-selector:{scope}:{selectedAgents.join(',')}
    </div>
  ),
}));

function makeAgent(agent: Omit<AgentInfo, 'targets'> & {
  globalAutomatic?: boolean;
  projectAutomatic?: boolean;
}): AgentInfo {
  return {
    ...agent,
    targets: {
      global: {
        supported: true,
        automatic: agent.globalAutomatic ?? false,
        path: agent.globalSkillsDir,
      },
      project: {
        supported: true,
        automatic: agent.projectAutomatic ?? false,
        path: agent.skillsDir,
      },
    },
  };
}

function makeAutomaticGlobalAgent(agent: Omit<AgentInfo, 'targets'>): AgentInfo {
  return {
    ...agent,
    targets: {
      global: {
        supported: true,
        automatic: true,
        path: '~/.agents/skills',
      },
      project: {
        supported: true,
        automatic: agent.skillsDir === '.agents/skills',
        path: agent.skillsDir,
      },
    },
  };
}

function makeScopeAwareAgent(
  agent: Omit<AgentInfo, 'targets'>,
  targets: AgentInfo['targets'],
): AgentInfo {
  return {
    ...agent,
    targets,
  };
}

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

function ProjectHarness() {
  const [state, setState] = useState<WizardState>(() => ({
    ...createState(),
    scope: 'project',
  }));
  return (
    <OptionsStep
      state={state}
      updateState={(updates) => setState((current) => ({ ...current, ...updates }))}
    />
  );
}

describe('OptionsStep', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getDefaultTargetAgentsMock.mockResolvedValue(null);
    getLastSelectedAgentsMock.mockResolvedValue([]);
  });

  it('hides mode radios when only the shared directory is relevant', async () => {
    listAgentsMock.mockResolvedValue([
      makeAutomaticGlobalAgent({
        id: 'amp',
        name: 'Amp',
        skillsDir: '.agents/skills',
        globalSkillsDir: '~/.config/agents/skills',
        detected: true,
      }),
      makeAutomaticGlobalAgent({
        id: 'warp',
        name: 'Warp',
        skillsDir: '.agents/skills',
        globalSkillsDir: '~/.agents/skills',
        detected: true,
      }),
    ]);
    render(<Harness />);

    await waitFor(() => {
      expect(screen.getByText('addSkill.mode.singleDirectoryHint')).toBeDefined();
    });

    expect(screen.getByText('addSkill.mode.title')).toBeDefined();
    expect(screen.queryByText('addSkill.mode.symlink')).toBeNull();
    expect(screen.queryByText('addSkill.mode.copy')).toBeNull();
  });

  it('passes scope to the agent selector and uses persisted defaults for that scope', async () => {
    listAgentsMock.mockResolvedValue([
      makeScopeAwareAgent({
        id: 'antigravity',
        name: 'Antigravity',
        skillsDir: '.agents/skills',
        globalSkillsDir: '~/.gemini/antigravity/skills',
        detected: true,
      }, {
        global: {
          supported: true,
          automatic: false,
          path: '~/.gemini/antigravity/skills',
        },
        project: {
          supported: true,
          automatic: true,
          path: '.agents/skills',
        },
      }),
      makeAgent({
        id: 'claude-code',
        name: 'Claude Code',
        skillsDir: '.claude/skills',
        globalSkillsDir: '~/.claude/skills',
        detected: true,
      }),
    ]);
    getDefaultTargetAgentsMock.mockResolvedValue({
      global: ['antigravity', 'claude-code'],
      project: ['antigravity', 'claude-code'],
    });

    render(<ProjectHarness />);

    await waitFor(() => {
      expect(screen.getByText('agent-selector:project:claude-code')).toBeDefined();
    });
  });

  it('starts persisted default loading without waiting for agents to finish loading', async () => {
    let resolveAgents!: (value: AgentInfo[]) => void;
    listAgentsMock.mockReturnValue(new Promise<AgentInfo[]>((resolve) => {
      resolveAgents = resolve;
    }));
    getDefaultTargetAgentsMock.mockResolvedValue({
      global: ['claude-code'],
      project: ['claude-code'],
    });
    getLastSelectedAgentsMock.mockResolvedValue([]);

    render(<Harness />);

    await waitFor(() => {
      expect(listAgentsMock).toHaveBeenCalledTimes(1);
      expect(getDefaultTargetAgentsMock).toHaveBeenCalledTimes(1);
      expect(getLastSelectedAgentsMock).toHaveBeenCalledTimes(1);
    });

    resolveAgents([
      makeAgent({
        id: 'claude-code',
        name: 'Claude Code',
        skillsDir: '.claude/skills',
        globalSkillsDir: '~/.claude/skills',
        detected: true,
      }),
    ]);

    await waitFor(() => {
      expect(screen.getByText('agent-selector:global:claude-code')).toBeDefined();
    });
  });

  it('shows install mode choices with a distinct recommended badge', async () => {
    listAgentsMock.mockResolvedValue([
      makeAutomaticGlobalAgent({
        id: 'warp',
        name: 'Warp',
        skillsDir: '.agents/skills',
        globalSkillsDir: '~/.agents/skills',
        detected: true,
      }),
      makeAgent({
        id: 'claude-code',
        name: 'Claude Code',
        skillsDir: '.claude/skills',
        globalSkillsDir: '~/.claude/skills',
        detected: true,
      }),
    ]);

    render(<Harness />);

    await waitFor(() => {
      expect(screen.getByText('agent-selector:global:claude-code')).toBeDefined();
    });

    expect(screen.getByText('addSkill.mode.title')).toBeDefined();
    expect(screen.getByText('addSkill.mode.symlink')).toBeDefined();
    expect(screen.getByText('addSkill.mode.copy')).toBeDefined();
    expect(screen.getByText('addSkill.mode.recommended')).toBeDefined();
  });
});
