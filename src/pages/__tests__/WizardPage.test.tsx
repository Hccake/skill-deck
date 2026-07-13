/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it } from 'vitest';
import { canProceedForStep, getStepFlow } from '@/components/skills/add-skill/types';
import type { WizardState } from '@/components/skills/add-skill/types';
import { parseWizardContext } from '@/components/skills/add-skill/wizard-context';

function createState(overrides: Partial<WizardState> = {}): WizardState {
  return {
    step: 'confirm',
    entryPoint: 'skills-panel',
    scope: 'global',
    projectPath: undefined,
    source: 'openclaw/community-skills',
    fetchStatus: 'success',
    fetchError: null,
    gitRef: null,
    availableSkills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'skills/demo/SKILL.md', pluginName: null }],
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
    ...overrides,
    privateCopyAgents: overrides.privateCopyAgents ?? [],
    privateCopyAgentsExpanded: overrides.privateCopyAgentsExpanded ?? false,
  };
}

describe('canProceedForStep', () => {
  it('blocks install on confirm step until guarded-source risk is acknowledged', () => {
    expect(canProceedForStep(createState())).toBe(false);
    expect(canProceedForStep(createState({ riskAcknowledged: true }))).toBe(true);
  });
});

describe('getStepFlow', () => {
  it('uses the selected context directly for Skills entry', () => {
    expect(getStepFlow('skills-panel')[0]).toBe('source');
  });

  it('keeps scope selection for Discover entry', () => {
    expect(getStepFlow('discovery')[0]).toBe('scope');
  });
});

describe('parseWizardContext', () => {
  it('restores the explicit WSL target from the wizard query', () => {
    expect(parseWizardContext(JSON.stringify({
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    }))).toEqual({
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    });
  });

  it('ignores malformed wizard context', () => {
    expect(parseWizardContext('{invalid')).toBeUndefined();
  });
});
