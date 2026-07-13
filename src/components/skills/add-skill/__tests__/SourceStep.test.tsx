/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import type { WizardState } from '../types';
import { SourceStep } from '../SourceStep';
import type { SearchSkill } from '../../skill-search/SkillSearch';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const fetchAvailableMock = vi.fn();
const fetchAvailableV2Mock = vi.fn();

vi.mock('@/hooks/useTauriApi', () => ({
  fetchAvailable: (source: string) => fetchAvailableMock(source),
  fetchAvailableV2: (...args: unknown[]) => fetchAvailableV2Mock(...args),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector: (state: { globalSkills: []; projectSkills: [] }) => unknown) =>
    selector({
      globalSkills: [],
      projectSkills: [],
    }),
}));

function SearchResultStub({ onInstall }: { onInstall: (skill: SearchSkill) => void }) {
  return (
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
    </>
  );
}

describe('SourceStep', () => {
  beforeEach(() => {
    fetchAvailableMock.mockReset();
    fetchAvailableV2Mock.mockReset();
  });

  it('stores risk policy from fetchAvailable', async () => {
    const onNext = vi.fn();

    fetchAvailableMock.mockResolvedValue({
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
    });
  });

  it('fetches a selected search result without waiting for a timer tick', async () => {
    const user = userEvent.setup();
    const onNext = vi.fn();

    fetchAvailableMock.mockResolvedValue({
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

    expect(fetchAvailableMock).toHaveBeenCalledWith('openclaw/community-skills@demo');

    await waitFor(() => {
      expect(onNext).toHaveBeenCalled();
    });
  });

  it('fetches from the explicit target context', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    } as const;
    fetchAvailableV2Mock.mockResolvedValue({
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

    await waitFor(() => expect(fetchAvailableV2Mock).toHaveBeenCalledWith(context, 'owner/repo'));
    expect(fetchAvailableMock).not.toHaveBeenCalled();
  });
});
