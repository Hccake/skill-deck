/* @vitest-environment jsdom */

import '@/test-utils';
import { render, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { installSkills } from '@/hooks/useTauriApi';
import type { WizardState } from '../types';
import { InstallingStep } from '../InstallingStep';
import type { PreparedInstall } from '@/workflows/skill-install-preparation';

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

function makePreparedInstall(): PreparedInstall {
  const request = {
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
      ownDirectorySelected: false,
      adapterTargets: ['eve:root', 'eve:research'],
    }],
    requestedMode: 'copy',
    acknowledgeRisk: false,
  } as never;
  const preview = {
    token: {
      generation: 'preview-1',
      registryRevision: 'registry-1',
      environmentRevision: 'environment-1',
      contextRevision: 'context-1',
    },
    skills: [],
  } as never;
  return { request, preview };
}

function makeState(prepared = makePreparedInstall()): WizardState {
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
    preparation: { status: 'ready', prepared },
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
  } as unknown as WizardState;
}

describe('InstallingStep', () => {
  beforeEach(() => {
    installSkillsMock.mockClear();
  });

  it('executes the exact request and token accepted on the confirmation step', async () => {
    const prepared = makePreparedInstall();
    render(
      <InstallingStep
        state={makeState(prepared)}
        prepared={prepared}
        updateState={() => undefined}
        scope="project"
        projectPath="/projects/eve-app"
      />
    );

    await waitFor(() => {
      expect(installSkillsMock).toHaveBeenCalledWith(
        prepared.request,
        prepared.preview.token,
      );
    });
  });

  it('does not rebuild the accepted request from mutable wizard selections', async () => {
    const prepared = makePreparedInstall();
    render(
      <InstallingStep
        state={{
          ...makeState(prepared),
          selectedSkills: ['changed-after-preview'],
          selectedAgentOptionIds: [],
        }}
        prepared={prepared}
        updateState={() => undefined}
        scope="project"
        projectPath="/projects/eve-app"
      />
    );

    await waitFor(() => {
      expect(installSkillsMock).toHaveBeenCalledWith(
        prepared.request,
        prepared.preview.token,
      );
    });
  });

  it('installs into the explicit target context', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    } as const;
    const prepared = makePreparedInstall();
    const contextualPrepared = {
      ...prepared,
      request: { ...prepared.request, context },
    };

    render(
      <InstallingStep
        state={{
          ...makeState(contextualPrepared),
          context,
        }}
        prepared={contextualPrepared}
        updateState={() => undefined}
        scope="project"
        projectPath="/projects/eve-app"
      />
    );

    await waitFor(() => {
      expect(installSkillsMock).toHaveBeenCalledWith(
        expect.objectContaining({ context, skills: ['demo'] }),
        prepared.preview.token,
      );
    });
  });

  it('adds storage-owner guidance when project installation throws', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    } as const;
    const updateState = vi.fn();
    const prepared = makePreparedInstall();
    installSkillsMock.mockRejectedValueOnce(new Error('permission denied'));

    render(
      <InstallingStep
        state={{ ...makeState(prepared), context }}
        prepared={prepared}
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
