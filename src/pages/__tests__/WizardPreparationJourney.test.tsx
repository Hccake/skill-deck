/* @vitest-environment jsdom */

import '@/test-utils';
import { StrictMode } from 'react';
import { MemoryRouter } from 'react-router-dom';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcquireSelectedPayloadsRequest, AvailableSkill, FetchResult } from '@/bindings';
import { TooltipProvider } from '@/components/ui/tooltip';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { useMutationStore } from '@/stores/mutation';
import { WizardPage } from '../WizardPage';

const api = vi.hoisted(() => ({
  discoverSkillSource: vi.fn(),
  acquireSelectedPayloads: vi.fn(),
  previewInstall: vi.fn(),
  getInstallAgentSelection: vi.fn(),
  confirmInstallAgentSelection: vi.fn(),
}));

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('@/hooks/useTauriApi', () => api);
vi.mock('@tauri-apps/api/event', () => ({
  emit: vi.fn().mockResolvedValue(undefined),
  listen: vi.fn().mockResolvedValue(() => undefined),
}));
vi.mock('@/hooks/useMutationMonitor', () => ({ useMutationMonitor: vi.fn() }));
vi.mock('@/lifecycle/useWindowLifecycle', () => ({
  useWindowLifecycle: () => ({ requestAction: vi.fn() }),
}));
vi.mock('@/components/skills/skill-search/SkillSearch', () => ({ SkillSearch: () => null }));

const context = { environment: { kind: 'native' }, scope: { scope: 'global' } } as const;

function discovery(sessionId: string, names: string[]): FetchResult {
  return {
    discoverySession: {
      sessionId,
      environment: context.environment,
      sourceFingerprint: sessionId,
      expiresAtEpochMs: 1000,
    },
    sourceType: 'github',
    sourceUrl: 'https://github.com/owner/repo',
    gitRef: null,
    skillFilter: null,
    skills: names.map((name): AvailableSkill => ({
      name, installDirName: name, description: name, relativePath: `skills/${name}`,
    })),
  };
}

describe('Wizard preparation recovery', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({ activeMutation: null, loading: false, cancelling: false });
    api.getInstallAgentSelection.mockResolvedValue({
      selection: makeAgentSelectionSnapshot({
        agents: [{ kind: 'standard', id: 'private-agent', displayName: 'Private Agent', detection: 'detected', directoryAccess: 'privateOnly', installOptionId: 'private-item', groupId: null }],
        installOptions: [{ id: 'private-item', kind: 'standardDirectory', agentIds: ['private-agent'], displayName: 'Private Agent', path: '~/.private-agent/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null }],
        userModeOptionIds: ['private-item'],
        baselineSelectedOptionIds: ['private-item'],
      }),
      selectionHistoryWarning: null,
    });
    api.confirmInstallAgentSelection.mockResolvedValue({ status: 'ready', warning: null });
    api.previewInstall.mockResolvedValue({
      status: 'ready',
      preview: {
        token: { generation: 'preview', registryRevision: 'registry', environmentRevision: 'environment', contextRevision: 'context' },
        skills: [{ skillName: 'demo', overwriteTargets: [], blockingReasons: [] }],
      },
    });
  });

  it('rediscovers expired content and confirms only selections still present in the new list', async () => {
    const user = userEvent.setup();
    api.discoverSkillSource
      .mockResolvedValueOnce(discovery('expired', ['demo', 'removed']))
      .mockResolvedValueOnce(discovery('fresh', ['demo', 'new-skill']));
    api.acquireSelectedPayloads.mockImplementation((request: AcquireSelectedPayloadsRequest) => (
      request.discoverySession.sessionId === 'expired'
        ? Promise.reject({ kind: 'payloadSessionExpired', data: { sessionId: 'expired' } })
        : Promise.resolve([])
    ));
    render(
      <StrictMode>
        <TooltipProvider>
          <MemoryRouter initialEntries={['/wizard?entryPoint=skills-panel']}>
            <WizardPage />
          </MemoryRouter>
        </TooltipProvider>
      </StrictMode>,
    );
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'owner/repo' } });
    await user.click(screen.getByRole('button', { name: 'addSkill.source.actions.fetch' }));
    await user.click(await screen.findByRole('checkbox', { name: 'demo' }));
    await user.click(screen.getByRole('checkbox', { name: 'removed' }));
    await user.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    await screen.findByRole('checkbox', { name: 'Private Agent' });
    await user.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));

    await user.click(await screen.findByRole('button', { name: 'addSkill.confirm.rediscoverSource' }));

    const retained = await screen.findByRole('checkbox', { name: 'demo' });
    expect(retained.getAttribute('aria-checked')).toBe('true');
    expect(screen.queryByRole('checkbox', { name: 'removed' })).toBeNull();
    expect(screen.getByRole('checkbox', { name: 'new-skill' }).getAttribute('aria-checked')).toBe('false');
    expect(api.discoverSkillSource).toHaveBeenCalledTimes(2);
    await user.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    await screen.findByRole('checkbox', { name: 'Private Agent' });
    await user.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));

    await waitFor(() => expect(screen.getByRole('button', { name: 'addSkill.actions.install' }).hasAttribute('disabled')).toBe(false));
    expect(api.previewInstall).toHaveBeenCalledWith(expect.objectContaining({
      context,
      skills: ['demo'],
      discoverySession: expect.objectContaining({ sessionId: 'fresh' }),
      agentSelection: expect.objectContaining({ selectedOptionIds: ['private-item'] }),
    }));
  });
});
