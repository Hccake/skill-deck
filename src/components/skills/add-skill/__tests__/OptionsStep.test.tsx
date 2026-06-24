/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import type { AgentInfo } from '@/bindings';
import { makeAgentScopeTarget } from '@/test-utils';
import { shouldShowInstallModeSelection, type WizardState } from '../types';
import { OptionsStep } from '../OptionsStep';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const listAgentsMock = vi.fn<() => Promise<AgentInfo[]>>();
const listAgentsForProjectMock = vi.fn<(projectPath?: string) => Promise<AgentInfo[]>>();
const listEveInstallTargetsMock = vi.fn();
const getDefaultTargetAgentsMock = vi.fn();
const getLastSelectedAgentsMock = vi.fn<() => Promise<string[]>>();

vi.mock('@/hooks/useTauriApi', () => ({
  listAgents: () => listAgentsMock(),
  listAgentsForProject: (projectPath?: string) => listAgentsForProjectMock(projectPath),
  listEveInstallTargets: (projectPath: string) => listEveInstallTargetsMock(projectPath),
  getDefaultTargetAgents: () => getDefaultTargetAgentsMock(),
  getLastSelectedAgents: () => getLastSelectedAgentsMock(),
}));

vi.mock('../AgentSelector', () => ({
  AgentSelector: ({
    selectedAgents,
    scope,
    allAgents,
  }: {
    selectedAgents: string[];
    scope: string;
    allAgents: AgentInfo[];
  }) => (
    <div>
      agent-selector:{scope}:{selectedAgents.join(',')}
      <span>all-agents:{allAgents.map((agent) => agent.id).join(',')}</span>
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
      global: makeAgentScopeTarget({
        automatic: agent.globalAutomatic ?? false,
        path: agent.globalSkillsDir,
      }),
      project: makeAgentScopeTarget({
        automatic: agent.projectAutomatic ?? false,
        path: agent.skillsDir,
        sharedPath: './.agents/skills',
      }),
    },
  };
}

function makeAutomaticGlobalAgent(agent: Omit<AgentInfo, 'targets'>): AgentInfo {
  return {
    ...agent,
    targets: {
      global: makeAgentScopeTarget({
        automatic: true,
        path: '~/.agents/skills',
      }),
      project: makeAgentScopeTarget({
        automatic: agent.skillsDir === '.agents/skills',
        path: agent.skillsDir,
        sharedPath: './.agents/skills',
      }),
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
    privateCopyAgents: [],
    allAgents: [],
    mode: 'symlink',
    otherAgentsExpanded: false,
    privateCopyAgentsExpanded: false,
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
    projectPath: '/projects/eve-app',
  }));
  return (
    <OptionsStep
      state={state}
      updateState={(updates) => setState((current) => ({ ...current, ...updates }))}
    />
  );
}

describe('OptionsStep', () => {
  it('uses install paths, not private read paths, when deciding whether mode selection is needed', () => {
    const sharedCompatibleAgent = makeScopeAwareAgent({
      id: 'firebender',
      name: 'Firebender',
      skillsDir: '.agents/skills',
      globalSkillsDir: '~/.firebender/skills',
      detected: true,
    }, {
      global: makeAgentScopeTarget({
        automatic: true,
        path: '~/.firebender/skills',
        availability: 'shared-compatible',
        sharedPath: '~/.agents/skills',
        installPath: '~/.agents/skills',
        privatePath: '~/.firebender/skills',
      }),
      project: makeAgentScopeTarget({
        automatic: true,
        path: '.agents/skills',
        sharedPath: './.agents/skills',
      }),
    });

    expect(shouldShowInstallModeSelection({
      allAgents: [sharedCompatibleAgent],
      selectedAgents: [],
      scope: 'global',
    })).toBe(false);
  });

  beforeEach(() => {
    vi.clearAllMocks();
    getDefaultTargetAgentsMock.mockResolvedValue(null);
    getLastSelectedAgentsMock.mockResolvedValue([]);
    listAgentsForProjectMock.mockResolvedValue([]);
    listEveInstallTargetsMock.mockResolvedValue([]);
  });

  it('loads project-aware agents using the selected project path', async () => {
    listAgentsMock.mockResolvedValue([]);
    listAgentsForProjectMock.mockResolvedValue([
      makeAgent({
        id: 'eve',
        name: 'Eve',
        skillsDir: 'agent/skills',
        globalSkillsDir: '',
        detected: true,
      }),
    ]);

    render(<ProjectHarness />);

    await waitFor(() => {
      expect(listAgentsForProjectMock).toHaveBeenCalledWith('/projects/eve-app');
    });
    expect(listAgentsMock).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(screen.getByText('all-agents:eve')).toBeDefined();
    });
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
    listAgentsForProjectMock.mockResolvedValue([
      makeScopeAwareAgent({
        id: 'antigravity',
        name: 'Antigravity',
        skillsDir: '.agents/skills',
        globalSkillsDir: '~/.gemini/antigravity/skills',
        detected: true,
      }, {
        global: makeAgentScopeTarget({
          automatic: false,
          path: '~/.gemini/antigravity/skills',
        }),
        project: makeAgentScopeTarget({
          automatic: true,
          path: '.agents/skills',
          sharedPath: './.agents/skills',
        }),
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
