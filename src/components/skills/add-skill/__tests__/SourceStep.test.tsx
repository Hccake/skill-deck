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
  discoverSkillSource: (...args: unknown[]) => fetchAvailableMock(...args),
}));

vi.mock('@tauri-apps/api/event', () => eventMocks);

const nativeGlobal = {
  environment: { kind: 'native' },
  scope: { scope: 'global' },
} as const;
const discoverySession = {
  sessionId: 'discovery-1',
  environment: nativeGlobal.environment,
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
    context: nativeGlobal,
    projectPath: undefined,
    source: '',
    sourceInput: '',
    fetchStatus: 'idle',
    fetchError: null,
    gitRef: null,
    availableSkills: [],
    selectedSkills: [],
    skillFilter: null,
    skillSearchQuery: '',
    overwrites: {},
    preparation: { status: 'idle' },
    agentSelectionIntent: { wildcardRequested: false, explicitAgentIds: [] },
    installResults: null,
    installError: undefined,
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
      <div data-testid="discovery-session">{state.discoverySession?.sessionId ?? 'none'}</div>
      <div data-testid="redirect-host">{state.redirectedDownloadHost ?? 'none'}</div>
      <div data-testid="selected-skills">{state.selectedSkills.join(',')}</div>
      <div data-testid="fetch-status">{state.fetchStatus}</div>
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

  it('stores the discovery session returned for Native Global', async () => {
    const onNext = vi.fn();

    fetchAvailableMock.mockResolvedValue({
      discoverySession,
      sourceType: 'github',
      sourceUrl: 'https://github.com/openclaw/community-skills',
      gitRef: null,
      skillFilter: null,
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
      expect(screen.getByTestId('discovery-session').textContent).toBe('discovery-1');
    });
    expect(fetchAvailableMock).toHaveBeenCalledWith(
      nativeGlobal.environment,
      'openclaw/community-skills',
      expect.any(String),
      { wildcardRequested: false, explicitSkillNames: [] },
    );
  });

  it('enables manual fetch from the current source input', async () => {
    fetchAvailableMock.mockResolvedValue({
      discoverySession,
      sourceType: 'github',
      sourceUrl: 'https://github.com/openclaw/community-skills',
      gitRef: null,
      skillFilter: null,
      skills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'skills/demo/SKILL.md' }],
    });

    render(<Harness onNext={() => undefined} />);

    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: 'openclaw/community-skills' },
    });
    const fetchButton = screen.getByRole('button', {
      name: 'addSkill.source.actions.fetch',
    });
    expect((fetchButton as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(fetchButton);

    await waitFor(() => expect(fetchAvailableMock).toHaveBeenCalledWith(
      nativeGlobal.environment,
      'openclaw/community-skills',
      expect.any(String),
      { wildcardRequested: false, explicitSkillNames: [] },
    ));
  });

  it('invalidates a successful discovery when the source input changes', () => {
    render(
      <Harness
        onNext={() => undefined}
        initialState={{
          ...createState(),
          sourceInput: 'owner/source-a',
          source: 'owner/source-a',
          fetchStatus: 'success',
          discoverySession,
          availableSkills: [
            { name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'SKILL.md' },
          ],
          selectedSkills: ['demo'],
          agentSelectionIntent: {
            wildcardRequested: true,
            explicitAgentIds: [],
          },
        }}
      />,
    );

    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: 'owner/source-b' },
    });

    expect(screen.getByTestId('fetch-status').textContent).toBe('idle');
    expect(screen.getByTestId('discovery-session').textContent).toBe('none');
    expect(screen.getByTestId('selected-skills').textContent).toBe('');
  });

  it('does not auto-fetch partial edits after returning to a prefilled source', async () => {
    render(
      <Harness
        onNext={() => undefined}
        autoFetch
        initialState={{
          ...createState(),
          sourceInput: 'owner/source-a',
          source: 'owner/source-a',
          fetchStatus: 'success',
          discoverySession,
          availableSkills: [
            { name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'SKILL.md' },
          ],
          selectedSkills: ['demo'],
        }}
      />,
    );

    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: 'o' },
    });

    await act(async () => {
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    });
    expect(fetchAvailableMock).not.toHaveBeenCalled();
    expect(screen.getByRole('textbox')).toHaveProperty('value', 'o');
  });

  it('stores the final host when a download redirects across hosts', async () => {
    const onNext = vi.fn();
    fetchAvailableMock.mockResolvedValue({
      discoverySession,
      sourceType: 'download',
      sourceUrl: 'https://example.com/SKILL.md',
      redirectedDownloadHost: 'cdn.example.net',
      gitRef: null,
      skillFilter: null,
      skills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'SKILL.md' }],
    });

    render(<Harness onNext={onNext} />);
    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: 'https://example.com/SKILL.md' },
    });
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });

    await waitFor(() => {
      expect(onNext).toHaveBeenCalled();
      expect(screen.getByTestId('redirect-host').textContent).toBe('cdn.example.net');
    });
  });

  it.each([
    {
      source: 'https://skills.sh/p/frontend',
      skillFilter: null,
      expected: 'alpha,beta',
    },
    {
      source: 'https://skills.sh/p/frontend@alpha',
      skillFilter: 'alpha',
      expected: 'alpha',
    },
    {
      source: 'skills add https://skills.sh/p/frontend --skill beta',
      skillFilter: null,
      expected: 'beta',
    },
    {
      source: 'owner/repo',
      skillFilter: null,
      expected: '',
    },
  ])('derives the initial selection for $source', async ({ source, skillFilter, expected }) => {
    const onNext = vi.fn();
    fetchAvailableMock.mockResolvedValue({
      discoverySession,
      sourceType: 'well-known',
      sourceUrl: 'https://skills.sh/p/frontend',
      gitRef: null,
      skillFilter,
      skills: [
        { name: 'alpha', installDirName: 'alpha', description: 'Alpha', relativePath: 'alpha' },
        { name: 'beta', installDirName: 'beta', description: 'Beta', relativePath: 'beta' },
      ],
    });

    render(<Harness onNext={onNext} />);
    fireEvent.change(screen.getByRole('textbox'), { target: { value: source } });
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });

    await waitFor(() => expect(onNext).toHaveBeenCalled());
    expect(screen.getByTestId('selected-skills').textContent).toBe(expected);
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
      skills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'skills/demo/SKILL.md' }],
    });

    render(<Harness onNext={onNext} />);

    await user.click(screen.getByRole('tab', { name: 'addSkill.source.tabs.search' }));
    await user.click(await screen.findByText('install search result'));

    expect(fetchAvailableMock).toHaveBeenCalledWith(
      nativeGlobal.environment,
      'openclaw/community-skills@demo',
      expect.any(String),
      { wildcardRequested: false, explicitSkillNames: [] },
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
      skills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'SKILL.md' }],
    });

    render(
      <SourceStep
        state={{ ...createState(), sourceInput: 'owner/repo', context }}
        updateState={() => undefined}
        onNext={() => undefined}
        autoFetch
      />
    );

    await waitFor(() => expect(fetchAvailableMock).toHaveBeenCalledWith(
      context.environment,
      'owner/repo',
      expect.any(String),
      { wildcardRequested: false, explicitSkillNames: [] },
    ));
  });

  it('sends wildcard and exact Skill selectors as one request intent', async () => {
    fetchAvailableMock.mockResolvedValue({
      discoverySession,
      sourceType: 'github',
      sourceUrl: 'https://github.com/owner/repo',
      gitRef: null,
      skillFilter: null,
      skills: [
        { name: 'public-one', installDirName: 'public-one', description: 'Public one', relativePath: 'public-one/SKILL.md' },
        { name: 'public-two', installDirName: 'public-two', description: 'Public two', relativePath: 'public-two/SKILL.md' },
      ],
    });

    render(<Harness onNext={() => undefined} />);
    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: 'skills add owner/repo --skill internal-one --skill * --skill internal-two' },
    });
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });

    await waitFor(() => expect(fetchAvailableMock).toHaveBeenCalledWith(
      nativeGlobal.environment,
      'owner/repo',
      expect.any(String),
      {
        wildcardRequested: true,
        explicitSkillNames: ['internal-one', 'internal-two'],
      },
    ));
    expect(screen.getByTestId('selected-skills').textContent).toBe('public-one,public-two');
  });

  it('preserves CLI Skill selection intent when the same input is fetched again', async () => {
    fetchAvailableMock.mockResolvedValue({
      discoverySession,
      sourceType: 'github',
      sourceUrl: 'https://github.com/owner/repo',
      gitRef: null,
      skillFilter: null,
      skills: [
        { name: 'public-one', installDirName: 'public-one', description: 'Public one', relativePath: 'public-one/SKILL.md' },
      ],
    });

    render(<Harness onNext={() => undefined} />);
    const sourceInput = screen.getByRole('textbox');
    fireEvent.change(sourceInput, {
      target: { value: 'skills add owner/repo --skill internal-one * internal-two' },
    });
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });
    await waitFor(() => expect(fetchAvailableMock).toHaveBeenCalledTimes(1));

    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });
    await waitFor(() => expect(fetchAvailableMock).toHaveBeenCalledTimes(2));

    expect(fetchAvailableMock.mock.calls.map((call) => call[3])).toEqual([
      { wildcardRequested: true, explicitSkillNames: ['internal-one', 'internal-two'] },
      { wildcardRequested: true, explicitSkillNames: ['internal-one', 'internal-two'] },
    ]);
  });

  it('stores Agent wildcard selection imported through --all', async () => {
    const updateState = vi.fn();
    fetchAvailableMock.mockResolvedValue({
      discoverySession,
      sourceType: 'github',
      sourceUrl: 'https://github.com/owner/repo',
      gitRef: null,
      skillFilter: null,
      skills: [
        { name: 'public-one', installDirName: 'public-one', description: 'Public one', relativePath: 'public-one/SKILL.md' },
      ],
    });

    render(
      <SourceStep
        state={{ ...createState(), sourceInput: 'skills add owner/repo --all' }}
        updateState={updateState}
        onNext={() => undefined}
        autoFetch
      />,
    );

    await waitFor(() => expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      agentSelectionIntent: {
        wildcardRequested: true,
        explicitAgentIds: [],
      },
    })));
  });

  it('ignores progress events from a previous source fetch operation', async () => {
    const fetchResult = deferred<Awaited<ReturnType<typeof fetchAvailableMock>>>();
    fetchAvailableMock.mockReturnValue(fetchResult.promise);

    render(
      <Harness
        onNext={() => undefined}
        initialState={{ ...createState(), sourceInput: 'owner/repo' }}
        autoFetch
      />
    );

    await waitFor(() => {
      expect(eventMocks.listeners).toHaveLength(1);
      expect(fetchAvailableMock).toHaveBeenCalled();
    });
    expect(screen.getByText('owner/repo')).toBeTruthy();
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
    expect(screen.queryByRole('progressbar')).toBeNull();

    await act(async () => {
      fetchResult.resolve({
        discoverySession,
        sourceType: 'github',
        sourceUrl: 'https://github.com/owner/repo',
        gitRef: null,
        skillFilter: null,
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
        state={{ ...createState(), sourceInput: 'owner/repo' }}
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
    skillSnapshots[contextKey(nativeGlobal)] = {
      skills: [{ name: 'native-skill', source: 'owner/native' }],
    };
    skillSnapshots[contextKey(otherContext)] = {
      skills: [{ name: 'debian-skill', source: 'owner/debian' }],
    };

    render(<Harness onNext={() => undefined} />);
    await userEvent.click(screen.getByRole('tab', { name: 'addSkill.source.tabs.search' }));

    expect(screen.getByTestId('installed-skill-keys').textContent).toBe('owner/native::native-skill');
  });
});
