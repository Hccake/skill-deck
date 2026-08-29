/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { SkillsStep } from '../SkillsStep';
import type { WizardState } from '../types';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: { count?: number; total?: number }) => (
      key === 'addSkill.skills.selected'
        ? `${values?.count} of ${values?.total}`
        : key
    ),
  }),
}));

const initialState: WizardState = {
  step: 'skills',
  entryPoint: 'skills-panel',
  scope: 'global',
  context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
  sourceInput: 'owner/repo',
  source: 'owner/repo',
  fetchStatus: 'success',
  fetchError: null,
  gitRef: null,
  availableSkills: [
    { name: 'alpha', installDirName: 'alpha', description: 'Frontend utility', relativePath: 'alpha/SKILL.md' },
    { name: 'beta', installDirName: 'beta', description: 'Backend utility', relativePath: 'beta/SKILL.md' },
    { name: 'gamma', installDirName: 'gamma', description: 'Backend utility', relativePath: 'gamma/SKILL.md' },
  ],
  selectedSkills: ['beta'],
  skillFilter: null,
  skillSearchQuery: '',
  overwrites: {},
  preparation: { status: 'idle' },
  agentSelectionIntent: { wildcardRequested: false, explicitAgentIds: [] },
  installResults: null,
};

function Harness() {
  const [state, setState] = useState(initialState);
  return (
    <>
      <SkillsStep
        state={state}
        updateState={(updates) => setState((current) => ({
          ...current,
          ...(typeof updates === 'function' ? updates(current) : updates),
        }))}
      />
      <output aria-label="selection">{state.selectedSkills.join(',')}</output>
    </>
  );
}

describe('SkillsStep', () => {
  it('maps installation Skills to shared candidates and selects only visible results', async () => {
    const user = userEvent.setup();
    render(<Harness />);

    await user.type(
      screen.getByRole('searchbox', { name: 'addSkill.skills.search' }),
      'frontend',
    );
    await user.click(screen.getByRole('button', { name: 'addSkill.skills.selectAll' }));

    expect(screen.getByRole('status', { name: 'selection' }).textContent).toBe('beta,alpha');
  });
});
