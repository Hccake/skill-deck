/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { WizardState } from '../types';
import { ConfirmStep } from '../ConfirmStep';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  checkOverwrites: vi.fn().mockResolvedValue({}),
  checkSkillAudit: vi.fn().mockResolvedValue(null),
}));

function createState(): WizardState {
  return {
    step: 'confirm',
    entryPoint: 'skills-panel',
    scope: 'global',
    projectPath: undefined,
    source: 'openclaw/community-skills',
    fetchStatus: 'success',
    fetchError: null,
    gitRef: null,
    availableSkills: [{ name: 'demo', description: 'Demo', relativePath: 'skills/demo/SKILL.md', pluginName: null }],
    selectedSkills: ['demo'],
    skillFilter: null,
    skillSearchQuery: '',
    selectedAgents: ['codex'],
    allAgents: [],
    mode: 'symlink',
    otherAgentsExpanded: false,
    otherAgentsSearchQuery: '',
    overwrites: {},
    confirmReady: true,
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    installError: undefined,
    retrySkillName: undefined,
    retryAgents: undefined,
    riskPolicy: { kind: 'require-confirmation', code: 'openclaw' },
    riskAcknowledged: false,
  };
}

function createTrustState(
  trustFields: Record<string, unknown>,
  source = 'https://example.com'
): WizardState {
  return {
    ...createState(),
    source,
    riskPolicy: { kind: 'none', code: null },
    availableSkills: [
      {
        name: 'demo',
        description: 'Demo',
        relativePath: 'demo/SKILL.md',
        pluginName: null,
        ...trustFields,
      } as never,
    ],
  };
}

describe('ConfirmStep', () => {
  it('renders guarded-source risk confirmation UI', () => {
    render(
      <ConfirmStep
        state={createState()}
        updateState={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('addSkill.risk.openclawTitle')).toBeTruthy();
    expect(screen.getByText('addSkill.risk.openclawAcknowledge')).toBeTruthy();
  });

  it('toggles riskAcknowledged when the shadcn checkbox is clicked', async () => {
    const updateState = vi.fn();
    render(
      <ConfirmStep
        state={createState()}
        updateState={updateState}
        scope="global"
      />
    );

    const checkbox = screen.getByRole('checkbox');
    await userEvent.click(checkbox);

    expect(updateState).toHaveBeenCalledWith({ riskAcknowledged: true });
  });

  it('shows legacy well-known trust metadata without digest verification', () => {
    render(
      <ConfirmStep
        state={createTrustState({
          wellKnownVersion: '0.1.0',
          wellKnownEntryType: 'legacy',
          trustReason: 'legacy',
        })}
        updateState={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('addSkill.confirm.trust.legacy')).toBeTruthy();
    expect(screen.queryByText('addSkill.confirm.trust.digestVerified')).toBeNull();
  });

  it('shows v2 well-known artifact host, type, and digest verification', () => {
    render(
      <ConfirmStep
        state={createTrustState({
          wellKnownVersion: '0.2.0',
          wellKnownEntryType: 'skill-md',
          artifactUrlHost: 'assets.example.com',
          digestVerified: true,
          trustReason: 'digest-verified',
        })}
        updateState={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('addSkill.confirm.trust.skillMd')).toBeTruthy();
    expect(screen.getByText('assets.example.com')).toBeTruthy();
    expect(screen.getByText('addSkill.confirm.trust.digestVerified')).toBeTruthy();
  });
});
