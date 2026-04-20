/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
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
});
