/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import type {
  AgentId,
  AgentRuntimeSnapshot,
  AgentSelectionGroup,
  ResolvedAgent,
  ResolvedAgentScope,
} from '@/bindings';
import { makeAgentRuntimeSnapshot, makeResolvedScopeFixture, makeResolvedAgent } from '@/test-utils';
import { canProceedForStep, shouldShowInstallModeSelection, type WizardState } from '../types';
import { OptionsStep } from '../OptionsStep';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const listAgentsMock = vi.fn();
const listAgentSelectionGroupsMock = vi.fn();
const listEveInstallTargetsMock = vi.fn();
const getDefaultTargetAgentsMock = vi.fn();
const configureAgentMock = vi.fn();
let configuredAgentSaved: ((snapshot: AgentRuntimeSnapshot, agentId: AgentId) => void) | undefined;

vi.mock('@/hooks/useTauriApi', () => ({
  listAgents: (...args: unknown[]) => listAgentsMock(...args),
  listAgentSelectionGroups: (...args: unknown[]) => listAgentSelectionGroupsMock(...args),
  listEveInstallTargets: (...args: unknown[]) => listEveInstallTargetsMock(...args),
  getDefaultTargetAgents: (...args: unknown[]) => getDefaultTargetAgentsMock(...args),
}));

vi.mock('@/hooks/useAgentConfigurationFlow', () => ({
  useAgentConfigurationFlow: ({
    onSaved,
  }: {
    onSaved: (snapshot: AgentRuntimeSnapshot, agentId: AgentId) => void;
  }) => {
    configuredAgentSaved = onSaved;
    return { configuringAgentId: null, configure: configureAgentMock };
  },
}));

vi.mock('@/components/agents/AgentSelector', () => ({
  AgentSelector: ({
    selectedAgents,
    scope,
    allAgents,
    selectionGroups,
    unknownAgentIds = [],
    onConfigureAgent,
  }: {
    selectedAgents: string[];
    scope: string;
    allAgents: ResolvedAgent[];
    selectionGroups: AgentSelectionGroup[];
    unknownAgentIds?: string[];
    onConfigureAgent?: (agentId: string) => void;
  }) => (
    <div>
      agent-selector:{scope}:{selectedAgents.join(',')}
      <span>all-agents:{allAgents.map((agent) => agent.definition.id).join(',')}</span>
      <span>selection-groups:{selectionGroups.map((group) => group.groupId).join(',')}</span>
      <span>unknown-agents:{unknownAgentIds.join(',')}</span>
      {unknownAgentIds.map((id) => <button key={id} onClick={() => onConfigureAgent?.(id)}>configure:{id}</button>)}
    </div>
  ),
}));

type AgentFixture = {
  id: string;
  name: string;
  skillsDir: string;
  globalSkillsDir: string;
  detected: boolean;
  globalAutomatic?: boolean;
  projectAutomatic?: boolean;
};

function makeAgent(agent: AgentFixture): ResolvedAgent {
  return makeResolvedAgent({
    id: agent.id,
    displayName: agent.name,
    detection: agent.detected ? 'detected' : 'notDetected',
    global: makeResolvedScopeFixture({
      automatic: agent.globalAutomatic ?? false,
      path: agent.globalSkillsDir,
    }),
    project: makeResolvedScopeFixture({
      automatic: agent.projectAutomatic ?? false,
      path: agent.skillsDir,
      sharedPath: './.agents/skills',
    }),
  });
}

function makeAutomaticGlobalAgent(agent: AgentFixture): ResolvedAgent {
  const resolved = makeAgent(agent);
  return {
    ...resolved,
    global: makeResolvedScopeFixture({
        automatic: true,
        path: '~/.agents/skills',
    }),
    project: makeResolvedScopeFixture({
        automatic: agent.skillsDir === '.agents/skills',
        path: agent.skillsDir,
        sharedPath: './.agents/skills',
    }),
  };
}

function makeScopeAwareAgent(
  agent: AgentFixture,
  targets: { global: ResolvedAgentScope; project: ResolvedAgentScope },
): ResolvedAgent {
  const resolved = makeAgent(agent);
  return {
    ...resolved,
    global: targets.global,
    project: targets.project,
  };
}

function runtimeSnapshot(agents: ResolvedAgent[]): AgentRuntimeSnapshot {
  return makeAgentRuntimeSnapshot(agents);
}

function createState(): WizardState {
  return {
    step: 'options',
    entryPoint: 'skills-panel',
    scope: 'global',
    projectPath: undefined,
    context: {
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    },
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
    context: {
      environment: { kind: 'host' },
      scope: { scope: 'project', project_id: 'eve-app' },
    },
  }));
  return (
    <OptionsStep
      state={state}
      updateState={(updates) => setState((current) => ({ ...current, ...updates }))}
    />
  );
}

function UnknownAgentHarness() {
  const [state, setState] = useState<WizardState>(() => ({
    ...createState(),
    preSelectedAgents: ['private-agent'],
  }));
  return <OptionsStep state={state} updateState={(updates) => setState((current) => ({ ...current, ...updates }))} />;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => { resolve = resolvePromise; });
  return { promise, resolve };
}

describe('OptionsStep', () => {
  it('blocks confirmation while a preselected Agent ID is still unknown', () => {
    expect(canProceedForStep({
      ...createState(),
      preSelectedAgents: ['private-agent'],
      step: 'options',
    })).toBe(false);
  });
  it('uses install paths, not private read paths, when deciding whether mode selection is needed', () => {
    const sharedCompatibleAgent = makeScopeAwareAgent({
      id: 'firebender',
      name: 'Firebender',
      skillsDir: '.agents/skills',
      globalSkillsDir: '~/.firebender/skills',
      detected: true,
    }, {
      global: makeResolvedScopeFixture({
        automatic: true,
        path: '~/.firebender/skills',
        availability: 'shared-compatible',
        sharedPath: '~/.agents/skills',
        privatePath: '~/.firebender/skills',
      }),
      project: makeResolvedScopeFixture({
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
    configuredAgentSaved = undefined;
    getDefaultTargetAgentsMock.mockResolvedValue(null);
    listAgentsMock.mockResolvedValue(runtimeSnapshot([]));
    listAgentSelectionGroupsMock.mockResolvedValue({ global: [], project: [] });
    listEveInstallTargetsMock.mockResolvedValue([]);
  });

  it('shows a recoverable error when Agent initialization fails', async () => {
    listAgentsMock.mockRejectedValueOnce(new Error('WSL unavailable'));
    render(<Harness />);

    expect(await screen.findByRole('alert')).toBeDefined();
    listAgentsMock.mockResolvedValueOnce(runtimeSnapshot([]));
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }));

    await waitFor(() => expect(listAgentsMock).toHaveBeenCalledTimes(2));
  });

  it('refreshes selection groups after an Agent configuration is saved', async () => {
    const configuredAgent = makeAgent({
      id: 'private-agent',
      name: 'Private Agent',
      skillsDir: '.private/skills',
      globalSkillsDir: '~/.private/skills',
      detected: true,
    });
    listAgentSelectionGroupsMock
      .mockResolvedValueOnce({ global: [], project: [] })
      .mockResolvedValueOnce({
        global: [{ groupId: 'shared-private-target', agentIds: ['private-agent'] }],
        project: [],
      });

    render(<Harness />);
    await screen.findByText('selection-groups:');

    act(() => configuredAgentSaved?.(runtimeSnapshot([configuredAgent]), 'private-agent'));

    await screen.findByText('selection-groups:shared-private-target');
    expect(listAgentSelectionGroupsMock).toHaveBeenCalledTimes(2);
  });

  it('ignores an older Agent response after the Environment changes', async () => {
    const hostRequest = deferred<AgentRuntimeSnapshot>();
    const wslRequest = deferred<AgentRuntimeSnapshot>();
    listAgentsMock
      .mockReturnValueOnce(hostRequest.promise)
      .mockReturnValueOnce(wslRequest.promise);
    const updateState = vi.fn();
    const hostState = createState();
    const { rerender } = render(<OptionsStep state={hostState} updateState={updateState} />);
    const wslState: WizardState = {
      ...hostState,
      context: {
        environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        scope: { scope: 'global' },
      },
    };
    rerender(<OptionsStep state={wslState} updateState={updateState} />);

    await act(async () => {
      wslRequest.resolve(runtimeSnapshot([makeAgent({
        id: 'wsl-agent', name: 'WSL Agent', skillsDir: '.wsl/skills',
        globalSkillsDir: '~/.wsl/skills', detected: true,
      })]));
    });
    await waitFor(() => expect(updateState).toHaveBeenCalled());
    await act(async () => {
      hostRequest.resolve(runtimeSnapshot([makeAgent({
        id: 'host-agent', name: 'Host Agent', skillsDir: '.host/skills',
        globalSkillsDir: '~/.host/skills', detected: true,
      })]));
    });

    expect(updateState.mock.calls.flatMap(([update]) => (
      update.allAgents?.map((agent: ResolvedAgent) => agent.definition.id) ?? []
    ))).not.toContain('host-agent');
  });

  it('loads project-aware agents using the selected project path', async () => {
    listAgentsMock.mockResolvedValue(runtimeSnapshot([
      makeAgent({
        id: 'eve',
        name: 'Eve',
        skillsDir: 'agent/skills',
        globalSkillsDir: '',
        detected: true,
      }),
    ]));

    render(<ProjectHarness />);

    await waitFor(() => {
      expect(listAgentsMock).toHaveBeenCalledWith({
        environment: { kind: 'host' },
        scope: { scope: 'project', project_id: 'eve-app' },
      });
    });
    expect(getDefaultTargetAgentsMock).toHaveBeenCalledWith({
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    });
    expect(listEveInstallTargetsMock).toHaveBeenCalledWith({
      environment: { kind: 'host' },
      scope: { scope: 'project', project_id: 'eve-app' },
    });
    await waitFor(() => {
      expect(screen.getByText('all-agents:eve')).toBeDefined();
    });
  });

  it('loads Eve targets from a WSL project context', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'eve-app' },
    } as const;
    listAgentsMock.mockResolvedValue(runtimeSnapshot([]));

    render(
      <OptionsStep
        state={{ ...createState(), scope: 'project', context }}
        updateState={() => undefined}
      />
    );

    await waitFor(() => expect(listEveInstallTargetsMock).toHaveBeenCalledWith(context));
  });

  it('loads agents and defaults from the explicit target context', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    } as const;
    listAgentsMock.mockResolvedValue(runtimeSnapshot([]));
    getDefaultTargetAgentsMock.mockResolvedValue({ global: [], project: [] });

    render(
      <OptionsStep
        state={{ ...createState(), context }}
        updateState={() => undefined}
      />
    );

    await waitFor(() => expect(listAgentsMock).toHaveBeenCalledWith(context));
    expect(getDefaultTargetAgentsMock).toHaveBeenCalledWith(context);
  });

  it('hides mode radios when only the shared directory is relevant', async () => {
    listAgentsMock.mockResolvedValue(runtimeSnapshot([
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
    ]));
    render(<Harness />);

    await waitFor(() => {
      expect(screen.getByText('addSkill.mode.singleDirectoryHint')).toBeDefined();
    });

    expect(screen.getByText('addSkill.mode.title')).toBeDefined();
    expect(screen.queryByText('addSkill.mode.symlink')).toBeNull();
    expect(screen.queryByText('addSkill.mode.copy')).toBeNull();
  });

  it('passes scope to the agent selector and uses persisted defaults for that scope', async () => {
    listAgentsMock.mockResolvedValue(runtimeSnapshot([
      makeScopeAwareAgent({
        id: 'antigravity',
        name: 'Antigravity',
        skillsDir: '.agents/skills',
        globalSkillsDir: '~/.gemini/antigravity/skills',
        detected: true,
      }, {
        global: makeResolvedScopeFixture({
          automatic: false,
          path: '~/.gemini/antigravity/skills',
        }),
        project: makeResolvedScopeFixture({
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
    ]));
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
    let resolveAgents!: (value: AgentRuntimeSnapshot) => void;
    listAgentsMock.mockReturnValue(new Promise<AgentRuntimeSnapshot>((resolve) => {
      resolveAgents = resolve;
    }));
    getDefaultTargetAgentsMock.mockResolvedValue({
      global: ['claude-code'],
      project: ['claude-code'],
    });

    render(<Harness />);

    await waitFor(() => {
      expect(listAgentsMock).toHaveBeenCalledTimes(1);
      expect(getDefaultTargetAgentsMock).toHaveBeenCalledTimes(1);
    });

    resolveAgents(runtimeSnapshot([
      makeAgent({
        id: 'claude-code',
        name: 'Claude Code',
        skillsDir: '.claude/skills',
        globalSkillsDir: '~/.claude/skills',
        detected: true,
      }),
    ]));

    await waitFor(() => {
      expect(screen.getByText('agent-selector:global:claude-code')).toBeDefined();
    });
  });

  it('shows install mode choices with a distinct recommended badge', async () => {
    listAgentsMock.mockResolvedValue(runtimeSnapshot([
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
    ]));

    render(<Harness />);

    await waitFor(() => {
      expect(screen.getByText('agent-selector:global:claude-code')).toBeDefined();
    });

    expect(screen.getByText('addSkill.mode.title')).toBeDefined();
    expect(screen.getByText('addSkill.mode.symlink')).toBeDefined();
    expect(screen.getByText('addSkill.mode.copy')).toBeDefined();
    expect(screen.getByText('addSkill.mode.recommended')).toBeDefined();
  });

  it('preserves unknown preselected Agent IDs and delegates configuration', async () => {
    listAgentsMock.mockResolvedValue(runtimeSnapshot([]));
    getDefaultTargetAgentsMock.mockResolvedValue(null);
    render(<UnknownAgentHarness />);

    await waitFor(() => expect(screen.getByText('unknown-agents:private-agent')).toBeDefined());
    fireEvent.click(screen.getByText('configure:private-agent'));
    expect(configureAgentMock).toHaveBeenCalledWith('private-agent');
  });
});
