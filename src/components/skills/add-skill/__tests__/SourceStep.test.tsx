/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import type { WizardState } from '../types';
import { SourceStep } from '../SourceStep';
import type { SearchSkill } from '../../skill-search/SkillSearch';
import { contextKey } from '@/lib/context';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const fetchAvailableMock = vi.fn();
const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
  listeners: [] as Array<(event: { payload: unknown }) => void>,
}));

vi.mock('@/hooks/useTauriApi', () => ({
  fetchAvailable: (...args: unknown[]) => fetchAvailableMock(...args),
}));

vi.mock('@tauri-apps/api/event', () => eventMocks);

const hostGlobal = {
  environment: { kind: 'host' },
  scope: { scope: 'global' },
} as const;
const discoverySession = {
  sessionId: 'discovery-1',
  environment: hostGlobal.environment,
  sourceFingerprint: 'source-1',
  expiresAtEpochMs: 1000,
} as const;

const skillSnapshots: Record<string, { skills: Array<{ name: string; source: string }> }> = {};

vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector: (state: { snapshots: typeof skillSnapshots }) => unknown) =>
    selector({ snapshots: skillSnapshots }),
}));

function SearchResultStub({
  installedSkillKeys,
  onInstall,
}: {
  installedSkillKeys: Set<string>;
  onInstall: (skill: SearchSkill) => void;
}) {
  return (
    <>
      <span data-testid="installed-skill-keys">{[...installedSkillKeys].join(',')}</span>
      <button
        type="button"
        onClick={() =>
          onInstall({
            name: 'demo',
            slug: 'demo',
            source: 'openclaw/community-skills',
            installs: 10,
          })
        }
      >
        install search result
      </button>
    </>
  );
}

vi.mock('../../skill-search', () => ({
  SkillSearch: SearchResultStub,
}));

vi.mock('../../skill-search/SkillSearch', () => ({
  SkillSearch: SearchResultStub,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function createState(): WizardState {
  return {
    step: 'source',
    entryPoint: 'skills-panel',
    scope: 'global',
    context: hostGlobal,
    projectPath: undefined,
    source: '',
    fetchStatus: 'idle',
    fetchError: null,
    gitRef: null,
    availableSkills: [],
    selectedSkills: [],
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
    installError: undefined,
    retrySkillName: undefined,
    retryAgents: undefined,
    riskPolicy: null,
    riskAcknowledged: false,
  };
}

function Harness({
  onNext,
  initialState = createState(),
  autoFetch = false,
}: {
  onNext: () => void;
  initialState?: WizardState;
  autoFetch?: boolean;
}) {
  const [state, setState] = useState<WizardState>(initialState);
  return (
    <>
      <SourceStep
        state={state}
        updateState={(updates) => setState((current) => ({ ...current, ...updates }))}
        onNext={onNext}
        autoFetch={autoFetch}
      />
      <div data-testid="risk-policy">{state.riskPolicy?.kind ?? 'none'}</div>
      <div data-testid="discovery-session">{state.discoverySession?.sessionId ?? 'none'}</div>
    </>
  );
}

describe('SourceStep', () => {
  beforeEach(() => {
    fetchAvailableMock.mockReset();
    eventMocks.listen.mockReset();
    eventMocks.listeners.length = 0;
    eventMocks.listen.mockImplementation((_eventName, callback) => {
      eventMocks.listeners.push(callback);
      return Promise.resolve(() => {});
    });
    for (const key of Object.keys(skillSnapshots)) delete skillSnapshots[key];
  });

  it('stores risk policy from fetchAvailable for Host Global', async () => {
    const onNext = vi.fn();

    fetchAvailableMock.mockResolvedValue({
      discoverySession,
      sourceType: 'github',
      sourceUrl: 'https://github.com/openclaw/community-skills',
      gitRef: null,
      skillFilter: null,
      riskPolicy: { kind: 'require-confirmation', code: 'openclaw' },
      skills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'skills/demo/SKILL.md' }],
    });

    render(
      <Harness onNext={onNext} />
    );

    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: 'openclaw/community-skills' },
    });
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });

    await waitFor(() => {
      expect(onNext).toHaveBeenCalled();
      expect(screen.getByTestId('risk-policy').textContent).toBe('require-confirmation');
      expect(screen.getByTestId('discovery-session').textContent).toBe('discovery-1');
    });
    expect(fetchAvailableMock).toHaveBeenCalledWith(
      hostGlobal,
      'openclaw/community-skills',
      expect.any(String),
    );
  });

  it('fetches a selected search result without waiting for a timer tick', async () => {
    const user = userEvent.setup();
    const onNext = vi.fn();

    fetchAvailableMock.mockResolvedValue({
      discoverySession,
      sourceType: 'github',
      sourceUrl: 'https://github.com/openclaw/community-skills',
      gitRef: null,
      skillFilter: 'demo',
      riskPolicy: { kind: 'none', code: null },
      skills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'skills/demo/SKILL.md' }],
    });

    render(<Harness onNext={onNext} />);

    await user.click(screen.getByRole('tab', { name: 'addSkill.source.tabs.search' }));
    await user.click(await screen.findByText('install search result'));

    expect(fetchAvailableMock).toHaveBeenCalledWith(
      hostGlobal,
      'openclaw/community-skills@demo',
      expect.any(String),
    );

    await waitFor(() => {
      expect(onNext).toHaveBeenCalled();
    });
  });

  it('fetches from the explicit target context', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    } as const;
    fetchAvailableMock.mockResolvedValue({
      discoverySession: { ...discoverySession, environment: context.environment },
      sourceType: 'github',
      sourceUrl: 'https://github.com/owner/repo',
      gitRef: null,
      skillFilter: null,
      riskPolicy: { kind: 'none', code: null },
      skills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'SKILL.md' }],
    });

    render(
      <SourceStep
        state={{ ...createState(), source: 'owner/repo', context }}
        updateState={() => undefined}
        onNext={() => undefined}
        autoFetch
      />
    );

    await waitFor(() => expect(fetchAvailableMock).toHaveBeenCalledWith(
      context,
      'owner/repo',
      expect.any(String),
    ));
  });

  it('ignores progress events from a previous source fetch operation', async () => {
    const fetchResult = deferred<Awaited<ReturnType<typeof fetchAvailableMock>>>();
    fetchAvailableMock.mockReturnValue(fetchResult.promise);

    render(
      <Harness
        onNext={() => undefined}
        initialState={{ ...createState(), source: 'owner/repo' }}
        autoFetch
      />
    );

    await waitFor(() => {
      expect(eventMocks.listeners).toHaveLength(1);
      expect(fetchAvailableMock).toHaveBeenCalled();
    });
    const operationId = fetchAvailableMock.mock.calls[0][2] as string;
    const emitProgress = eventMocks.listeners[0];

    await act(async () => {
      emitProgress({
        payload: {
          operation_id: 'previous-operation',
          phase: 'cloning',
          elapsed_secs: 42,
          timeout_secs: 120,
          message: null,
        },
      });
    });
    expect(screen.getByText('addSkill.source.status.cloning')).toBeTruthy();
    expect(screen.queryByText('addSkill.source.status.cloningWithTime')).toBeNull();

    await act(async () => {
      emitProgress({
        payload: {
          operation_id: operationId,
          phase: 'cloning',
          elapsed_secs: 2,
          timeout_secs: 120,
          message: null,
        },
      });
    });
    expect(screen.getByText('addSkill.source.status.cloningWithTime')).toBeTruthy();

    await act(async () => {
      fetchResult.resolve({
        discoverySession,
        sourceType: 'github',
        sourceUrl: 'https://github.com/owner/repo',
        gitRef: null,
        skillFilter: null,
        riskPolicy: { kind: 'none', code: null },
        skills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'SKILL.md' }],
      });
    });
  });

  it('does not write a late fetch result after SourceStep unmounts', async () => {
    const fetchResult = deferred<Awaited<ReturnType<typeof fetchAvailableMock>>>();
    fetchAvailableMock.mockReturnValue(fetchResult.promise);
    const updateState = vi.fn();
    const onNext = vi.fn();
    const { unmount } = render(
      <SourceStep
        state={{ ...createState(), source: 'owner/repo' }}
        updateState={updateState}
        onNext={onNext}
        autoFetch
      />
    );

    await waitFor(() => expect(fetchAvailableMock).toHaveBeenCalled());
    updateState.mockClear();
    unmount();

    fetchResult.resolve({
      discoverySession,
      sourceType: 'github',
      sourceUrl: 'https://github.com/owner/repo',
      gitRef: null,
      skillFilter: null,
      riskPolicy: { kind: 'none', code: null },
      skills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'SKILL.md' }],
    });
    await Promise.resolve();
    await Promise.resolve();

    expect(updateState).not.toHaveBeenCalled();
    expect(onNext).not.toHaveBeenCalled();
  });

  it('marks installed skills from the wizard context snapshot only', async () => {
    const otherContext = {
      environment: { kind: 'wsl', distro_name: 'Debian' },
      scope: { scope: 'global' },
    } as const;
    skillSnapshots[contextKey(hostGlobal)] = {
      skills: [{ name: 'host-skill', source: 'owner/host' }],
    };
    skillSnapshots[contextKey(otherContext)] = {
      skills: [{ name: 'debian-skill', source: 'owner/debian' }],
    };

    render(<Harness onNext={() => undefined} />);
    await userEvent.click(screen.getByRole('tab', { name: 'addSkill.source.tabs.search' }));

    expect(screen.getByTestId('installed-skill-keys').textContent).toBe('owner/host::host-skill');
  });
});
