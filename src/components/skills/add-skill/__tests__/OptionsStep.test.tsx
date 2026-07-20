/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentRuntimeSnapshot, ResolvedAgent } from '@/bindings';
import type { InstallTargetOptionsController } from '@/hooks/useInstallTargetOptions';
import { makeResolvedAgent, makeResolvedAgentScope } from '@/test-utils';
import { canProceedForStep, shouldShowInstallModeSelection, type WizardState } from '../types';
import { OptionsStep } from '../OptionsStep';

const mocks = vi.hoisted(() => ({
  configure: vi.fn(),
  onSaved: null as null | ((snapshot: AgentRuntimeSnapshot, agentId: string) => void),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/hooks/useAgentConfigurationFlow', () => ({
  useAgentConfigurationFlow: ({ onSaved }: {
    onSaved: (snapshot: AgentRuntimeSnapshot, agentId: string) => void;
  }) => {
    mocks.onSaved = onSaved;
    return {
      configuringAgentId: null,
      configurationResult: null,
      configure: mocks.configure,
    };
  },
}));

vi.mock('@/components/agents/AgentSelector', () => ({
  AgentSelector: ({
    selectedAgents,
    allAgents,
    selectionGroups,
    unknownAgentIds,
    onSelectionChange,
    onConfigureAgent,
  }: {
    selectedAgents: string[];
    allAgents: ResolvedAgent[];
    selectionGroups: Array<{ groupId: string }>;
    unknownAgentIds: string[];
    onSelectionChange: (agents: string[]) => void;
    onConfigureAgent: (agentId: string) => void;
  }) => (
    <div>
      <span>selected:{selectedAgents.join(',')}</span>
      <span>agents:{allAgents.map((agent) => agent.definition.id).join(',')}</span>
      <span>groups:{selectionGroups.map((group) => group.groupId).join(',')}</span>
      <span>unknown:{unknownAgentIds.join(',')}</span>
      <button type="button" onClick={() => onSelectionChange(['private-agent'])}>select-private</button>
      {unknownAgentIds.map((id) => (
        <button type="button" key={id} onClick={() => onConfigureAgent(id)}>configure:{id}</button>
      ))}
    </div>
  ),
}));

function privateAgent(id = 'private-agent') {
  return makeResolvedAgent({
    id,
    global: makeResolvedAgentScope({
      readsShared: false,
      privatePath: `~/.${id}/skills`,
      readPaths: [`~/.${id}/skills`],
    }),
  });
}

function createState(overrides: Partial<WizardState> = {}): WizardState {
  return {
    step: 'options',
    entryPoint: 'skills-panel',
    scope: 'global',
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
    availableAgentTargets: [],
    selectedAgentTargets: [],
    mode: 'symlink',
    otherAgentsExpanded: false,
    privateCopyAgentsExpanded: false,
    otherAgentsSearchQuery: '',
    overwrites: {},
    preparation: { status: 'idle' },
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    ...overrides,
  };
}

function readyController(
  allAgents: ResolvedAgent[] = [],
  overrides: Partial<Extract<InstallTargetOptionsController, { status: 'ready' }>> = {},
): InstallTargetOptionsController {
  return {
    status: 'ready',
    inputKey: 'host/global',
    facts: {
      allAgents,
      selectionGroups: [],
      availableAgentTargets: [],
      defaultAgents: [],
      defaultsUnavailable: false,
    },
    retry: vi.fn(),
    acceptConfiguredAgent: vi.fn(),
    ...overrides,
  };
}

function renderStep({
  state = createState(),
  targetOptions = readyController(),
}: {
  state?: WizardState;
  targetOptions?: InstallTargetOptionsController;
} = {}) {
  const updateState = vi.fn();
  render(
    <OptionsStep
      state={state}
      updateState={updateState}
      targetOptions={targetOptions}
    />,
  );
  return { updateState };
}

describe('OptionsStep', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.onSaved = null;
  });

  it('blocks confirmation while a preselected Agent ID is still unknown', () => {
    expect(canProceedForStep(createState({ preSelectedAgents: ['private-agent'] }))).toBe(false);
  });

  it('uses install paths when deciding whether mode selection is needed', () => {
    const sharedCompatibleAgent = makeResolvedAgent({
      id: 'firebender',
      global: makeResolvedAgentScope({
        readsShared: true,
        sharedPath: '~/.agents/skills',
        privatePath: '~/.firebender/skills',
      }),
    });

    expect(shouldShowInstallModeSelection({
      allAgents: [sharedCompatibleAgent],
      selectedAgents: [],
      scope: 'global',
    })).toBe(false);
  });

  it('renders loading and exposes a retry for required fact failures', () => {
    const retry = vi.fn().mockResolvedValue(undefined);
    const base = {
      inputKey: 'host/global',
      retry,
      acceptConfiguredAgent: vi.fn(),
    };
    const { rerender } = render(
      <OptionsStep
        state={createState()}
        updateState={vi.fn()}
        targetOptions={{ ...base, status: 'loading' }}
      />,
    );
    expect(screen.getByRole('status').textContent).toBe('common.loading');

    rerender(
      <OptionsStep
        state={createState()}
        updateState={vi.fn()}
        targetOptions={{
          ...base,
          status: 'error',
          error: { kind: 'custom', data: { message: 'unavailable' } },
        }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }));
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it('renders ready facts and keeps selection changes in Wizard state', () => {
    const agent = privateAgent();
    const { updateState } = renderStep({
      targetOptions: readyController([agent], {
        facts: {
          allAgents: [agent],
          selectionGroups: [{ groupId: 'private-group', agentIds: ['private-agent'] }],
          availableAgentTargets: [],
          defaultAgents: [],
          defaultsUnavailable: false,
        },
      }),
    });

    expect(screen.getByText('agents:private-agent')).toBeDefined();
    expect(screen.getByText('groups:private-group')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'select-private' }));
    expect(updateState).toHaveBeenCalledWith({
      selectedAgents: ['private-agent'],
      selectedAgentTargets: [],
    });
  });

  it('shows a non-blocking warning when only saved defaults are unavailable', () => {
    renderStep({
      targetOptions: readyController([], {
        facts: {
          allAgents: [],
          selectionGroups: [],
          availableAgentTargets: [],
          defaultAgents: null,
          defaultsUnavailable: true,
        },
      }),
    });

    expect(screen.getByText('addSkill.agents.defaultLoadWarning')).toBeDefined();
    expect(screen.getByText('addSkill.mode.symlink')).toBeDefined();
  });

  it('delegates unknown Agent configuration to the configuration flow', () => {
    renderStep({ state: createState({ preSelectedAgents: ['private-agent'] }) });
    fireEvent.click(screen.getByRole('button', { name: 'configure:private-agent' }));
    expect(mocks.configure).toHaveBeenCalledWith('private-agent');
  });
});
