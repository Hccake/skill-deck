/* @vitest-environment jsdom */
/// <reference types="node" />

import { tmpdir } from 'node:os';
import { join, posix } from 'node:path';
import { StrictMode } from 'react';
import { MemoryRouter } from 'react-router-dom';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import type {
  AcquiredPayloadHandle,
  EnvironmentRef,
  FetchResult,
  InstallRequest,
  InstallResponse,
  PreviewToken,
  ProjectInfo,
  SkillLocationRef,
} from '@/bindings';

const mocks = vi.hoisted(() => ({
  projects: vi.fn<(environment: EnvironmentRef) => Promise<ProjectInfo[]>>(),
  emit: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
  emit: mocks.emit,
}));
vi.mock('@/lifecycle/useWindowLifecycle', () => ({
  useWindowLifecycle: () => ({ requestAction: vi.fn() }),
}));

const native: EnvironmentRef = { kind: 'native' };
const ubuntu: EnvironmentRef = { kind: 'wsl', distro_name: 'Ubuntu' };
const previewToken: PreviewToken = {
  generation: 'preview-1',
  registryRevision: 'registry-1',
  environmentRevision: 'environment-1',
  contextRevision: 'context-1',
};

function project(environment: EnvironmentRef): ProjectInfo {
  return {
    binding: {
      id: 'project-1',
      nativePath: environment.kind === 'native'
        ? join(tmpdir(), 'skill-deck-tests', 'project-one')
        : posix.join('/work', 'project-one'),
      displayName: 'Project One',
      order: null,
      suppressCrossStorageWarning: false,
    },
    storage: { access: 'native', owner: environment },
  };
}

function sourceResult(environment: EnvironmentRef): FetchResult {
  return {
    discoverySession: {
      sessionId: 'source-1',
      environment,
      sourceFingerprint: 'fingerprint-1',
      expiresAtEpochMs: Date.now() + 60_000,
    },
    sourceType: 'github',
    sourceUrl: 'https://github.com/owner/skills',
    gitRef: null,
    skillFilter: 'demo',
    redirectedDownloadHost: null,
    skills: [{
      name: 'demo',
      installDirName: 'demo',
      description: 'Demo Skill',
      relativePath: 'demo/SKILL.md',
      pluginName: null,
    }],
  };
}

async function openWizard(context: SkillLocationRef, entryPoint = 'discovery') {
  const { WizardPage } = await import('../WizardPage');
  const params = new URLSearchParams({
    entryPoint,
    context: JSON.stringify(context),
    environmentName: 'Test System',
    prefillSource: 'owner/skills',
    prefillSkillName: 'demo',
  });
  if (context.scope.scope === 'project') {
    params.set('projectPath', project(context.environment).binding.nativePath);
  }
  return render(
    <StrictMode>
      <TooltipProvider>
        <MemoryRouter initialEntries={[`/wizard?${params}`]}>
          <WizardPage />
        </MemoryRouter>
      </TooltipProvider>
    </StrictMode>,
  );
}

async function installSelectedSkill() {
  await screen.findByRole('checkbox', { name: /demo/ });
  fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
  await waitFor(() => expect(
    screen.getByRole('button', { name: 'addSkill.actions.next' }).hasAttribute('disabled'),
  ).toBe(false));
  fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
  const install = await screen.findByRole('button', { name: 'addSkill.actions.install' });
  await waitFor(() => expect(install.hasAttribute('disabled')).toBe(false));
  fireEvent.click(install);
  await screen.findByText('addSkill.complete.status.succeeded');
}

describe('Discovery installation with an independent window state', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.projects.mockReset().mockImplementation(async (environment) => [project(environment)]);
    vi.mocked(invoke).mockImplementation(async (command, arguments_) => {
      const args = arguments_ as Record<string, unknown>;
      switch (command) {
        case 'get_active_mutation':
          return { revision: 1, active: null };
        case 'list_environment_projects':
          return mocks.projects(args.environment as EnvironmentRef);
        case 'discover_skill_source':
          return sourceResult(args.environment as EnvironmentRef);
        case 'get_install_agent_selection':
          return { selection: makeAgentSelectionSnapshot(), selectionHistoryWarning: null };
        case 'confirm_install_agent_selection':
          return { status: 'ready', warning: null };
        case 'acquire_selected_payloads': {
          const { discoverySession } = args.request as {
            discoverySession: FetchResult['discoverySession'];
          };
          return [{
            ...discoverySession,
            skillPath: 'demo/SKILL.md',
            payloadId: 'payload-1',
            manifestHash: 'manifest-1',
          } satisfies AcquiredPayloadHandle];
        }
        case 'preview_install': {
          const request = args.request as InstallRequest;
          return {
            status: 'ready',
            preview: {
              token: previewToken,
              skills: [{
                skillName: 'demo',
                payload: request.payloads[0],
                overwriteTargets: [],
                blockingReasons: [],
                fallbackForecasts: [],
                overridesLibrary: false,
              }],
            },
          };
        }
        case 'install_skills': {
          const request = args.request as InstallRequest;
          return {
            units: [{
              unitId: 'unit-1',
              skillName: 'demo',
              source: null,
              target: request.context,
              status: 'succeeded',
              retryable: false,
              lockCommitted: true,
              actualMode: 'symlink',
              fallbackReason: null,
              agentTargets: [],
              warnings: [],
              error: null,
              recovery: null,
            }],
            warnings: [],
          } satisfies InstallResponse;
        }
        default:
          throw new Error(`Unexpected command: ${command}`);
      }
    });
  });

  it.each([native, ubuntu])('loads projects and installs into the chosen project in $kind', async (environment) => {
    await openWizard({ environment, scope: { scope: 'global' } });
    const option = await screen.findByRole('radio', { name: /Project One/ });
    expect(mocks.projects).toHaveBeenCalledExactlyOnceWith(environment);
    expect(screen.getByRole('radio', { name: /addSkill.scopeSelect.global/ })
      .getAttribute('aria-checked')).toBe('true');

    fireEvent.click(option);
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    await installSelectedSkill();

    const context: SkillLocationRef = {
      environment,
      scope: { scope: 'project', project_id: 'project-1' },
    };
    expect(invoke).toHaveBeenCalledWith('get_install_agent_selection', expect.objectContaining({ context }));
    expect(invoke).toHaveBeenCalledWith('preview_install', {
      request: expect.objectContaining({ context, skills: ['demo'] }),
    });
    expect(invoke).toHaveBeenCalledWith('install_skills', {
      request: expect.objectContaining({ context, skills: ['demo'] }),
      expectedToken: previewToken,
    });
    expect(mocks.emit).toHaveBeenCalledExactlyOnceWith('wizard-result', {
      action: 'refresh', context, mutatedSkillNames: ['demo'],
    });
  });

  it('defaults to Global and can continue while projects are loading', async () => {
    let finishLoading!: (projects: ProjectInfo[]) => void;
    mocks.projects.mockReturnValue(new Promise((resolve) => { finishLoading = resolve; }));
    await openWizard({ environment: native, scope: { scope: 'project', project_id: 'old-project' } });

    expect(screen.getByRole('radio', { name: /addSkill.scopeSelect.global/ })
      .getAttribute('aria-checked')).toBe('true');
    expect(screen.getByText('common.loading')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    await installSelectedSkill();

    expect(invoke).toHaveBeenCalledWith('install_skills', {
      request: expect.objectContaining({ context: { environment: native, scope: { scope: 'global' } } }),
      expectedToken: previewToken,
    });
    await act(async () => { finishLoading([]); });
  });

  it('shows a project loading failure and retries without changing the Global selection', async () => {
    mocks.projects.mockRejectedValueOnce({ kind: 'io', data: { message: 'Project list unavailable' } });
    await openWizard({ environment: ubuntu, scope: { scope: 'global' } });

    expect(await screen.findByRole('alert')).toBeDefined();
    expect(screen.getByText('context.projectsLoadError')).toBeDefined();
    expect(screen.getByRole('button', { name: 'addSkill.actions.next' }).hasAttribute('disabled')).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'context.environmentRetry' }));

    expect(await screen.findByRole('radio', { name: /Project One/ })).toBeDefined();
    expect(mocks.projects).toHaveBeenCalledTimes(2);
    expect(screen.getByRole('radio', { name: /addSkill.scopeSelect.global/ })
      .getAttribute('aria-checked')).toBe('true');
  });

  it('explains an empty project list while keeping Global available', async () => {
    mocks.projects.mockResolvedValue([]);
    await openWizard({ environment: native, scope: { scope: 'global' } });

    expect(await screen.findByText('addSkill.scopeSelect.noProjects')).toBeDefined();
    expect(screen.getAllByRole('radio')).toHaveLength(1);
    expect(screen.getByRole('button', { name: 'addSkill.actions.next' }).hasAttribute('disabled')).toBe(false);
  });

  it('keeps the explicit project for the Skills entry without loading the project list', async () => {
    const context: SkillLocationRef = {
      environment: native, scope: { scope: 'project', project_id: 'project-1' },
    };
    await openWizard(context, 'skills-panel');
    await installSelectedSkill();

    expect(mocks.projects).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith('install_skills', {
      request: expect.objectContaining({ context }), expectedToken: previewToken,
    });
  });
});
