/* @vitest-environment jsdom */

import '@/test-utils';
import { render, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { installSkills } from '@/hooks/useTauriApi';
import type { WizardState } from '../types';
import { InstallingStep } from '../InstallingStep';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  installSkills: vi.fn().mockResolvedValue({
    successful: [],
    failed: [],
    symlinkFallbackAgents: [],
    defaultAvailableAgents: [],
    privateAdaptedAgents: [],
    privateCopyAgents: [],
    targetDetails: [],
  }),
}));

vi.mock('@/utils/cross-storage-guidance', () => ({
  getCrossStorageFailureGuidance: () => 'crossStorage.failureGuidance',
}));

const installSkillsMock = vi.mocked(installSkills);

function makeState(): WizardState {
  return {
    step: 'installing',
    entryPoint: 'skills-panel',
    scope: 'project',
    projectPath: '/projects/eve-app',
    context: {
      environment: { kind: 'host' },
      scope: { scope: 'project', project_id: 'eve-app' },
    },
    source: 'owner/repo',
    fetchStatus: 'success',
    fetchError: null,
    gitRef: null,
    riskPolicy: null,
    riskAcknowledged: false,
    availableSkills: [],
    selectedSkills: ['demo'],
    skillFilter: null,
    skillSearchQuery: '',
    selectedAgents: ['eve'],
    selectedAgentTargets: [
      { agent: 'eve', subagent: null },
      { agent: 'eve', subagent: 'research' },
    ],
    privateCopyAgents: [],
    allAgents: [],
    mode: 'copy',
    otherAgentsExpanded: false,
    privateCopyAgentsExpanded: false,
    otherAgentsSearchQuery: '',
    overwrites: {},
    confirmReady: true,
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    retrySkillName: undefined,
    retryAgents: undefined,
    retryAgentTargets: undefined,
  } as unknown as WizardState;
}

describe('InstallingStep', () => {
  beforeEach(() => {
    installSkillsMock.mockClear();
  });

  it('passes concrete Eve targets to installSkills', async () => {
    render(
      <InstallingStep
        state={makeState()}
        updateState={() => undefined}
        scope="project"
        projectPath="/projects/eve-app"
      />
    );

    await waitFor(() => {
      expect(installSkillsMock).toHaveBeenCalledWith(makeState().context, expect.objectContaining({
        agents: [],
        agentTargets: [
          { agent: 'eve', subagent: null },
          { agent: 'eve', subagent: 'research' },
        ],
      }));
    });
  });

  it('preserves concrete Eve targets when retrying one failed skill', async () => {
    render(
      <InstallingStep
        state={{
          ...makeState(),
          retrySkillName: 'demo',
          retryAgents: [],
          retryAgentTargets: [{ agent: 'eve', subagent: 'research' }],
        }}
        updateState={() => undefined}
        scope="project"
        projectPath="/projects/eve-app"
      />
    );

    await waitFor(() => {
      expect(installSkillsMock).toHaveBeenCalledWith(makeState().context, expect.objectContaining({
        skills: ['demo'],
        agents: [],
        agentTargets: [{ agent: 'eve', subagent: 'research' }],
        retry: true,
      }));
    });
  });

  it('installs into the explicit target context', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    } as const;

    render(
      <InstallingStep
        state={{ ...makeState(), context }}
        updateState={() => undefined}
        scope="project"
        projectPath="/projects/eve-app"
      />
    );

    await waitFor(() => {
      expect(installSkillsMock).toHaveBeenCalledWith(context, expect.objectContaining({
        skills: ['demo'],
      }));
    });
  });

  it('adds storage-owner guidance when project installation throws', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    } as const;
    const updateState = vi.fn();
    installSkillsMock.mockRejectedValueOnce(new Error('permission denied'));

    render(
      <InstallingStep
        state={{ ...makeState(), context }}
        updateState={updateState}
        scope="project"
        projectPath="/mnt/c/Code/app"
      />
    );

    await waitFor(() => expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      step: 'error',
      installError: expect.objectContaining({
        suggestions: expect.arrayContaining(['crossStorage.failureGuidance']),
      }),
    })));
  });
});
