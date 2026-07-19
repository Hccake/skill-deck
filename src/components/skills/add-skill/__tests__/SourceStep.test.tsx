/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
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

vi.mock('@/hooks/useTauriApi', () => ({
  fetchAvailable: (...args: unknown[]) => fetchAvailableMock(...args),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

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
    riskPolicy: null,
    riskAcknowledged: false,
  };
}

function Harness({ onNext }: { onNext: () => void }) {
  const [state, setState] = useState<WizardState>(createState());
  return (
    <>
      <SourceStep
        state={state}
        updateState={(updates) => setState((current) => ({ ...current, ...updates }))}
        onNext={onNext}
      />
      <div data-testid="risk-policy">{state.riskPolicy?.kind ?? 'none'}</div>
      <div data-testid="discovery-session">{state.discoverySession?.sessionId ?? 'none'}</div>
    </>
  );
}

describe('SourceStep', () => {
  beforeEach(() => {
    fetchAvailableMock.mockReset();
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
    expect(fetchAvailableMock).toHaveBeenCalledWith(hostGlobal, 'openclaw/community-skills');
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

    await waitFor(() => expect(fetchAvailableMock).toHaveBeenCalledWith(context, 'owner/repo'));
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
