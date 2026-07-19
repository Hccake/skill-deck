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
    units: [],
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
      { agentId: 'eve', targetId: 'eve:root' },
      { agentId: 'eve', targetId: 'eve:research' },
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
    installRequest: {
      context: {
        environment: { kind: 'host' },
        scope: { scope: 'project', project_id: 'eve-app' },
      },
      source: 'owner/repo',
      discoverySession: {
        sessionId: 'discovery-1',
        environment: { kind: 'host' },
        sourceFingerprint: 'source-1',
        expiresAtEpochMs: 1000,
      },
      payloads: [],
      skills: ['demo'],
      agentIntents: [{
        agentId: 'eve',
        privateEntry: 'none',
        adapterTargets: ['eve:root', 'eve:research'],
      }],
      requestedMode: 'copy',
      acknowledgeRisk: false,
    },
    installPreview: {
      token: {
        generation: 'preview-1',
        registryRevision: 'registry-1',
        environmentRevision: 'environment-1',
        contextRevision: 'context-1',
      },
      skills: [],
    },
    retrySkillName: undefined,
    retryAgents: undefined,
    retryAgentTargets: undefined,
  } as unknown as WizardState;
}

describe('InstallingStep', () => {
  beforeEach(() => {
    installSkillsMock.mockClear();
  });

  it('executes the exact request and token accepted on the confirmation step', async () => {
    render(
      <InstallingStep
        state={makeState()}
        updateState={() => undefined}
        scope="project"
        projectPath="/projects/eve-app"
      />
    );

    await waitFor(() => {
      expect(installSkillsMock).toHaveBeenCalledWith(
        makeState().installRequest,
        makeState().installPreview?.token,
      );
    });
  });

  it('does not rebuild the accepted request from mutable wizard selections', async () => {
    render(
      <InstallingStep
        state={{
          ...makeState(),
          selectedSkills: ['changed-after-preview'],
          selectedAgentTargets: [],
        }}
        updateState={() => undefined}
        scope="project"
        projectPath="/projects/eve-app"
      />
    );

    await waitFor(() => {
      expect(installSkillsMock).toHaveBeenCalledWith(
        makeState().installRequest,
        makeState().installPreview?.token,
      );
    });
  });

  it('installs into the explicit target context', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    } as const;

    render(
      <InstallingStep
        state={{
          ...makeState(),
          context,
          installRequest: { ...makeState().installRequest!, context },
        }}
        updateState={() => undefined}
        scope="project"
        projectPath="/projects/eve-app"
      />
    );

    await waitFor(() => {
      expect(installSkillsMock).toHaveBeenCalledWith(
        expect.objectContaining({ context, skills: ['demo'] }),
        makeState().installPreview?.token,
      );
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
